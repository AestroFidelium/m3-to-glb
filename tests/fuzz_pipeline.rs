//! Structure-aware fuzzing of the whole conversion pipeline.
//!
//! Random bytes almost never get past the M3 tag table, so a raw-bytes target
//! (see `fuzz_parse.rs`) only exercises the first few hundred lines of the
//! parser. Here the fuzzer's input is decoded into a [`FuzzModel`] — vertices,
//! regions, bones, materials, animation tracks, effects, each field arbitrary —
//! which is serialized into a *structurally valid* M3 file and then optionally
//! corrupted byte by byte. That reaches every stage: geometry decode, material
//! resolution, skinning, animation sampling, effect extras, GLB packing.
//!
//! The property is stronger than "does not panic": whenever conversion
//! succeeds, the output must pass [`common::check_glb`] — load in the `gltf`
//! crate, keep every index and accessor in range, keep animation time strictly
//! increasing, and form an acyclic node forest.
//!
//! ```text
//! cargo bolero test --profile fuzz fuzz_whole_pipeline -T 10min   # libFuzzer
//! cargo test --test fuzz_pipeline                                # corpus + random replay
//! ```

mod common;

use arbitrary::Arbitrary;
use bolero::check;
use bytemuck::Zeroable;
use common::*;
use m3_to_glb::m3::structures::{Atvl, Lite, Par, Proj};
use m3_to_glb::{Converter, TextureCache};

/// Names drawn by index, so the fuzzer can hit interesting strings (empty,
/// quotes, control characters, non-ASCII) without having to invent them.
const NAMES: [&str; 8] = ["", "Root", "Ref_Head", "Stand_full", "a\"b\\c", "\u{1}\u{1f}", "Кость", "x_diff.dds"];
const LAYERS: [&str; 6] = ["diff", "norm", "emis1", "ao", "spec", "nonexistent"];

fn name(i: u8) -> String {
    NAMES[i as usize % NAMES.len()].to_owned()
}

fn pick<T: Copy>(table: &[T], i: u8) -> T {
    table[i as usize % table.len()]
}

/// Fill a `Pod` record from fuzzer bytes, zero-padding what is missing.
fn pod_from<T: bytemuck::Pod>(bytes: &[u8]) -> T {
    let mut v = vec![0u8; std::mem::size_of::<T>()];
    let n = bytes.len().min(v.len());
    v[..n].copy_from_slice(&bytes[..n]);
    bytemuck::pod_read_unaligned(&v)
}

#[derive(Debug, Arbitrary)]
struct FuzzVtx {
    pos:     [f32; 3],
    normal:  [u8; 4],
    tangent: [u8; 4],
    uv0:     [i16; 2],
    weights: [u8; 4],
    lookups: [u8; 4],
}

#[derive(Debug, Arbitrary)]
struct FuzzRegion {
    first_vertex: u8,
    vertex_count: u8,
    first_face:   u8,
    face_count:   u8,
    lookup_first: u8,
    lookup_count: u8,
    flags:        u8,
    uv_multiply:  f32,
    uv_offset:    f32,
}

#[derive(Debug, Arbitrary)]
struct FuzzBone {
    name:     u8,
    parent:   i8,
    t:        [f32; 3],
    r:        [f32; 4],
    s:        [f32; 3],
    anim_ids: [u8; 3],
    batching: (u8, u8, u8),
}

#[derive(Debug, Arbitrary)]
struct FuzzLayer {
    which:  u8,
    texture: u8,
    color:  Option<[u8; 4]>,
    tiling: [f32; 2],
}

#[derive(Debug, Arbitrary)]
struct FuzzMaterial {
    blend:  u8,
    flags:  u8,
    alpha:  u8,
    layers: Vec<FuzzLayer>,
}

#[derive(Debug, Arbitrary)]
enum FuzzTrack {
    Vec3(Vec<(i16, [f32; 3])>),
    Quat(Vec<(i16, [f32; 4])>),
    Real(Vec<(i16, f32)>),
    I16(Vec<(i16, i16)>),
    U16(Vec<(i16, u16)>),
}

impl FuzzTrack {
    fn track(&self) -> Track {
        fn keys<V: Copy>(k: &[(i16, V)]) -> Vec<(i32, V)> {
            k.iter().take(12).map(|&(t, v)| (i32::from(t), v)).collect()
        }
        match self {
            Self::Vec3(k) => Track::Vec3(keys(k)),
            Self::Quat(k) => Track::Quat(keys(k)),
            Self::Real(k) => Track::Real(keys(k)),
            Self::I16(k) => Track::I16(keys(k)),
            Self::U16(k) => Track::U16(keys(k)),
        }
    }
}

#[derive(Debug, Arbitrary)]
struct FuzzStc {
    name:   u8,
    tracks: Vec<(u8, FuzzTrack)>,
    short:  bool,
}

#[derive(Debug, Arbitrary)]
struct FuzzAnims {
    stcs:      Vec<FuzzStc>,
    sequences: Vec<(u8, Vec<u8>)>,
}

impl FuzzAnims {
    fn apply(&self, spec: &mut ModelSpec) {
        spec.stcs = self
            .stcs
            .iter()
            .take(4)
            .map(|s| StcSpec {
                name:       name(s.name),
                tracks:     s.tracks.iter().take(6).map(|(id, t)| (u32::from(*id % 16), t.track())).collect(),
                short_refs: s.short,
            })
            .collect();
        spec.sequences = self
            .sequences
            .iter()
            .take(3)
            .map(|(n, stcs)| SeqSpec { name: name(*n), stcs: stcs.iter().take(4).map(|&i| u32::from(i % 6)).collect() })
            .collect();
    }
}

#[derive(Debug, Arbitrary)]
struct FuzzModel {
    magic:        u8,
    vertex_flags: u32,
    vertices:     Vec<FuzzVtx>,
    vertex_slack: i8,
    faces:        Vec<u16>,
    regn_version: u8,
    regions:      Vec<FuzzRegion>,
    batches:      Vec<(u8, u8, i8)>,
    force_div:    bool,
    bone_lookup:  Vec<u8>,
    bones:        Vec<FuzzBone>,
    iref:         bool,
    versions:     [u8; 10],
    materials:    Vec<FuzzMaterial>,
    madds:        Vec<Vec<u8>>,
    matms:        Vec<(u8, u8)>,
    composites:   Vec<Vec<u8>>,
    anims:        FuzzAnims,
    particles:    Vec<(u8, u8, Vec<u8>)>,
    lights:       Vec<(u8, Vec<u8>)>,
    projections:  Vec<(u8, u8, Vec<u8>)>,
    attachments:  Vec<(u8, u8)>,
    volumes:      Vec<(u8, Vec<u8>)>,
    /// A companion `.m3a` file, when present.
    m3a:          Option<FuzzAnims>,
    fx:           bool,
    /// Byte flips applied to the serialized model.
    corruption:   Vec<(u32, u8)>,
}

impl FuzzModel {
    fn spec(&self) -> ModelSpec {
        let v = self.versions;
        let mut spec = ModelSpec {
            magic: pick(&[*b"43DM", *b"33DM", *b"23DM", *b"MD34"], self.magic),
            // Keep the flags mostly in the known range so the layout is one a
            // real file could have, but let the unknown high bits through too.
            vertex_flags: self.vertex_flags & 0x7FFF_FFFF,
            vertex_slack: i32::from(self.vertex_slack),
            regn_version: u32::from(self.regn_version % 7),
            force_division: self.force_div,
            iref: self.iref,
            mat_version: pick(&[15, 16, 17, 18, 19, 20, 21], v[0]),
            layr_version: pick(&[20, 22, 23, 24, 25, 26, 30], v[1]),
            madd_version: pick(&[1, 2, 3, 4], v[2]),
            seqs_version: pick(&[1, 2], v[3]),
            par_version: pick(&[21, 22, 23, 24], v[4]),
            lite_version: pick(&[6, 7], v[5]),
            proj_version: pick(&[4, 5], v[6]),
            att_version: pick(&[0, 1], v[7]),
            atvl_version: pick(&[0, 1], v[8]),
            ..ModelSpec::default()
        };
        spec.vertices = self
            .vertices
            .iter()
            .take(48)
            .map(|x| Vtx {
                pos: x.pos,
                normal: x.normal,
                tangent: x.tangent,
                uv0: x.uv0,
                uv1: x.uv0,
                weights: x.weights,
                lookups: x.lookups,
            })
            .collect();
        spec.faces = self.faces.iter().take(144).map(|f| f % 64).collect();
        spec.regions = self
            .regions
            .iter()
            .take(4)
            .map(|r| RegionSpec {
                first_vertex:      u32::from(r.first_vertex),
                vertex_count:      u32::from(r.vertex_count),
                first_face:        u32::from(r.first_face),
                face_count:        u32::from(r.face_count),
                first_bone_lookup: u16::from(r.lookup_first),
                bone_lookup_count: u16::from(r.lookup_count),
                flags:             u32::from(r.flags),
                uv_multiply:       r.uv_multiply,
                uv_offset:         r.uv_offset,
            })
            .collect();
        spec.batches = self
            .batches
            .iter()
            .take(6)
            .map(|&(region, matm, bone)| BatchSpec { region: u16::from(region % 6), matm: u16::from(matm % 8), bone: i16::from(bone) })
            .collect();
        spec.bone_lookup = self.bone_lookup.iter().take(16).map(|&b| u16::from(b % 20)).collect();
        spec.bones = self
            .bones
            .iter()
            .take(12)
            .map(|b| BoneSpec {
                name:     name(b.name),
                parent:   i16::from(b.parent),
                t:        b.t,
                r:        b.r,
                s:        b.s,
                anim_ids: b.anim_ids.map(|i| u32::from(i % 16)),
                batching: (u16::from(b.batching.0), u32::from(b.batching.1), u32::from(b.batching.2 % 2)),
            })
            .collect();
        spec.materials = self
            .materials
            .iter()
            .take(4)
            .map(|m| MatSpec {
                blend:           u32::from(m.blend % 8),
                flags:           u32::from(m.flags),
                alpha_threshold: u32::from(m.alpha),
                layers:          m
                    .layers
                    .iter()
                    .take(4)
                    .map(|l| {
                        let layer = LayerSpec { texture: name(l.texture), color: l.color, uv_tiling: l.tiling };
                        (pick(&LAYERS, l.which), layer)
                    })
                    .collect(),
            })
            .collect();
        spec.madds = self.madds.iter().take(3).map(|p| p.iter().take(4).map(|&n| name(n)).collect()).collect();
        spec.matms = self
            .matms
            .iter()
            .take(8)
            .map(|&(t, i)| (pick(&[1, 1, 3, 12, 2, 0], t), u32::from(i % 6)))
            .collect();
        spec.composites = self.composites.iter().take(3).map(|s| s.iter().take(3).map(|&i| u32::from(i % 9)).collect()).collect();
        self.anims.apply(&mut spec);

        let bone = |b: u8| b % 14; // mostly in range, sometimes past the end
        spec.particles = self
            .particles
            .iter()
            .take(3)
            .map(|(b, mat, raw)| Par { bone: u32::from(bone(*b)), material_reference_index: u32::from(*mat % 8), ..pod_from(raw) })
            .collect();
        spec.lights = self
            .lights
            .iter()
            .take(3)
            .map(|(b, raw)| Lite { bone: u16::from(bone(*b)), ..pod_from(raw) })
            .collect();
        spec.projections = self
            .projections
            .iter()
            .take(3)
            .map(|(b, mat, raw)| Proj { bone: u32::from(bone(*b)), material_reference_index: u32::from(*mat % 8), ..pod_from(raw) })
            .collect();
        spec.attachments = self.attachments.iter().take(4).map(|&(n, b)| (name(n), u32::from(bone(b)))).collect();
        spec.volumes = self
            .volumes
            .iter()
            .take(3)
            .map(|(b, raw)| Atvl { bone0: u32::from(bone(*b)), ..pod_from::<Atvl>(raw) })
            .collect();
        spec
    }

    fn m3a(&self) -> Option<Vec<u8>> {
        let anims = self.m3a.as_ref()?;
        let mut spec = ModelSpec::default();
        anims.apply(&mut spec);
        Some(spec.build())
    }
}

/// Convert and check. Errors are fine; panics and invalid output are not.
fn run(model: &FuzzModel) {
    let mut bytes = model.spec().build();
    corrupt(&mut bytes, &model.corruption[..model.corruption.len().min(8)]);
    let m3a = model.m3a();
    let mut c = Converter::new().effects(model.fx);
    if let Some(a) = &m3a {
        c = c.animations(a);
    }
    if let Ok(glb) = c.convert(&bytes)
        && let Err(e) = check_glb(&glb.bytes) {
            panic!("converter produced an invalid GLB: {e}\n{model:#?}");
        }
}

#[test]
fn fuzz_whole_pipeline() {
    check!().with_arbitrary::<FuzzModel>().for_each(run);
}

/// Texture lookups take M3 paths straight from the file.
#[test]
fn fuzz_texture_lookup() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("unit_diff.png"), b"x").unwrap();
    let cache = TextureCache::build(dir.path().to_str().unwrap()).unwrap();
    check!().with_type::<String>().for_each(|path| {
        let _ = cache.find(path);
        if let Some((p, mime)) = cache.find_with_mime(path) {
            assert!(p.ends_with("unit_diff.png") && mime == "image/png");
        }
    });
}

/// A default `FuzzModel` must convert — the fuzzer's empty input is not an
/// error path.
#[test]
fn smallest_input_converts() {
    let empty = FuzzModel::arbitrary(&mut arbitrary::Unstructured::new(&[])).unwrap();
    run(&empty);
    let glb = Converter::new().convert(&empty.spec().build());
    assert!(glb.is_ok(), "{glb:?}");
    let _ = Par::zeroed();
}
