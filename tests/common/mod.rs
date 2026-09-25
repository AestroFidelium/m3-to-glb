//! Test support: a synthetic M3 writer and a GLB validity oracle.
//!
//! Blizzard's models cannot live in a public repository, so the tests build
//! their own. [`ModelSpec`] describes a model in plain terms — vertices,
//! regions, bones, materials, animation tracks, effects — and
//! [`ModelSpec::build`] lays it out as a real M3 file: a tag table, 16-byte
//! aligned tag payloads, and references between them. The same writer feeds
//! the structure-aware fuzz targets, which start from files that parse instead
//! of from random noise.
//!
//! [`check_glb`] is the other half: everything the converter emits must be a
//! GLB that an independent loader accepts and whose internals are consistent.

#![allow(dead_code, reason = "each test binary uses a different subset")]

use bytemuck::{Pod, Zeroable};
use m3_to_glb::m3::structures::{
    Atvl, Bat, Bone, Cms, Div, Lite, Matm, Par, Proj, Reference, Regn, Schr, Seqs, SeqsV1, Stc, Stg,
};
use m3_to_glb::m3::{layr_record_size, layr_uv_tiling_offset, mat_record_size, stride_from_flags};
use m3_to_glb::m3::reader::M3File;
use m3_to_glb::processor::VertexOffsets;

/// A length or index as the `u32` the M3 format stores. Test fixtures stay far
/// below 4 GiB, so overflowing is a bug in the test.
pub fn len32(n: usize) -> u32 {
    u32::try_from(n).expect("test fixture larger than 4 GiB")
}

// ─── Raw tag writer ──────────────────────────────────────────────────────────

struct Tag {
    name:    [u8; 4],
    version: u32,
    data:    Vec<u8>,
    reps:    u32,
}

/// Lays out tags the way an M3 file does: a 12-byte header, each payload
/// 16-byte aligned, and the tag index at the end.
pub struct M3Writer {
    magic: [u8; 4],
    tags:  Vec<Tag>,
}

impl M3Writer {
    pub fn new(magic: [u8; 4]) -> Self {
        // Tag 0 is the header tag itself in real files; a reference with
        // `index == 0` therefore never points at useful data.
        let mut w = Self { magic, tags: Vec::new() };
        w.raw("MD34", 11, vec![0; 24], 1);
        w
    }

    /// Add a tag. `name` is the ASCII spelling (`"DIV_"`); it is stored
    /// byte-reversed, as on disk. Returns the tag index.
    pub fn raw(&mut self, name: &str, version: u32, data: Vec<u8>, reps: u32) -> u32 {
        let mut n: [u8; 4] = name.as_bytes().try_into().expect("tag names are 4 bytes");
        n.reverse();
        self.tags.push(Tag { name: n, version, data, reps });
        len32(self.tags.len() - 1)
    }

    /// Add a tag of `Pod` records and return a reference to all of them. An
    /// empty slice adds nothing and returns the null reference.
    pub fn pods<T: Pod>(&mut self, name: &str, version: u32, items: &[T]) -> Reference {
        if items.is_empty() {
            return Reference::zeroed();
        }
        let data = bytemuck::cast_slice(items).to_vec();
        let index = self.raw(name, version, data, len32(items.len()));
        Reference { entries: len32(items.len()), index, flags: 0 }
    }

    /// Records whose on-disk size differs from the Rust struct (older
    /// versions): each is pre-serialized to `record` bytes.
    pub fn records(&mut self, name: &str, version: u32, records: &[Vec<u8>]) -> Reference {
        if records.is_empty() {
            return Reference::zeroed();
        }
        let index = self.raw(name, version, records.concat(), len32(records.len()));
        Reference { entries: len32(records.len()), index, flags: 0 }
    }

    /// A NUL-terminated `CHAR` string. The empty string is the null reference.
    pub fn chars(&mut self, s: &str) -> Reference {
        if s.is_empty() {
            return Reference::zeroed();
        }
        let mut data = s.as_bytes().to_vec();
        data.push(0);
        let n = len32(data.len());
        let index = self.raw("CHAR", 0, data, n);
        Reference { entries: n, index, flags: 0 }
    }

    pub fn finish(self) -> Vec<u8> {
        let align = |v: &mut Vec<u8>| v.resize(v.len().next_multiple_of(16), 0);
        let mut out = vec![0u8; 16];
        let mut entries = Vec::with_capacity(self.tags.len());
        for t in &self.tags {
            let offset = len32(out.len());
            out.extend_from_slice(&t.data);
            align(&mut out);
            entries.push((t, offset));
        }
        let index_offset = len32(out.len());
        for (t, offset) in &entries {
            out.extend_from_slice(&t.name);
            out.extend_from_slice(&offset.to_le_bytes());
            out.extend_from_slice(&t.reps.to_le_bytes());
            out.extend_from_slice(&t.version.to_le_bytes());
        }
        out[0..4].copy_from_slice(&self.magic);
        out[4..8].copy_from_slice(&index_offset.to_le_bytes());
        out[8..12].copy_from_slice(&len32(entries.len()).to_le_bytes());
        out
    }
}

// ─── Model description ───────────────────────────────────────────────────────

/// One vertex. Only the components `vertex_flags` enables are written.
#[derive(Debug, Clone, Copy, Default)]
pub struct Vtx {
    pub pos:     [f32; 3],
    pub normal:  [u8; 4],
    pub tangent: [u8; 4],
    pub uv0:     [i16; 2],
    pub uv1:     [i16; 2],
    pub weights: [u8; 4],
    pub lookups: [u8; 4],
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RegionSpec {
    pub first_vertex:      u32,
    pub vertex_count:      u32,
    pub first_face:        u32,
    pub face_count:        u32,
    pub first_bone_lookup: u16,
    pub bone_lookup_count: u16,
    pub flags:             u32,
    pub uv_multiply:       f32,
    pub uv_offset:         f32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BatchSpec {
    pub region: u16,
    pub matm:   u16,
    pub bone:   i16,
}

#[derive(Debug, Clone, Default)]
pub struct BoneSpec {
    pub name:     String,
    pub parent:   i16,
    pub t:        [f32; 3],
    pub r:        [f32; 4],
    pub s:        [f32; 3],
    /// Animation ids of the location / rotation / scale tracks (0 = none).
    pub anim_ids: [u32; 3],
    /// `(header.flags, header.id, default)` of the batching toggle.
    pub batching: (u16, u32, u32),
}

impl BoneSpec {
    pub fn named(name: &str, parent: i16) -> Self {
        Self { name: name.into(), parent, r: [0.0, 0.0, 0.0, 1.0], s: [1.0; 3], ..Self::default() }
    }
}

#[derive(Debug, Clone, Default)]
pub struct LayerSpec {
    pub texture:   String,
    /// Flat colour, stored `[b, g, r, a]`; sets the colour-layer flag.
    pub color:     Option<[u8; 4]>,
    pub uv_tiling: [f32; 2],
}

#[derive(Debug, Clone, Default)]
pub struct MatSpec {
    pub blend:           u32,
    pub flags:           u32,
    pub alpha_threshold: u32,
    /// `(layer name, layer)` — `"diff"`, `"norm"`, `"emis1"`, `"ao"`, …
    pub layers:          Vec<(&'static str, LayerSpec)>,
}

#[derive(Debug, Clone)]
pub enum Track {
    Vec3(Vec<(i32, [f32; 3])>),
    Quat(Vec<(i32, [f32; 4])>),
    Real(Vec<(i32, f32)>),
    I16(Vec<(i32, i16)>),
    U16(Vec<(i32, u16)>),
}

#[derive(Debug, Clone, Default)]
pub struct StcSpec {
    pub name:   String,
    pub tracks: Vec<(u32, Track)>,
    /// Write `anim_refs` one entry short — an inconsistent table.
    pub short_refs: bool,
}

#[derive(Debug, Clone, Default)]
pub struct SeqSpec {
    pub name: String,
    /// STC indices in this sequence's STG_.
    pub stcs: Vec<u32>,
}

/// A whole model. Every list may be empty; the defaults are a well-formed,
/// empty MD34 file with the most common tag versions.
#[derive(Debug, Clone)]
pub struct ModelSpec {
    pub magic:        [u8; 4],
    pub vertex_flags: u32,
    pub vertices:     Vec<Vtx>,
    /// Extra bytes appended to the vertex buffer (or negative: truncated).
    pub vertex_slack: i32,
    pub faces:        Vec<u16>,
    pub regn_version: u32,
    pub regions:      Vec<RegionSpec>,
    pub batches:      Vec<BatchSpec>,
    /// Emit DIV_ + U8__ even when there are no vertices.
    pub force_division: bool,
    pub bone_lookup:  Vec<u16>,
    pub bones:        Vec<BoneSpec>,
    pub iref:         bool,
    pub mat_version:  u32,
    pub layr_version: u32,
    pub materials:    Vec<MatSpec>,
    pub madd_version: u32,
    pub madds:        Vec<Vec<String>>,
    /// `blend_mode` of each MADD, index-aligned with `madds`; missing = 0.
    pub madd_blends:  Vec<u32>,
    pub matms:        Vec<(u32, u32)>,
    pub composites:   Vec<Vec<u32>>,
    pub seqs_version: u32,
    pub sequences:    Vec<SeqSpec>,
    pub stcs:         Vec<StcSpec>,
    pub par_version:  u32,
    pub particles:    Vec<Par>,
    pub lite_version: u32,
    pub lights:       Vec<Lite>,
    pub proj_version: u32,
    pub projections:  Vec<Proj>,
    pub att_version:  u32,
    pub attachments:  Vec<(String, u32)>,
    pub atvl_version: u32,
    pub volumes:      Vec<Atvl>,
}

impl Default for ModelSpec {
    fn default() -> Self {
        Self {
            magic:          *b"43DM",
            vertex_flags:   0,
            vertices:       Vec::new(),
            vertex_slack:   0,
            faces:          Vec::new(),
            regn_version:   5,
            regions:        Vec::new(),
            batches:        Vec::new(),
            force_division: false,
            bone_lookup:    Vec::new(),
            bones:          Vec::new(),
            iref:           false,
            mat_version:    19,
            layr_version:   22,
            materials:      Vec::new(),
            madd_version:   3,
            madds:          Vec::new(),
            madd_blends:    Vec::new(),
            matms:          Vec::new(),
            composites:     Vec::new(),
            seqs_version:   2,
            sequences:      Vec::new(),
            stcs:           Vec::new(),
            par_version:    24,
            particles:      Vec::new(),
            lite_version:   7,
            lights:         Vec::new(),
            proj_version:   5,
            projections:    Vec::new(),
            att_version:    1,
            attachments:    Vec::new(),
            atvl_version:   0,
            volumes:        Vec::new(),
        }
    }
}

/// Vertex flags for the common static-mesh layout: position, compressed
/// normal, uv0, uv1, compressed tangent.
pub const FLAGS_STATIC: u32 = 0x0186_0001;
/// [`FLAGS_STATIC`] plus four skin weight/lookup pairs.
pub const FLAGS_SKINNED: u32 = FLAGS_STATIC | 0x20 | 0x40;

impl ModelSpec {
    /// A single opaque triangle-pair quad with one textured MAT_ material.
    pub fn quad() -> Self {
        let v = |x: f32, y: f32, u: i16, w: i16| Vtx {
            pos: [x, y, 0.0],
            normal: [128, 128, 255, 255],
            tangent: [255, 128, 128, 0],
            uv0: [u, w],
            ..Vtx::default()
        };
        Self {
            vertex_flags: FLAGS_STATIC,
            vertices: vec![v(0.0, 0.0, 0, 0), v(1.0, 0.0, 2048, 0), v(1.0, 1.0, 2048, 2048), v(0.0, 1.0, 0, 2048)],
            faces: vec![0, 1, 2, 0, 2, 3],
            regions: vec![RegionSpec { vertex_count: 4, face_count: 6, uv_multiply: 16.0, ..RegionSpec::default() }],
            batches: vec![BatchSpec { region: 0, matm: 0, bone: -1 }],
            materials: vec![MatSpec {
                layers: vec![("diff", LayerSpec { texture: "Assets\\Textures\\quad_diff.dds".into(), ..LayerSpec::default() })],
                ..MatSpec::default()
            }],
            matms: vec![(1, 0)],
            ..Self::default()
        }
    }

    /// Serialize to M3 bytes.
    pub fn build(&self) -> Vec<u8> {
        let mut w = M3Writer::new(self.magic);
        self.write_geometry(&mut w);
        let (bones_ref, bone_lookup) = self.write_skeleton(&mut w);
        self.write_materials(&mut w);
        let [seqs_ref, stc_ref, stg_ref] = self.write_animation(&mut w);
        self.write_effects(&mut w);

        // MODL: only the fields the reader consults.
        let mut modl = vec![0u8; 136];
        let mut put = |at: usize, bytes: &[u8]| modl[at..at + bytes.len()].copy_from_slice(bytes);
        put(16, bytemuck::bytes_of(&seqs_ref));
        put(28, bytemuck::bytes_of(&stc_ref));
        put(40, bytemuck::bytes_of(&stg_ref));
        put(80, bytemuck::bytes_of(&bones_ref));
        put(96, &self.vertex_flags.to_le_bytes());
        put(124, bytemuck::bytes_of(&bone_lookup));
        w.raw("MODL", 23, modl, 1);

        w.finish()
    }

    /// Vertex buffer, faces, regions, batches and the division tying them.
    fn write_geometry(&self, w: &mut M3Writer) {
        let stride = stride_from_flags(self.vertex_flags);
        let offsets = VertexOffsets::from_flags(self.vertex_flags);
        let mut vbuf = vec![0u8; stride * self.vertices.len()];
        for (i, v) in self.vertices.iter().enumerate() {
            let at = |o: usize| i * stride + o;
            vbuf[at(0)..at(12)].copy_from_slice(bytemuck::bytes_of(&v.pos));
            if let Some(o) = offsets.normal {
                vbuf[at(o)..at(o + 4)].copy_from_slice(&v.normal);
            }
            if let Some(o) = offsets.tangent {
                vbuf[at(o)..at(o + 4)].copy_from_slice(&v.tangent);
            }
            if let Some(o) = offsets.uv0 {
                vbuf[at(o)..at(o + 4)].copy_from_slice(bytemuck::bytes_of(&v.uv0));
            }
            if let Some(o) = offsets.uv1 {
                vbuf[at(o)..at(o + 4)].copy_from_slice(bytemuck::bytes_of(&v.uv1));
            }
            if let Some(s) = offsets.skin {
                let (wo, lo) = (s.weights_offset, s.lookups_offset);
                vbuf[at(wo)..at(wo + s.pairs)].copy_from_slice(&v.weights[..s.pairs]);
                vbuf[at(lo)..at(lo + s.pairs)].copy_from_slice(&v.lookups[..s.pairs]);
            }
        }
        match usize::try_from(self.vertex_slack) {
            Ok(extra) => vbuf.resize(vbuf.len() + extra, 0),
            Err(_) => vbuf.truncate(vbuf.len().saturating_sub(self.vertex_slack.unsigned_abs() as usize)),
        }

        let geometry = !self.vertices.is_empty() || self.force_division;
        if geometry {
            let n = len32(vbuf.len());
            w.raw("U8__", 0, vbuf, n);
        }
        let faces = w.pods("U16_", 0, &self.faces);
        let regions: Vec<Vec<u8>> = self.regions.iter().map(|r| self.region_bytes(r)).collect();
        let regions = w.records("REGN", self.regn_version, &regions);
        let batches: Vec<Bat> = self
            .batches
            .iter()
            .map(|b| Bat {
                region_index: b.region,
                material_reference_index: b.matm,
                bone: b.bone,
                ..Bat::zeroed()
            })
            .collect();
        let batches = w.pods("BAT_", 1, &batches);
        if geometry {
            let div = Div { faces, regions, batches, msec: Reference::zeroed(), instances: 0 };
            w.pods("DIV_", 2, &[div]);
        }
    }

    /// Bones, the bone lookup table and (optionally) rest matrices.
    fn write_skeleton(&self, w: &mut M3Writer) -> (Reference, Reference) {
        let bone_lookup = w.pods("U16_", 0, &self.bone_lookup);
        let bones: Vec<Bone> = self
            .bones
            .iter()
            .map(|b| {
                let mut bone = Bone::zeroed();
                bone.name = w.chars(&b.name);
                bone.parent = b.parent;
                bone.location.default = vec3(b.t);
                bone.rotation.default = m3_to_glb::m3::structures::Quat { x: b.r[0], y: b.r[1], z: b.r[2], w: b.r[3] };
                bone.scale.default = vec3(b.s);
                bone.location.header.id = b.anim_ids[0];
                bone.rotation.header.id = b.anim_ids[1];
                bone.scale.header.id = b.anim_ids[2];
                bone.batching.header.flags = b.batching.0;
                bone.batching.header.id = b.batching.1;
                bone.batching.default = b.batching.2;
                bone
            })
            .collect();
        let bones_ref = w.pods("BONE", 1, &bones);
        if self.iref {
            let irefs = vec![0u8; 64 * self.bones.len()];
            w.raw("IREF", 0, irefs, len32(self.bones.len()));
        }
        (bones_ref, bone_lookup)
    }

    /// `MAT_` + `LAYR`, `MADD`, `MATM` and `CMP_`.
    fn write_materials(&self, w: &mut M3Writer) {
        let mats: Vec<Vec<u8>> = self.materials.iter().map(|m| self.mat_bytes(w, m)).collect();
        w.records("MAT_", self.mat_version, &mats);
        let madds: Vec<Vec<u8>> = self
            .madds
            .iter()
            .enumerate()
            .map(|(i, paths)| {
                let schrs: Vec<Schr> = paths.iter().map(|p| Schr { path: w.chars(p) }).collect();
                let r = w.pods("SCHR", 0, &schrs);
                let (size, at) = match self.madd_version {
                    1 => (140, 36),
                    2 => (152, 48),
                    _ => (160, 48),
                };
                let mut rec = vec![0u8; size];
                rec[at..at + 12].copy_from_slice(bytemuck::bytes_of(&r));
                let blend = self.madd_blends.get(i).copied().unwrap_or(0);
                rec[at + 76..at + 80].copy_from_slice(&blend.to_le_bytes());
                rec
            })
            .collect();
        w.records("MADD", self.madd_version, &madds);
        let matms: Vec<Matm> = self.matms.iter().map(|&(t, i)| Matm { mat_type: t, material_index: i }).collect();
        w.pods("MATM", 0, &matms);
        let cmps: Vec<Vec<u8>> = self
            .composites
            .iter()
            .map(|sections| {
                let cms: Vec<Cms> = sections
                    .iter()
                    .map(|&i| Cms { material_reference_index: i, ..Cms::zeroed() })
                    .collect();
                let r = w.pods("CMS_", 0, &cms);
                let mut rec = vec![0u8; 28];
                rec[16..28].copy_from_slice(bytemuck::bytes_of(&r));
                rec
            })
            .collect();
        w.records("CMP_", 2, &cmps);
    }

    /// `STC_`, `SEQS` and `STG_`; returns their references for MODL.
    fn write_animation(&self, w: &mut M3Writer) -> [Reference; 3] {
        let stcs: Vec<Stc> = self.stcs.iter().map(|s| stc(w, s)).collect();
        let stc_ref = w.pods("STC_", 4, &stcs);
        let (seqs, stgs): (Vec<Vec<u8>>, Vec<Stg>) = self
            .sequences
            .iter()
            .map(|s| {
                let mut seq = Seqs::zeroed();
                seq.name = w.chars(&s.name);
                let rec = if self.seqs_version == 1 {
                    // v1 has an extra u32 at +52.
                    let v2 = bytemuck::bytes_of(&seq);
                    let mut v1 = vec![0u8; std::mem::size_of::<SeqsV1>()];
                    v1[..52].copy_from_slice(&v2[..52]);
                    v1[56..].copy_from_slice(&v2[52..]);
                    v1
                } else {
                    bytemuck::bytes_of(&seq).to_vec()
                };
                let stg = Stg { name: seq.name, stc_indices: w.pods("U32_", 0, &s.stcs) };
                (rec, stg)
            })
            .unzip();
        let seqs_ref = w.records("SEQS", self.seqs_version, &seqs);
        let stg_ref = w.pods("STG_", 0, &stgs);
        [seqs_ref, stc_ref, stg_ref]
    }

    /// Particles, lights, projections, attachment points and volumes.
    fn write_effects(&self, w: &mut M3Writer) {
        let pars: Vec<Vec<u8>> = self.particles.iter().map(|p| par_bytes(p, self.par_version)).collect();
        w.records("PAR_", self.par_version, &pars);
        w.pods("LITE", self.lite_version, &self.lights);
        w.pods("PROJ", self.proj_version, &self.projections);
        let atts: Vec<m3_to_glb::m3::structures::Att> = self
            .attachments
            .iter()
            .map(|(name, bone)| m3_to_glb::m3::structures::Att {
                unknown00: -1,
                name: w.chars(name),
                bone: *bone,
            })
            .collect();
        w.pods("ATT_", self.att_version, &atts);
        w.pods("ATVL", self.atvl_version, &self.volumes);
    }

    fn region_bytes(&self, r: &RegionSpec) -> Vec<u8> {
        let full = Regn {
            first_vertex_index: r.first_vertex,
            vertex_count: r.vertex_count,
            first_face_index: r.first_face,
            face_count: r.face_count,
            first_bone_lookup_idx: r.first_bone_lookup,
            bone_lookup_count: r.bone_lookup_count,
            flags: r.flags,
            uv_multiply: r.uv_multiply,
            uv_offset: r.uv_offset,
            ..Regn::zeroed()
        };
        let b = bytemuck::bytes_of(&full);
        match self.regn_version {
            0..=2 => {
                // id | u16 first_vertex | u16 vertex_count | the rest of V5 from
                // first_face up to (not including) flags.
                let mut v = Vec::with_capacity(28);
                v.extend_from_slice(&b[0..4]);
                // v≤2 stores these as u16; larger fixture values saturate.
                let short = |n: u32| u16::try_from(n).unwrap_or(u16::MAX);
                v.extend_from_slice(&short(r.first_vertex).to_le_bytes());
                v.extend_from_slice(&short(r.vertex_count).to_le_bytes());
                v.extend_from_slice(&b[16..36]);
                v
            }
            3 => b[..36].to_vec(),
            4 => b[..40].to_vec(),
            _ => b.to_vec(),
        }
    }

    fn mat_bytes(&self, w: &mut M3Writer, mat: &MatSpec) -> Vec<u8> {
        let mut rec = vec![0u8; mat_record_size(self.mat_version)];
        rec[16..20].copy_from_slice(&mat.flags.to_le_bytes());
        rec[20..24].copy_from_slice(&mat.blend.to_le_bytes());
        rec[40..44].copy_from_slice(&mat.alpha_threshold.to_le_bytes());
        for (name, layer) in &mat.layers {
            let Some(at) = M3File::mat_layer_offset(self.mat_version, name) else { continue };
            let mut l = vec![0u8; layr_record_size(self.layr_version)];
            let bitmap = w.chars(&layer.texture);
            l[4..16].copy_from_slice(bytemuck::bytes_of(&bitmap));
            if let Some(c) = layer.color {
                l[24..28].copy_from_slice(&c);
                l[36..40].copy_from_slice(&0x400u32.to_le_bytes());
            }
            let t = layr_uv_tiling_offset(self.layr_version) + 8;
            l[t..t + 8].copy_from_slice(bytemuck::bytes_of(&layer.uv_tiling));
            let r = w.records("LAYR", self.layr_version, &[l]);
            rec[at..at + 12].copy_from_slice(bytemuck::bytes_of(&r));
        }
        rec
    }
}

fn vec3(v: [f32; 3]) -> m3_to_glb::m3::structures::Vec3 {
    m3_to_glb::m3::structures::Vec3 { x: v[0], y: v[1], z: v[2] }
}

/// SD block: `frames` (I32_) + `keys` (typed), 32 bytes.
fn sd_block<K: Pod>(w: &mut M3Writer, key_tag: &str, keys: &[(i32, K)]) -> [Reference; 2] {
    let frames: Vec<i32> = keys.iter().map(|k| k.0).collect();
    let values: Vec<K> = keys.iter().map(|k| k.1).collect();
    [w.pods("I32_", 0, &frames), w.pods(key_tag, 0, &values)]
}

fn stc(w: &mut M3Writer, spec: &StcSpec) -> Stc {
    let mut out = Stc::zeroed();
    out.name = w.chars(&spec.name);
    let mut ids = Vec::new();
    let mut refs = Vec::new();
    let (mut vec3s, mut quats, mut reals, mut i16s, mut u16s) = (vec![], vec![], vec![], vec![], vec![]);
    for (id, track) in &spec.tracks {
        let (kind, blocks, block) = match track {
            Track::Vec3(k) => (2u32, &mut vec3s, sd_block(w, "VEC3", k)),
            Track::Quat(k) => {
                let k: Vec<(i32, m3_to_glb::m3::structures::Quat)> = k
                    .iter()
                    .map(|&(t, v)| (t, m3_to_glb::m3::structures::Quat { x: v[0], y: v[1], z: v[2], w: v[3] }))
                    .collect();
                (3, &mut quats, sd_block(w, "QUAT", &k))
            }
            Track::Real(k) => (5, &mut reals, sd_block(w, "REAL", k)),
            Track::I16(k) => (7, &mut i16s, sd_block(w, "I16_", k)),
            Track::U16(k) => (8, &mut u16s, sd_block(w, "U16_", k)),
        };
        ids.push(*id);
        refs.push((kind << 16) | len32(blocks.len()));
        let mut record = [0u8; 32];
        record[0..12].copy_from_slice(bytemuck::bytes_of(&block[0]));
        record[20..32].copy_from_slice(bytemuck::bytes_of(&block[1]));
        blocks.push(record);
    }
    if spec.short_refs {
        refs.pop();
    }
    out.anim_ids = w.pods("U32_", 0, &ids);
    out.anim_refs = w.pods("U32_", 0, &refs);
    out.sd3v = w.pods("SD3V", 0, &vec3s);
    out.sd4q = w.pods("SD4Q", 0, &quats);
    out.sdr3 = w.pods("SDR3", 0, &reals);
    out.sds6 = w.pods("SDS6", 0, &i16s);
    out.sdu6 = w.pods("SDU6", 0, &u16s);
    out
}

/// Serialize a v24 [`Par`] as `version` (22/23/24; anything else is written
/// as v24 bytes under the unsupported version number).
fn par_bytes(p: &Par, version: u32) -> Vec<u8> {
    let full = bytemuck::bytes_of(p);
    let splice = std::mem::offset_of!(Par, world_forces_mass_mult);
    match version {
        23 => [&full[..splice], &full[splice + 4..full.len() - 4]].concat(),
        22 => [&full[..splice], &full[splice + 4..full.len() - 8]].concat(),
        _ => full.to_vec(),
    }
}


// ─── Shared fixtures ─────────────────────────────────────────────────────────

/// [`ModelSpec::quad`] skinned to a two-bone chain.
pub fn skinned_quad() -> ModelSpec {
    let mut spec = ModelSpec::quad();
    spec.vertex_flags = FLAGS_SKINNED;
    for (i, v) in spec.vertices.iter_mut().enumerate() {
        v.weights = [200, 55, 0, 0];
        v.lookups = [u8::from(i % 2 == 1), 1, 0, 0];
    }
    spec.bones = vec![
        BoneSpec { t: [0.0, 0.0, 1.0], ..BoneSpec::named("Root", -1) },
        BoneSpec { t: [0.0, 1.0, 0.0], ..BoneSpec::named("Child", 0) },
    ];
    spec.bone_lookup = vec![0, 1];
    spec.regions[0].bone_lookup_count = 2;
    spec.iref = true;
    spec
}

/// [`skinned_quad`] with a clip that exercises every animation path: a
/// duplicate and a backwards key, a wrong-kind track, and STCs that must be
/// skipped (unnamed, inconsistent tables, no matching bone, empty).
pub fn animated() -> ModelSpec {
    let mut spec = skinned_quad();
    spec.bones[0].anim_ids = [11, 12, 13];
    spec.bones[1].anim_ids = [21, 22, 0];
    spec.stcs = vec![
        StcSpec {
            name: "Stand_full".into(),
            tracks: vec![
                (11, Track::Vec3(vec![(0, [0.0, 0.0, 0.0]), (500, [0.0, 0.0, 1.0]), (500, [0.0, 0.0, 2.0]), (1000, [0.0; 3])])),
                (12, Track::Quat(vec![(0, [0.0, 0.0, 0.0, 1.0]), (1000, [0.0, 0.0, 0.0, -1.0])])),
                (13, Track::Vec3(vec![(0, [1.0; 3])])),
                (21, Track::Vec3(vec![(0, [0.0; 3]), (-5, [1.0; 3]), (200, [2.0; 3])])),
                // Wrong kind for a rotation id: ignored.
                (22, Track::Real(vec![(0, 1.0)])),
            ],
            ..StcSpec::default()
        },
        StcSpec { name: "Loose".into(), tracks: vec![(21, Track::Vec3(vec![(0, [0.0; 3])]))], ..StcSpec::default() },
        // No name → skipped; mismatched tables → skipped; no matching bone → skipped.
        StcSpec { name: String::new(), tracks: vec![(11, Track::Vec3(vec![(0, [0.0; 3])]))], ..StcSpec::default() },
        StcSpec {
            name: "Short".into(),
            tracks: vec![(11, Track::Vec3(vec![(0, [0.0; 3])])), (12, Track::Vec3(vec![(0, [0.0; 3])]))],
            short_refs: true,
        },
        StcSpec { name: "Nobody".into(), tracks: vec![(999, Track::Vec3(vec![(0, [0.0; 3])]))], ..StcSpec::default() },
        StcSpec { name: "Empty".into(), tracks: vec![], ..StcSpec::default() },
    ];
    spec.sequences = vec![SeqSpec { name: "Stand".into(), stcs: vec![0, 0, 42] }];
    spec
}

/// A skeleton-only model carrying one of each effect kind, with animated
/// emitter tracks and materials reached directly, via a composite and via MADD.
pub fn with_effects() -> ModelSpec {
    let mut spec = ModelSpec { bones: vec![BoneSpec::named("Bone_FX", -1)], ..ModelSpec::default() };
    let mut par = Par::zeroed();
    par.emit_rate.default = 10.0;
    par.emit_rate.header.id = 77;
    par.emit_count.header.id = 78;
    par.size.header.id = 79;
    par.material_reference_index = 0;
    spec.particles = vec![par, Par { bone: 9, ..par }];
    let mut lite = Lite::zeroed();
    lite.shape = 2;
    lite.intensity.header.id = 80;
    lite.attenuation_near.default = 0.5;
    spec.lights = vec![lite, Lite { bone: 9, ..lite }];
    let mut proj = Proj::zeroed();
    proj.offset.default.z = 2.0;
    proj.material_reference_index = 1;
    proj.pitch.default = 0.5;
    spec.projections = vec![proj, Proj { bone: 9, ..proj }];
    spec.materials = vec![
        MatSpec { blend: 2, layers: vec![("emis1", LayerSpec { color: Some([0, 0, 255, 255]), ..LayerSpec::default() })], ..MatSpec::default() },
    ];
    spec.madds = vec![vec!["fx_diff.dds".into()]];
    spec.madd_blends = vec![1];
    spec.matms = vec![(1, 0), (3, 0), (12, 0), (5, 0)];
    spec.composites = vec![vec![2]];
    spec.stcs = vec![StcSpec {
        name: "Birth".into(),
        tracks: vec![
            (77, Track::Real((0..100).map(|i| (i * 10, i as f32)).collect())),
            (78, Track::I16(vec![(0, 5), (100, 0)])),
            (79, Track::Vec3(vec![(0, [1.0; 3]), (40, [2.0; 3])])),
            (80, Track::Real(vec![(0, 0.0), (50, 3.0)])),
            (81, Track::U16(vec![(0, 1)])),
        ],
        ..StcSpec::default()
    }];
    spec
}

// ─── Mutation ────────────────────────────────────────────────────────────────

/// XOR `mask` into the byte at `pos % len` for each pair — corrupts a built
/// file in place to drive the parser's error paths.
pub fn corrupt(bytes: &mut [u8], edits: &[(u32, u8)]) {
    if bytes.is_empty() {
        return;
    }
    let len = bytes.len();
    for &(pos, mask) in edits {
        bytes[pos as usize % len] ^= mask;
    }
}

// ─── GLB oracle ──────────────────────────────────────────────────────────────

/// What a valid GLB turned out to contain.
#[derive(Debug)]
pub struct GlbSummary {
    pub json: serde_json::Value,
    pub bin:  Vec<u8>,
}

impl GlbSummary {
    pub fn count(&self, key: &str) -> usize {
        self.json.get(key).and_then(|v| v.as_array()).map_or(0, Vec::len)
    }

    /// Nodes whose `extras` carry `key` (`"m3fx"` / `"m3attach"`).
    pub fn extras(&self, key: &str) -> Vec<&serde_json::Value> {
        self.json["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|n| n.get("extras")?.get(key))
            .collect()
    }

    pub fn node_named(&self, name: &str) -> Option<&serde_json::Value> {
        self.json["nodes"].as_array()?.iter().find(|n| n["name"] == name)
    }
}

fn u32_at(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
        .ok_or_else(|| format!("truncated at {at}"))
}

/// Check that `glb` is a well-formed glTF binary: header and chunk framing,
/// JSON that parses, acceptance by the independent `gltf` crate loader, and
/// the cross-references it does not check — buffer ranges, index ranges,
/// animation timing, and an acyclic node forest.
pub fn check_glb(glb: &[u8]) -> Result<GlbSummary, String> {
    let (json, bin) = split_chunks(glb)?;
    // The `gltf` crate does not implement KHR_texture_basisu and rejects any
    // file that requires it; for those, load without its validation pass and
    // rely on the checks below.
    let basisu = json.get("extensionsUsed").is_some_and(|e| e.to_string().contains("KHR_texture_basisu"));
    let doc = if basisu {
        gltf::Gltf::from_slice_without_validation(glb)
    } else {
        gltf::Gltf::from_slice(glb)
    }
    .map_err(|e| format!("gltf crate rejected it: {e}"))?;

    check_ranges(&doc, &bin)?;
    check_indices(&doc, &bin)?;
    check_animations(&doc, &bin)?;
    check_hierarchy(&doc)?;
    Ok(GlbSummary { json, bin })
}

/// Header and chunk framing; returns the parsed JSON and the BIN payload.
fn split_chunks(glb: &[u8]) -> Result<(serde_json::Value, Vec<u8>), String> {
    let len = |at| u32_at(glb, at).map(|n| n as usize);
    if u32_at(glb, 0)? != 0x4654_6C67 || u32_at(glb, 4)? != 2 {
        return Err("bad GLB header".into());
    }
    if len(8)? != glb.len() {
        return Err("header length != file length".into());
    }
    let json_len = len(12)?;
    if u32_at(glb, 16)? != 0x4E4F_534A || !json_len.is_multiple_of(4) {
        return Err("bad JSON chunk".into());
    }
    let json_bytes = glb.get(20..20 + json_len).ok_or("JSON chunk truncated")?;
    let json = serde_json::from_slice(json_bytes).map_err(|e| format!("JSON: {e}"))?;
    let rest = &glb[20 + json_len..];
    if rest.is_empty() {
        return Ok((json, Vec::new()));
    }
    let bin_len = u32_at(rest, 0)? as usize;
    if u32_at(rest, 4)? != 0x004E_4942 || !bin_len.is_multiple_of(4) || rest.len() != 8 + bin_len {
        return Err("bad BIN chunk".into());
    }
    Ok((json, rest[8..].to_vec()))
}

/// Buffer views inside the buffer, accessors inside their views.
fn check_ranges(doc: &gltf::Gltf, bin: &[u8]) -> Result<(), String> {
    let declared = doc.buffers().next().map_or(0, |b| b.length());
    if declared > bin.len() {
        return Err(format!("buffer declares {declared} bytes, BIN has {}", bin.len()));
    }
    if let Some(v) = doc.views().find(|v| v.offset() + v.length() > declared) {
        return Err(format!("bufferView {} out of range", v.index()));
    }
    for a in doc.accessors() {
        if a.count() == 0 {
            return Err(format!("accessor {} has count 0", a.index()));
        }
        let view = a.view().ok_or("sparse accessors are not emitted")?;
        if a.offset() + a.count() * a.size() > view.length() {
            return Err(format!("accessor {} overruns its view", a.index()));
        }
    }
    Ok(())
}

/// The bytes an accessor covers.
fn accessor_bytes<'b>(a: &gltf::Accessor<'_>, bin: &'b [u8]) -> &'b [u8] {
    let view = a.view().expect("checked by check_ranges");
    let start = view.offset() + a.offset();
    &bin[start..start + a.count() * a.size()]
}

/// Every primitive's attributes agree on the vertex count, and every index
/// addresses one of those vertices.
fn check_indices(doc: &gltf::Gltf, bin: &[u8]) -> Result<(), String> {
    for p in doc.meshes().flat_map(|m| m.primitives()) {
        let verts = p.get(&gltf::Semantic::Positions).ok_or("primitive without POSITION")?.count();
        if p.attributes().any(|(_, a)| a.count() != verts) {
            return Err("attribute counts differ within a primitive".into());
        }
        let Some(idx) = p.indices() else { continue };
        if idx.count() % 3 != 0 {
            return Err("index count is not a multiple of 3".into());
        }
        let ids: Vec<u32> = bytemuck::pod_collect_to_vec(accessor_bytes(&idx, bin));
        if let Some(bad) = ids.iter().find(|&&i| i as usize >= verts) {
            return Err(format!("index {bad} >= vertex count {verts}"));
        }
    }
    Ok(())
}

/// Sampler input is finite and strictly increasing, with one output per input.
fn check_animations(doc: &gltf::Gltf, bin: &[u8]) -> Result<(), String> {
    for anim in doc.animations() {
        for sampler in anim.samplers() {
            let times: Vec<f32> = bytemuck::pod_collect_to_vec(accessor_bytes(&sampler.input(), bin));
            if times.iter().any(|t| !t.is_finite()) || times.windows(2).any(|w| w[0] >= w[1]) {
                return Err(format!("animation '{}' has non-increasing input", anim.name().unwrap_or("")));
            }
            if sampler.output().count() != sampler.input().count() {
                return Err("sampler input/output count mismatch".into());
            }
        }
    }
    Ok(())
}

/// Nodes form a forest: every node has at most one parent, no cycles, scene
/// roots are parentless, and each skin has one IBM per joint.
fn check_hierarchy(doc: &gltf::Gltf) -> Result<(), String> {
    let n = doc.nodes().count();
    let mut parent = vec![None; n];
    for node in doc.nodes() {
        for c in node.children() {
            if parent[c.index()].replace(node.index()).is_some() {
                return Err(format!("node {} has two parents", c.index()));
            }
        }
    }
    for start in 0..n {
        let mut at = start;
        for _ in 0..=n {
            match parent[at] {
                Some(p) => at = p,
                None => break,
            }
        }
        if parent[at].is_some() {
            return Err(format!("cycle through node {start}"));
        }
    }
    if let Some(root) = doc.scenes().flat_map(|s| s.nodes()).find(|r| parent[r.index()].is_some()) {
        return Err(format!("scene root {} has a parent", root.index()));
    }
    for skin in doc.skins() {
        if skin.inverse_bind_matrices().is_some_and(|ibm| ibm.count() != skin.joints().count()) {
            return Err("IBM count != joint count".into());
        }
    }
    Ok(())
}
