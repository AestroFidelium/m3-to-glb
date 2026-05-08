//! Zero-copy M3 reader.
//!
//! ## What an M3 actually looks like (from a tag dump)
//!
//!   tags[21] "__8U"  count=1332640  — vertex buffer (count = bytes)
//!   tags[22] "_VID"  count=1        — DIV_  (1 Division)
//!   tags[23] "_61U"  count=113646   — u16 indices (count = elements)
//!   tags[25] "NGER"  count=3        — REGN  (3 regions)
//!   tags[18] "ENOB"  count=2        — BONE
//!
//! All tag names are ASCII inside a little-endian u32 (so the bytes are
//! reversed):
//!   "DIV_" → b"_VID",  "REGN" → b"NGER",  "BONE" → b"ENOB"
//!   "U8__" → b"__8U",  "U16_" → b"_61U"
//!
//! We DO NOT use `ModelHeader` — its layout doesn't match the actual file.
//! Instead we look up tags directly by their LE name.

use super::structures::{
    Bat, Bone, Div, Iref, Layr, MdIndexEntry, Quat, Reference, Regn, Schr, Sd3v, Sd4q, Seqs,
    SeqsV1, Stc, Stg, Vec3,
};
use super::{M3Version, detect_version};
use anyhow::{Result, ensure};
use bytemuck::cast_slice;
use simdutf8::basic::from_utf8;
use tracing::debug;

// ─── LE tag names ────────────────────────────────────────────────────────────
const TAG_DIV: &[u8; 4] = b"_VID"; // "DIV_"
const TAG_BONE: &[u8; 4] = b"ENOB"; // "BONE"
const TAG_VERTICES: &[u8; 4] = b"__8U"; // "U8__" — vertices (count = bytes)
const TAG_INDICES: &[u8; 4] = b"_61U"; // "U16_" — u16 indices

const TAG_BATCHES: &[u8; 4] = b"_TAB"; // "BAT_"

pub struct M3File<'data> {
    data: &'data [u8],
    version: M3Version,
    tags: &'data [MdIndexEntry],
}

impl<'data> M3File<'data> {
    pub fn from_bytes(data: &'data [u8]) -> Result<Self> {
        let version = detect_version(data)?;

        ensure!(data.len() >= 12, "file too small for the M3 header");

        // Common header layout for all M3 versions (Md32/Md33/Md34):
        //   +0  tag(4)           magic bytes
        //   +4  index_offset(4)  byte offset of tag table
        //   +8  index_size(4)    byte size of tag table
        let index_offset = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
        let index_size_bytes = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;

        let tag_size = std::mem::size_of::<MdIndexEntry>();
        let num_tags = index_size_bytes; // stored as count of entries, not byte size

        debug!(
            "M3 magic={:?} version={:?} num_tags={} tag_offset={}",
            std::str::from_utf8(&data[..4]).unwrap_or("?"),
            version,
            num_tags,
            index_offset,
        );

        let tags_end = index_offset + num_tags * tag_size;
        ensure!(
            data.len() >= tags_end,
            "file truncated: tag table doesn't fit"
        );

        let tags: &[MdIndexEntry] = cast_slice(&data[index_offset..tags_end]);

        Ok(M3File {
            data,
            version,
            tags,
        })
    }

    // ── Statistics ──────────────────────────────────────────────────────────

    pub fn mesh_count(&self) -> usize {
        // Number of REGN tags = number of regions (sub-meshes).
        self.tags.iter().filter(|t| t.tag_bytes() == *b"NGER").count()
    }

    pub fn material_count(&self) -> usize {
        // MAT_ count = repetitions of elements in the _TAM tag.
        self.tags
            .iter()
            .find(|t| t.tag_bytes() == *b"_TAM")
            .map(|t| t.repetitions as usize)
            .unwrap_or(0)
    }

    /// Number of MADD materials (tag "DDAM"). MADD is a node-based material
    /// system used by newer HotS models (Tracer etc.).
    pub fn madd_count(&self) -> usize {
        self.tags
            .iter()
            .find(|t| t.tag_bytes() == *b"DDAM")
            .map(|t| t.repetitions as usize)
            .unwrap_or(0)
    }

    /// List of texture paths (CHAR strings) for the MADD at `madd_idx`.
    /// Returned in the order they appear in the file. MADD has no explicit
    /// diff/norm/emis slot tags — the ordering is purely the HotS naming
    /// convention (filename suffix `_diff` / `_norm` / `_emis` / `_spec` / `_ao`);
    /// see `slot_from_filename` in `glb/mod.rs`.
    pub fn madd_texture_paths(&self, madd_idx: usize) -> Result<Vec<String>> {
        let Some(tag_idx) = self.find_tag(b"DDAM") else { return Ok(Vec::new()); };
        let entry = &self.tags[tag_idx];
        let version = entry.version;

        // texture_paths offset inside MADD per version:
        //   v1 (140B): name(12)+unk2(12)+unk4(12)            → +36
        //   v2 (152B): name(12)+unk2(12)+unk3(12)+unk4(12)   → +48
        //   v3 (160B): same as v2                            → +48
        let (file_elem_sz, tex_paths_off) = match version {
            1 => (140usize, 36usize),
            2 => (152, 48),
            _ => (160, 48), // v3 and unknown versions default to v3 layout
        };

        if madd_idx >= entry.repetitions as usize { return Ok(Vec::new()); }

        let base = entry.offset as usize + madd_idx * file_elem_sz;
        let r_start = base + tex_paths_off;
        if r_start + 12 > self.data.len() { return Ok(Vec::new()); }

        let entries = u32::from_le_bytes(self.data[r_start..r_start + 4].try_into().unwrap());
        let index = u32::from_le_bytes(self.data[r_start + 4..r_start + 8].try_into().unwrap());
        let flags = u32::from_le_bytes(self.data[r_start + 8..r_start + 12].try_into().unwrap());
        let texture_paths_ref = Reference { entries, index, flags };

        debug!(
            "MADD[{}] v{} texture_paths: entries={} tag_idx={} flags={:#x}",
            madd_idx, version, entries, index, flags
        );

        if entries == 0 { return Ok(Vec::new()); }
        let schrs: Vec<Schr> = self.read_ref_slice::<Schr>(&texture_paths_ref)?;

        let mut out = Vec::with_capacity(schrs.len());
        for schr in &schrs {
            let s = self.read_char(&schr.path).unwrap_or("").to_owned();
            out.push(s);
        }
        Ok(out)
    }

    pub fn bone_count(&self) -> usize {
        self.tags
            .iter()
            .find(|t| t.tag_bytes() == *TAG_BONE)
            .map(|t| t.repetitions as usize)
            .unwrap_or(0)
    }

    // ── Geometry ────────────────────────────────────────────────────────────

    /// Read `vertex_flags` from the MODL tag (LE name "LDOM").
    /// The flags determine which components a vertex carries and how big it is.
    /// Offset of `vertex_flags` within MODL is 96 bytes (per structures.xml).
    pub fn vertex_flags(&self) -> u32 {
        const VERTEX_FLAGS_OFFSET: usize = 96;
        // model_name(12) + flags(4) + sequences(12) + stc(12) + stg(12) +
        // bone_anim_sets(12) + split_count(4) + sts(12) + bones(12) + skin_bone_count(4) = 96
        let Some(idx) = self.find_tag(b"LDOM") else {
            return 0x180_007d;
        };
        let entry = &self.tags[idx];
        let start = entry.offset as usize + VERTEX_FLAGS_OFFSET;
        if start + 4 > self.data.len() {
            return 0x180_007d;
        }
        u32::from_le_bytes(
            self.data[start..start + 4]
                .try_into()
                .unwrap_or([0x7d, 0x00, 0x80, 0x01]),
        )
    }

    /// Reads a Reference (12 bytes: entries, index, flags) at the given byte
    /// offset inside the MODL tag. Same offsets work across all MODL versions
    /// for the prefix fields (up through bone_lookup at +124).
    fn read_modl_ref(&self, field_offset: usize) -> Option<Reference> {
        let modl_idx = self.find_tag(b"LDOM")?;
        let modl_off = self.tags[modl_idx].offset as usize;
        let start = modl_off + field_offset;
        if start + 12 > self.data.len() {
            return None;
        }
        let entries = u32::from_le_bytes(self.data[start..start + 4].try_into().ok()?);
        let index = u32::from_le_bytes(self.data[start + 4..start + 8].try_into().ok()?);
        let flags = u32::from_le_bytes(self.data[start + 8..start + 12].try_into().ok()?);
        Some(Reference { entries, index, flags })
    }

    /// MODL.bones (offset 80) → BONE tag entries.
    pub fn bones(&self) -> Result<Vec<Bone>> {
        match self.read_modl_ref(80) {
            Some(r) if r.entries > 0 => self.read_ref_slice::<Bone>(&r),
            _ => Ok(Vec::new()),
        }
    }

    /// MODL.bone_lookup (offset 124) → U16_ tag entries.
    pub fn bone_lookup(&self) -> Result<Vec<u16>> {
        match self.read_modl_ref(124) {
            Some(r) if r.entries > 0 => self.read_ref_slice::<u16>(&r),
            _ => Ok(Vec::new()),
        }
    }

    /// IREF entries (per-bone inverse rest matrices). Found by tag b"FERI"
    /// rather than via MODL.bone_rests, since the bone_rests offset moves
    /// across MODL versions but only one IREF tag exists per file.
    pub fn bone_rests(&self) -> Result<Vec<Iref>> {
        match self.find_tag(b"FERI") {
            Some(idx) => self.read_tag_slice::<Iref>(idx),
            None => Ok(Vec::new()),
        }
    }

    /// Compute the vertex stride from `vertex_flags` per structures.xml.
    pub fn vertex_stride(&self) -> usize {
        let flags = self.vertex_flags();
        debug!("vertex_flags = 0x{:08X}", flags);
        let mut size: usize = 12; // pos (always present)

        if flags & 0x000020 != 0 {
            size += 4;
        } // skin0: 2×(lookup+weight) = 4B
        if flags & 0x000040 != 0 {
            size += 4;
        } // skin1: 2×(lookup+weight) = 4B
        if flags & 0x000080 != 0 {
            size += 12;
        } // unknown
        if flags & 0x000100 != 0 {
            size += 4;
        } // unknown
        if flags & 0x000200 != 0 {
            size += 4;
        } // color (COL)
        if flags & 0x000400 != 0 {
            size += 4;
        } // unknown
        if flags & 0x000800 != 0 {
            size += 4;
        } // unknown
        if flags & 0x001000 != 0 {
            size += 4;
        } // unknown
        if flags & 0x002000 != 0 {
            size += 8;
        } // fuv0 (float vec2)
        if flags & 0x004000 != 0 {
            size += 8;
        } // fuv1
        if flags & 0x008000 != 0 {
            size += 8;
        } // fuv2
        if flags & 0x010000 != 0 {
            size += 8;
        } // fuv3
        if flags & 0x020000 != 0 {
            size += 4;
        } // uv0 (int16×2)
        if flags & 0x040000 != 0 {
            size += 4;
        } // uv1
        if flags & 0x080000 != 0 {
            size += 4;
        } // uv2
        if flags & 0x100000 != 0 {
            size += 4;
        } // uv3
        if flags & 0x200000 != 0 {
            size += 12;
        } // normal as vec3 float?
        if flags & 0x400000 != 0 {
            size += 12;
        } // tangent as vec3 float?
        if flags & 0x800000 != 0 {
            size += 4;
        } // normal (Vector3As3uint8 + sign)
        if flags & 0x1000000 != 0 {
            size += 4;
        } // tangent
        if flags & 0x2000000 != 0 {
            size += 4;
        } // unknown
        if flags & 0x4000000 != 0 {
            size += 12;
        } // unknown
        if flags & 0x8000000 != 0 {
            size += 12;
        } // unknown
        if flags & 0x10000000 != 0 {
            size += 4;
        } // unknown
        if flags & 0x20000000 != 0 {
            size += 4;
        } // unknown
        if flags & 0x40000000 != 0 {
            size += 4;
        } // uv4

        debug!("vertex_stride from flags: {} bytes", size);
        size
    }

    /// Vertex buffer — raw bytes (tag "__8U", count = bytes).
    pub fn vertex_data(&self) -> Result<&'data [u8]> {
        let idx = self
            .find_tag(TAG_VERTICES)
            .ok_or_else(|| anyhow::anyhow!("vertex tag '__8U' not found"))?;
        let entry = &self.tags[idx];
        let start = entry.offset as usize;
        let end = start + entry.repetitions as usize; // U8: repetitions = bytes
        ensure!(end <= self.data.len(), "vertex buffer out of bounds");
        debug!(
            "vertex_data: tag[{}] offset={} size={}",
            idx, start, entry.repetitions
        );
        Ok(&self.data[start..end])
    }

    /// All Divisions (tag "_VID", count = element count).
    pub fn divisions(&self) -> Result<Vec<Div>> {
        let idx = self
            .find_tag(TAG_DIV)
            .ok_or_else(|| anyhow::anyhow!("tag '_VID' (DIV_) not found"))?;
        self.read_tag_slice::<Div>(idx)
    }

    /// Regions of a Division — addressed via the Reference inside the Division.
    /// Returns the regions and the REGN tag version (needed for the v≤2 face
    /// fix-up in `processor`).
    pub fn regions(&self, div: &Div) -> Result<(Vec<Regn>, u32)> {
        if div.regions.entries == 0 {
            return Ok((Vec::new(), 0));
        }
        let tag_idx = div.regions.index as usize;
        ensure!(tag_idx < self.tags.len(), "regions ref out of bounds");

        let entry = &self.tags[tag_idx];
        let version = entry.version;
        let count = div.regions.entries as usize;

        // Region size depends on the NGER (REGN) tag version. Matches
        // RegnV2..RegnV5 in structures.rs.
        let file_elem_sz: usize = match version {
            0 | 1 | 2 => 28, // till_v2: u16 first_vertex_index/vertex_count, no unknown01
            3 => 36,         // +unknown01, u32 first_vertex_index/vertex_count
            4 => 40,         // +flags
            _ => 48,         // v5+: +uv_multiply, +uv_offset
        };
        let our_sz = std::mem::size_of::<Regn>(); // 48

        debug!(
            "read_ref_slice tag[{}] {:?} offset={} entries={} elem={}B (file) / {}B (ours) ver={}",
            tag_idx,
            std::str::from_utf8(&entry.tag_bytes()).unwrap_or("?"),
            entry.offset,
            count,
            file_elem_sz,
            our_sz,
            version,
        );

        let start = entry.offset as usize;
        let mut result = Vec::with_capacity(count);

        for i in 0..count {
            let off = start + i * file_elem_sz;
            if off + file_elem_sz > self.data.len() {
                break;
            }

            let raw = &self.data[off..off + file_elem_sz];
            let mut buf = [0u8; 48];

            if version <= 2 {
                // RegnV2 layout (28B): id(4) | first_vtx u16 | vtx_count u16 | first_face u32 |
                //   face_count u32 | bone_count u16 | first_bone_lookup u16 | bone_lookup_count u16 |
                //   unknown02 u16 | vertex_lookups_used u8 | unknown04 u8 | root_bone u16
                // Map to RegnV5 (48B): id(4) unknown01(4) first_vtx u32 vtx_count u32 ...
                buf[0..4].copy_from_slice(&raw[0..4]); // id
                // unknown01 = 0 (already in buf)
                let fv = u16::from_le_bytes([raw[4], raw[5]]) as u32;
                let vc = u16::from_le_bytes([raw[6], raw[7]]) as u32;
                buf[8..12].copy_from_slice(&fv.to_le_bytes());
                buf[12..16].copy_from_slice(&vc.to_le_bytes());
                buf[16..28].copy_from_slice(&raw[8..20]); // first_face..bone_lookup_count
                buf[28..36].copy_from_slice(&raw[20..28]); // unknown02..root_bone
            } else {
                let copy_len = file_elem_sz.min(our_sz);
                buf[..copy_len].copy_from_slice(&raw[..copy_len]);
            }

            // For v<5: uv_multiply isn't in the file — default to 16.0; uv_offset = 0.
            if version < 5 {
                let default_uv_multiply: f32 = 16.0;
                buf[40..44].copy_from_slice(&default_uv_multiply.to_le_bytes());
            }

            let region: Regn = unsafe { std::ptr::read(buf.as_ptr() as *const Regn) };
            result.push(region);
        }

        Ok((result, version))
    }

    /// u16 triangle indices for a Division.
    pub fn face_indices(&self, div: &Div) -> Result<Vec<u16>> {
        self.read_ref_slice::<u16>(&div.faces)
    }

    /// Batch records of a Division (BAT_).
    pub fn batches(&self, div: &Div) -> Result<Vec<Bat>> {
        self.read_ref_slice::<Bat>(&div.batches)
    }

    // ── Materials ───────────────────────────────────────────────────────────

    /// Material references (MATM, tag b"MTAM").
    pub fn material_references(&self) -> Result<Vec<crate::m3::structures::Matm>> {
        match self.find_tag(b"MTAM") {
            Some(idx) => self.read_tag_slice::<crate::m3::structures::Matm>(idx),
            None => Ok(Vec::new()),
        }
    }

    /// Offset of the named layer inside MAT_ for the given version.
    fn mat_layer_offset(version: u32, layer: &str) -> Option<usize> {
        match version {
            15 => match layer {
                "diff" => Some(52),
                "decal" => Some(64),
                "spec" => Some(76),
                "emis1" => Some(88),
                "emis2" => Some(100),
                "envi" => Some(112),
                "envi_mask" => Some(124),
                "alpha1" => Some(136),
                "alpha2" => Some(148),
                "norm" => Some(160),
                "height" => Some(172),
                "light" => Some(184),
                "ao" => Some(196),
                _ => None,
            },
            16 | 17 | 18 => match layer {
                "diff" => Some(52),
                "decal" => Some(64),
                "spec" => Some(76),
                "gloss" => Some(88),
                "emis1" => Some(100),
                "emis2" => Some(112),
                "envi" => Some(124),
                "envi_mask" => Some(136),
                "alpha1" => Some(148),
                "alpha2" => Some(160),
                "norm" => Some(172),
                "height" => Some(184),
                "light" => Some(196),
                "ao" => Some(208),
                _ => None,
            },
            19 => match layer {
                // v19: hdr_envi_const/diff/spec absent (+12), so layers start at +52.
                "diff" => Some(52),
                "decal" => Some(64),
                "spec" => Some(76),
                "gloss" => Some(88),
                "emis1" => Some(100),
                "emis2" => Some(112),
                "envi" => Some(124),
                "envi_mask" => Some(136),
                "alpha1" => Some(148),
                "alpha2" => Some(160),
                "norm" => Some(172),
                "height" => Some(184),
                "light" => Some(196),
                "ao" => Some(208),
                "norm_blend1_mask" => Some(220),
                "norm_blend2_mask" => Some(232),
                "norm_blend1" => Some(244),
                "norm_blend2" => Some(256),
                _ => None,
            },
            20 => match layer {
                // v20: adds hdr_envi_const/diff/spec (+12), so layers start at +64.
                "diff" => Some(64),
                "decal" => Some(76),
                "spec" => Some(88),
                "gloss" => Some(100),
                "emis1" => Some(112),
                "emis2" => Some(124),
                "envi" => Some(136),
                "envi_mask" => Some(148),
                "alpha1" => Some(160),
                "alpha2" => Some(172),
                "norm" => Some(184),
                "height" => Some(196),
                "light" => Some(208),
                "ao" => Some(220),
                "norm_blend1_mask" => Some(232),
                "norm_blend2_mask" => Some(244),
                "norm_blend1" => Some(256),
                "norm_blend2" => Some(268),
                _ => None,
            },
            _ => {
                debug!(
                    "MAT_ unknown version {}, defaulting to v20 offsets",
                    version
                );
                Self::mat_layer_offset(20, layer)
            }
        }
    }

    /// Read a u32 field from MAT_ at a fixed offset (same across all versions).
    fn mat_read_u32(&self, mat_idx: usize, field_offset: usize) -> Option<u32> {
        let tag_idx = self.find_tag(b"_TAM")?;
        let entry = &self.tags[tag_idx];
        let version = entry.version;
        let file_elem_sz: usize = match version {
            15 => 268,
            16 | 17 | 18 => 280,
            19 => 340,
            20 => 352,
            v => {
                debug!("MAT_ unknown v{}", v);
                352
            }
        };
        let base = entry.offset as usize + mat_idx * file_elem_sz;
        let end = base + field_offset + 4;
        if end > self.data.len() {
            return None;
        }
        Some(u32::from_le_bytes(
            self.data[base + field_offset..end].try_into().ok()?,
        ))
    }

    /// `blend_mode` from MAT_ (+20). 0=Opaque,1=Blend,2=Erase,3=Add,4=AddAlpha,5=Mod,6=Mod2x.
    pub fn mat_blend_mode(&self, mat_idx: usize) -> u32 {
        self.mat_read_u32(mat_idx, 20).unwrap_or(0)
    }

    /// `alpha_test_threshold` from MAT_ (+40). Stored as uint8 in the low byte of a u32.
    pub fn mat_alpha_threshold(&self, mat_idx: usize) -> u32 {
        // The field is uint8 but read as a u32 LE — the real value lives in the low byte.
        let raw = self.mat_read_u32(mat_idx, 40).unwrap_or(0);
        raw & 0xFF // low byte only
    }

    pub fn mat_flags(&self, mat_idx: usize) -> u32 {
        self.mat_read_u32(mat_idx, 16).unwrap_or(0)
    }
    /// Reference to the named material layer.
    pub fn mat_layer_ref(&self, mat_idx: usize, layer: &str) -> Option<Reference> {
        let tag_idx = self.find_tag(b"_TAM")?;
        let entry = &self.tags[tag_idx];
        let version = entry.version;

        let file_elem_sz: usize = match version {
            15 => 268,
            16 | 17 | 18 => 280,
            19 => 340,
            20 => 352,
            v => {
                debug!("MAT_ unknown v{}", v);
                352
            }
        };

        let layer_off = Self::mat_layer_offset(version, layer)?;

        let base = entry.offset as usize + mat_idx * file_elem_sz;
        let field_end = base + layer_off + 12; // Reference = 12 bytes
        if field_end > self.data.len() {
            debug!("MAT_[{}] layer '{}' out of bounds", mat_idx, layer);
            return None;
        }

        let entries = u32::from_le_bytes(
            self.data[base + layer_off..base + layer_off + 4]
                .try_into()
                .ok()?,
        );
        let index = u32::from_le_bytes(
            self.data[base + layer_off + 4..base + layer_off + 8]
                .try_into()
                .ok()?,
        );
        let flags = u32::from_le_bytes(
            self.data[base + layer_off + 8..base + layer_off + 12]
                .try_into()
                .ok()?,
        );

        debug!(
            "MAT_[{}] v{} layer '{}' at +{}: entries={} index={} flags={:#x}",
            mat_idx, version, layer, layer_off, entries, index, flags
        );

        Some(Reference {
            entries,
            index,
            flags,
        })
    }

    /// Read the first Layer the Reference points at.
    /// Returns just the `color_bitmap` Reference (texture path).
    pub fn read_layer_bitmap_ref(&self, r: &Reference) -> Option<Reference> {
        if r.entries == 0 {
            return None;
        }
        let tag_idx = r.index as usize;
        if tag_idx >= self.tags.len() {
            return None;
        }

        let entry = &self.tags[tag_idx];
        let version = entry.version;
        let start = entry.offset as usize;

        // `color_bitmap` is always the second field: +4 (after id:u32).
        // Holds for ALL LAYR versions (v20..v26).
        let bitmap_off = start + 4;
        if bitmap_off + 12 > self.data.len() {
            return None;
        }

        let entries = u32::from_le_bytes(self.data[bitmap_off..bitmap_off + 4].try_into().ok()?);
        let index = u32::from_le_bytes(self.data[bitmap_off + 4..bitmap_off + 8].try_into().ok()?);
        let flags = u32::from_le_bytes(self.data[bitmap_off + 8..bitmap_off + 12].try_into().ok()?);

        debug!(
            "LAYR v{} tag[{}] color_bitmap: entries={} index={} flags={:#x}",
            version, tag_idx, entries, index, flags
        );

        Some(Reference {
            entries,
            index,
            flags,
        })
    }

    /// Texture path for the named layer of material `mat_idx`.
    pub fn texture_path_for_layer(&self, mat_idx: usize, layer: &str) -> Result<String> {
        let layer_ref = match self.mat_layer_ref(mat_idx, layer) {
            Some(r) => r,
            None => return Ok(String::new()),
        };

        let bitmap_ref = match self.read_layer_bitmap_ref(&layer_ref) {
            Some(r) => r,
            None => {
                debug!("  -> layer '{}': no bitmap ref", layer);
                return Ok(String::new());
            }
        };

        if bitmap_ref.entries == 0 {
            debug!("  -> layer '{}': bitmap.entries = 0", layer);
            return Ok(String::new());
        }

        let s = self.read_char(&bitmap_ref).unwrap_or("");
        debug!("  -> texture path (layer '{}'): {:?}", layer, s);
        Ok(s.to_owned())
    }

    /// Reference to layer_diff for material `mat_idx`. Backwards-compat helper.
    pub fn mat_layer_diff_ref(&self, mat_idx: usize) -> Option<Reference> {
        self.mat_layer_ref(mat_idx, "diff")
    }

    /// Diffuse texture path for material `mat_idx`. Backwards-compat helper.
    pub fn diffuse_texture_path(&self, mat_idx: usize) -> Result<String> {
        self.texture_path_for_layer(mat_idx, "diff")
    }

    /// Read `uv_tiling.default` from a Layer pointed at by the Reference.
    /// Vec2AnimRef: header(8) + default_x(4) + default_y(4) + ...
    /// Returns `(tiling_x, tiling_y)`, defaulting to (1.0, 1.0).
    pub fn read_layer_uv_tiling(&self, r: &Reference) -> (f32, f32) {
        if r.entries == 0 {
            return (1.0, 1.0);
        }
        let tag_idx = r.index as usize;
        if tag_idx >= self.tags.len() {
            return (1.0, 1.0);
        }

        let entry = &self.tags[tag_idx];
        let version = entry.version;
        let start = entry.offset as usize;

        // uv_tiling offset inside LAYR per version.
        let uv_tiling_off: usize = match version {
            20 | 21 | 22 => 244,
            23 => 244,           // triplanar is added AFTER color_brightness
            24 | 25 | 26 => 252, // +noise_amplitude(4)+noise_frequency(4)
            _ => 244,
        };

        // Vec2AnimRef.default starts at +8 (after the header).
        let default_off = start + uv_tiling_off + 8;
        if default_off + 8 > self.data.len() {
            return (1.0, 1.0);
        }

        let tx = f32::from_le_bytes(
            self.data[default_off..default_off + 4]
                .try_into()
                .unwrap_or([0, 0, 128, 63]),
        );
        let ty = f32::from_le_bytes(
            self.data[default_off + 4..default_off + 8]
                .try_into()
                .unwrap_or([0, 0, 128, 63]),
        );

        // tiling = 0 → treat as 1 (defensive against div-by-zero).
        let tx = if tx.abs() < 1e-6 { 1.0 } else { tx };
        let ty = if ty.abs() < 1e-6 { 1.0 } else { ty };

        debug!(
            "LAYR v{} uv_tiling at +{}: ({}, {})",
            version, uv_tiling_off, tx, ty
        );
        (tx, ty)
    }

    // Backwards-compat helper.
    pub fn read_layer(&self, r: &Reference) -> Result<Option<Layr>> {
        if r.entries == 0 {
            return Ok(None);
        }
        let tag_idx = r.index as usize;
        if tag_idx >= self.tags.len() {
            return Ok(None);
        }

        let entry = &self.tags[tag_idx];
        let version = entry.version;
        let file_elem_sz: usize = match version {
            20 | 21 | 22 => 356,
            23 => 428,
            24 => 436,
            25 => 468,
            26 => 464,
            v => {
                debug!("LAYR unknown v{}", v);
                356
            }
        };

        let start = entry.offset as usize;
        if start + file_elem_sz > self.data.len() {
            return Ok(None);
        }

        let raw = &self.data[start..start + file_elem_sz];
        let our_sz = std::mem::size_of::<Layr>(); // 356
        let mut buf = [0u8; 356];
        buf[..file_elem_sz.min(our_sz)].copy_from_slice(&raw[..file_elem_sz.min(our_sz)]);

        let layer: Layr = unsafe { std::ptr::read(buf.as_ptr() as *const Layr) };
        Ok(Some(layer))
    }

    // ── Animations (SEQS / STG_ / STC_) ─────────────────────────────────────

    /// MODL.sequences (offset 16) → SEQS array. Supports versions v1 (96 bytes)
    /// and v2 (92 bytes); v1 carries an extra `unknown05: u32` field that we
    /// ignore on read.
    pub fn sequences(&self) -> Result<Vec<Seqs>> {
        let r = match self.read_modl_ref(16) {
            Some(r) if r.entries > 0 => r,
            _ => return Ok(Vec::new()),
        };
        let tag_idx = r.index as usize;
        ensure!(tag_idx < self.tags.len(), "SEQS ref out of bounds");
        let entry = &self.tags[tag_idx];
        let version = entry.version;
        let count = r.entries as usize;
        let start = entry.offset as usize;

        let our_sz = std::mem::size_of::<Seqs>(); // 92
        let file_elem_sz: usize = match version {
            1 => std::mem::size_of::<SeqsV1>(), // 96
            _ => our_sz,                        // v2 = 92
        };

        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let off = start + i * file_elem_sz;
            ensure!(off + file_elem_sz <= self.data.len(), "SEQS[{}] out of bounds", i);
            let raw = &self.data[off..off + file_elem_sz];
            let mut buf = [0u8; std::mem::size_of::<Seqs>()];
            if version == 1 {
                // v1 (96B) → v2 layout (92B): drop `unknown05` at +52 (4 bytes)
                buf[..52].copy_from_slice(&raw[..52]);
                buf[52..our_sz].copy_from_slice(&raw[56..56 + (our_sz - 52)]);
            } else {
                buf[..our_sz.min(file_elem_sz)]
                    .copy_from_slice(&raw[..our_sz.min(file_elem_sz)]);
            }
            let s: Seqs = unsafe { std::ptr::read(buf.as_ptr() as *const Seqs) };
            out.push(s);
        }
        Ok(out)
    }

    /// MODL.sequence_transformation_groups (offset 40) → STG_ array.
    pub fn sequence_groups(&self) -> Result<Vec<Stg>> {
        match self.read_modl_ref(40) {
            Some(r) if r.entries > 0 => self.read_ref_slice::<Stg>(&r),
            _ => Ok(Vec::new()),
        }
    }

    /// MODL.sequence_transformation_collections (offset 28) → STC_ array.
    pub fn sequence_collections(&self) -> Result<Vec<Stc>> {
        match self.read_modl_ref(28) {
            Some(r) if r.entries > 0 => self.read_ref_slice::<Stc>(&r),
            _ => Ok(Vec::new()),
        }
    }

    /// Read a Reference to a u32 array (anim_ids / anim_refs / stc_indices).
    pub fn read_ref_u32(&self, r: &Reference) -> Result<Vec<u32>> {
        if r.entries == 0 { return Ok(Vec::new()); }
        self.read_ref_slice::<u32>(r)
    }

    /// Read a Reference to an i32 array (frames in SDxx — millisecond timestamps).
    pub fn read_ref_i32(&self, r: &Reference) -> Result<Vec<i32>> {
        if r.entries == 0 { return Ok(Vec::new()); }
        self.read_ref_slice::<i32>(r)
    }

    /// Read a Reference to a VEC3 array (SD3V key values — translation/scale).
    pub fn read_ref_vec3(&self, r: &Reference) -> Result<Vec<Vec3>> {
        if r.entries == 0 { return Ok(Vec::new()); }
        self.read_ref_slice::<Vec3>(r)
    }

    /// Read a Reference to a QUAT array (SD4Q key values — rotation).
    pub fn read_ref_quat(&self, r: &Reference) -> Result<Vec<Quat>> {
        if r.entries == 0 { return Ok(Vec::new()); }
        self.read_ref_slice::<Quat>(r)
    }

    /// Read the SD3V block array referenced by `STC.sd3v`.
    pub fn read_sd3v(&self, r: &Reference) -> Result<Vec<Sd3v>> {
        if r.entries == 0 { return Ok(Vec::new()); }
        self.read_ref_slice::<Sd3v>(r)
    }

    /// Read the SD4Q block array referenced by `STC.sd4q`.
    pub fn read_sd4q(&self, r: &Reference) -> Result<Vec<Sd4q>> {
        if r.entries == 0 { return Ok(Vec::new()); }
        self.read_ref_slice::<Sd4q>(r)
    }

    // ── Tag lookup ──────────────────────────────────────────────────────────

    pub fn find_tag(&self, tag_le: &[u8; 4]) -> Option<usize> {
        self.tags.iter().position(|t| t.tag_bytes() == *tag_le)
    }

    pub fn dump_tags(&self) {
        for (i, tag) in self.tags.iter().enumerate() {
            let tb = tag.tag_bytes();
            let name = std::str::from_utf8(&tb).unwrap_or("????");
            debug!(
                "  tags[{:3}] {:?} offset={:8} count={}",
                i, name, tag.offset, tag.repetitions
            );
        }
    }

    pub fn version(&self) -> M3Version {
        self.version
    }

    // ── Internal helpers ────────────────────────────────────────────────────

    /// Read every element of a tag. `repetitions` is the count of T values.
    fn read_tag_slice<T: bytemuck::Pod>(&self, tag_idx: usize) -> Result<Vec<T>> {
        let entry = &self.tags[tag_idx];
        let elem_sz = std::mem::size_of::<T>();
        let start = entry.offset as usize;
        let byte_len = entry.repetitions as usize * elem_sz;
        let end = start + byte_len;

        let tb = entry.tag_bytes();
        debug!(
            "read_tag_slice tag[{}] {:?} offset={} count={} elem={}B => {}B total",
            tag_idx,
            std::str::from_utf8(&tb).unwrap_or("?"),
            start,
            entry.repetitions,
            elem_sz,
            byte_len,
        );

        ensure!(
            end <= self.data.len(),
            "tag[{}] out of bounds (end={} > file={})",
            tag_idx,
            end,
            self.data.len()
        );
        ensure!(
            byte_len % elem_sz == 0,
            "byte_len {} is not a multiple of elem_sz {}",
            byte_len,
            elem_sz
        );

        Ok(self.copy_aligned(&self.data[start..end]))
    }

    /// Read elements via Reference. `ref.entries` = element count, `ref.index` = tag index.
    fn read_ref_slice<T: bytemuck::Pod>(&self, r: &Reference) -> Result<Vec<T>> {
        if r.entries == 0 {
            return Ok(Vec::new());
        }

        let tag_idx = r.index as usize;
        ensure!(
            tag_idx < self.tags.len(),
            "ref index {} out of bounds (tags={})",
            tag_idx,
            self.tags.len()
        );

        let entry = &self.tags[tag_idx];
        let elem_sz = std::mem::size_of::<T>();
        let start = entry.offset as usize;
        let byte_len = r.entries as usize * elem_sz; // entries from Reference, not the tag's repetitions!
        let end = start + byte_len;

        let tb = entry.tag_bytes();
        debug!(
            "read_ref_slice tag[{}] {:?} offset={} entries={} elem={}B => {}B",
            tag_idx,
            std::str::from_utf8(&tb).unwrap_or("?"),
            start,
            r.entries,
            elem_sz,
            byte_len,
        );

        ensure!(
            end <= self.data.len(),
            "ref[{}] out of bounds (end={} > file={})",
            tag_idx,
            end,
            self.data.len()
        );

        Ok(self.copy_aligned(&self.data[start..end]))
    }

    /// CHAR string via Reference (count = bytes, same as a tag).
    pub fn read_char(&self, r: &Reference) -> Result<&'data str> {
        if r.entries == 0 {
            return Ok("");
        }
        let tag_idx = r.index as usize;
        ensure!(tag_idx < self.tags.len(), "char ref out of bounds");
        let entry = &self.tags[tag_idx];
        let start = entry.offset as usize;
        let end = start + entry.repetitions as usize;
        ensure!(end <= self.data.len(), "char data out of bounds");
        let bytes = &self.data[start..end];
        let bytes = bytes
            .iter()
            .position(|&b| b == 0)
            .map_or(bytes, |i| &bytes[..i]);
        from_utf8(bytes).map_err(|e| anyhow::anyhow!("invalid UTF-8: {e}"))
    }

    /// Copy bytes into a `Vec<T>` while honouring alignment.
    fn copy_aligned<T: bytemuck::Pod>(&self, raw: &[u8]) -> Vec<T> {
        let elem_sz = std::mem::size_of::<T>();
        match bytemuck::try_cast_slice::<u8, T>(raw) {
            Ok(slice) => slice.to_vec(),
            Err(_) => {
                debug!("copy_aligned: unaligned, {} bytes", raw.len());
                let mut out = Vec::with_capacity(raw.len() / elem_sz);
                for chunk in raw.chunks_exact(elem_sz) {
                    // SAFETY: T: Pod, chunk is the right size; we use unaligned read.
                    let val = unsafe { std::ptr::read_unaligned(chunk.as_ptr() as *const T) };
                    out.push(val);
                }
                out
            }
        }
    }
}
