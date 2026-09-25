//! M3 materials → glTF PBR materials, and the texture/blend summary effects
//! carry in their `extras`.

use super::buffers::{Buffers, Images};
use super::json_builder::GltfMaterial;
use super::ktx2::TextureRole;
use crate::fx::{self, FxItem};
use crate::m3::reader::M3File;
use crate::m3::structures::Matm;
use crate::processor::MeshDataSoA;
use tracing::debug;

/// `MATM.mat_type` of a standard material (`MAT_`).
const MAT_STANDARD: u32 = 1;
/// `MATM.mat_type` of a node-based material (`MADD`, newer HotS heroes).
const MAT_MADD: u32 = 12;

/// Material context: the model's material tables plus where textures go.
pub(super) struct Materials<'a, 'm> {
    m3:         &'a M3File<'m>,
    matms:      Vec<Matm>,
    mat_count:  usize,
    madd_count: usize,
}

/// The glTF materials that were emitted and how MATM indices map onto them.
pub(super) struct MaterialTable {
    pub json:  Vec<GltfMaterial>,
    /// `remap[matm_index]` → glTF material index.
    pub remap: Vec<Option<usize>>,
}

impl MaterialTable {
    /// glTF material for a region's MATM index, if it was emitted.
    pub fn resolve(&self, matm: Option<usize>) -> Option<usize> {
        matm.and_then(|i| self.remap.get(i).copied().flatten())
    }
}

impl<'a, 'm> Materials<'a, 'm> {
    pub fn new(m3: &'a M3File<'m>) -> Self {
        let matms = m3.material_references().unwrap_or_default();
        let (mat_count, madd_count) = (m3.material_count(), m3.madd_count());
        debug!("M3 materials: MAT_={}, MADD={}, MATM={}", mat_count, madd_count, matms.len());
        Self { m3, matms, mat_count, madd_count }
    }

    /// Emit a glTF material for every MATM entry some region actually uses —
    /// an unreferenced material trips the validator's `UNUSED_OBJECT`.
    pub fn for_meshes(&self, meshes: &[MeshDataSoA], bufs: &mut Buffers, images: &mut Images<'_>) -> MaterialTable {
        let used: ahash::AHashSet<usize> = meshes
            .iter()
            .flat_map(|m| m.region_primitives.iter().filter_map(|rp| rp.material_index))
            .collect();
        let mut table = MaterialTable { json: Vec::new(), remap: vec![None; self.matms.len()] };
        for (matm_idx, matm) in self.matms.iter().enumerate() {
            if !used.contains(&matm_idx) {
                continue;
            }
            let mat_idx = matm.material_index as usize;
            let material = match matm.mat_type {
                MAT_STANDARD if mat_idx < self.mat_count => self.standard(mat_idx, bufs, images),
                MAT_MADD if mat_idx < self.madd_count => self.madd(mat_idx, bufs, images),
                t => {
                    debug!("matm[{}] mat_type={} mat_idx={} — unsupported, skipping", matm_idx, t, mat_idx);
                    continue;
                }
            };
            table.remap[matm_idx] = Some(table.json.len());
            table.json.push(material);
        }
        table
    }

    /// A `MAT_` material: explicit diffuse / normal / emissive / AO layers.
    fn standard(&self, mat_idx: usize, bufs: &mut Buffers, images: &mut Images<'_>) -> GltfMaterial {
        let m3 = self.m3;
        let diff_path = m3.texture_path_for_layer(mat_idx, "diff");
        let mut load = |layer: &str, role| images.load(bufs, &m3.texture_path_for_layer(mat_idx, layer), role);
        let base_color_texture = load("diff", TextureRole::Color);
        let normal_texture = load("norm", TextureRole::NormalMap);
        let emissive_texture = load("emis1", TextureRole::Color);
        let occlusion_texture = load("ao", TextureRole::Data);

        let blend_mode = m3.mat_blend_mode(mat_idx);
        let alpha_threshold = m3.mat_alpha_threshold(mat_idx);
        let flags = m3.mat_flags(mat_idx);
        debug!("MAT_[{}] blend_mode={} alpha_threshold={} flags=0x{:08X}", mat_idx, blend_mode, alpha_threshold, flags);

        let (alpha_mode, alpha_cutoff) = if blend_mode != 0 {
            (Some("BLEND"), 0.5)
        } else if alpha_threshold > 0 {
            (Some("MASK"), alpha_threshold as f32 / 255.0)
        } else {
            (None, 0.5)
        };

        // Flat-colour layers (LAYR colour bit, no bitmap): m3studio renders
        // these — e.g. an additive energy glow whose colour is in `color_value`.
        let emissive_factor = if emissive_texture.is_some() {
            [1.0; 3]
        } else {
            m3.layer_color(mat_idx, "emis1").map_or([0.0; 3], |c| [c[0], c[1], c[2]])
        };
        // A diffuse bitmap modulates white; a diffuse colour layer supplies the
        // colour; a material with neither has no albedo source — black, not
        // glTF's white, or textureless effect geometry renders as white panels.
        let base_color_factor = if base_color_texture.is_some() || !diff_path.is_empty() {
            [1.0; 4]
        } else {
            m3.layer_color(mat_idx, "diff").unwrap_or([0.0, 0.0, 0.0, 1.0])
        };

        GltfMaterial {
            name: format!("material_{mat_idx}"),
            base_color_texture,
            base_color_factor,
            normal_texture,
            emissive_texture,
            occlusion_texture,
            metallic_factor: 0.0,
            roughness_factor: 1.0,
            emissive_factor,
            alpha_mode,
            alpha_cutoff,
            double_sided: flags & 0x8 != 0,
        }
    }

    /// A `MADD` material: an unlabelled texture list, routed by file suffix.
    fn madd(&self, mat_idx: usize, bufs: &mut Buffers, images: &mut Images<'_>) -> GltfMaterial {
        let paths = self.m3.madd_texture_paths(mat_idx).unwrap_or_default();
        debug!("MADD[{}]: {} texture(s)", mat_idx, paths.len());
        let mut slots: [Option<usize>; 4] = [None; 4];
        for p in &paths {
            let Some(slot) = MaddSlot::from_filename(p) else { continue };
            // The first texture for a slot wins.
            if slots[slot as usize].is_none() {
                slots[slot as usize] = images.load(bufs, p, slot.role());
            }
        }
        let [diff, norm, emis, ao] = slots;
        // Same enum as `MAT_`; MADD has no alpha-test threshold we know of.
        let blend_mode = self.m3.madd_blend_mode(mat_idx);
        debug!("MADD[{}] blend_mode={}", mat_idx, blend_mode);
        GltfMaterial {
            name: format!("madd_{mat_idx}"),
            base_color_texture: diff,
            base_color_factor: [1.0; 4],
            normal_texture: norm,
            emissive_texture: emis,
            occlusion_texture: ao,
            metallic_factor: 0.0,
            roughness_factor: 1.0,
            emissive_factor: if emis.is_some() { [1.0; 3] } else { [0.0; 3] },
            alpha_mode: (blend_mode != 0).then_some("BLEND"),
            alpha_cutoff: 0.5,
            double_sided: false,
        }
    }

    /// What each effect's material contributes to its `extras`: the diffuse
    /// texture, a flat colour, and the blend mode.
    ///
    /// An emitter's material never becomes a glTF material — nothing in the
    /// scene draws with it — so only its texture travels, for the runtime that
    /// spawns the emitter to sample. Effects reference composites (`CMP_`) as
    /// readily as plain materials, so each reference is first resolved to its
    /// renderable section, the same way region materials are.
    pub fn for_effects(
        &self,
        items: &[FxItem],
        bufs: &mut Buffers,
        images: &mut Images<'_>,
    ) -> ahash::AHashMap<usize, fx::MaterialResolve> {
        let mut out = ahash::AHashMap::new();
        if items.is_empty() {
            return out;
        }
        let resolved = self.m3.resolved_material_refs();
        for requested in items.iter().filter_map(|it| it.matm_index) {
            if out.contains_key(&requested) {
                continue;
            }
            let matm_idx = resolved.get(requested).copied().flatten().unwrap_or(requested);
            let Some(matm) = self.matms.get(matm_idx) else {
                debug!("effect references matm[{}] of {} — no material", matm_idx, self.matms.len());
                continue;
            };
            out.insert(requested, self.effect_material(*matm, bufs, images));
        }
        out
    }

    fn effect_material(&self, matm: Matm, bufs: &mut Buffers, images: &mut Images<'_>) -> fx::MaterialResolve {
        let m3 = self.m3;
        let mat_idx = matm.material_index as usize;
        let mut resolve = fx::MaterialResolve::default();
        match matm.mat_type {
            MAT_STANDARD if mat_idx < self.mat_count => {
                let diff = m3.texture_path_for_layer(mat_idx, "diff");
                resolve.texture = images.load(bufs, &diff, TextureRole::Color);
                // A textureless effect material carries its colour in the layer.
                if resolve.texture.is_none() {
                    resolve.color = m3.layer_color(mat_idx, "diff").or_else(|| m3.layer_color(mat_idx, "emis1"));
                }
                resolve.blend = blend_name(m3.mat_blend_mode(mat_idx));
            }
            MAT_MADD if mat_idx < self.madd_count => {
                let paths = m3.madd_texture_paths(mat_idx).unwrap_or_default();
                if let Some(diff) = paths.iter().find(|p| MaddSlot::from_filename(p) == Some(MaddSlot::Diff)) {
                    resolve.texture = images.load(bufs, diff, TextureRole::Color);
                }
                resolve.blend = blend_name(m3.madd_blend_mode(mat_idx));
            }
            t => debug!("effect material type {} — unsupported", t),
        }
        resolve
    }
}

/// Which optional vertex attributes a primitive should reference, given its
/// material. TANGENT is only meaningful with a normal texture; `TEXCOORD_0` only
/// when the material samples some texture. Emitting them otherwise trips the
/// glTF validator's `UNUSED_MESH_TANGENT` / `UNUSED_OBJECT` notices.
pub(super) fn prim_attr_accessors(
    material_idx: Option<usize>,
    materials:    &[GltfMaterial],
    tangent_acc:  usize,
    texcoord_acc: usize,
) -> (Option<usize>, Option<usize>) {
    let mat = material_idx.and_then(|mi| materials.get(mi));
    let has_normal = mat.is_some_and(|m| m.normal_texture.is_some());
    let has_any_texture = mat.is_some_and(|m| {
        m.base_color_texture.is_some()
            || m.normal_texture.is_some()
            || m.emissive_texture.is_some()
            || m.occlusion_texture.is_some()
    });
    (has_normal.then_some(tangent_acc), has_any_texture.then_some(texcoord_acc))
}

/// M3 `MAT_.blend_mode` → the blend name written into an effect's `extras`.
/// Values are m3studio's `mat_blend`: 0 opaque, 1 alpha blend, 2 add,
/// 3 alpha add, 4 mod, 5 mod 2x.
fn blend_name(blend_mode: u32) -> &'static str {
    match blend_mode {
        0 => "opaque",
        2 | 3 => "add",
        4 | 5 => "multiply",
        _ => "blend",
    }
}

/// MADD material texture slot, derived from the filename suffix.
/// MADD has no explicit diff/norm/emis fields, so textures are routed by the
/// HotS naming convention: `..._diff.dds`, `..._norm.dds`, `..._emis.dds`,
/// `..._ao.dds`. `_spec` is ignored (no metallic-roughness analogue).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MaddSlot {
    Diff,
    Norm,
    Emis,
    Ao,
}

impl MaddSlot {
    fn from_filename(path: &str) -> Option<Self> {
        let stem = std::path::Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if stem.ends_with("_diff") {
            Some(Self::Diff)
        } else if stem.ends_with("_norm") {
            Some(Self::Norm)
        } else if ["_emis", "_emis1", "_emis2"].iter().any(|s| stem.ends_with(s)) {
            Some(Self::Emis)
        } else if stem.ends_with("_ao") {
            Some(Self::Ao)
        } else {
            None
        }
    }

    fn role(self) -> TextureRole {
        match self {
            Self::Diff | Self::Emis => TextureRole::Color,
            Self::Norm => TextureRole::NormalMap,
            Self::Ao => TextureRole::Data,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mat(base: Option<usize>, normal: Option<usize>) -> GltfMaterial {
        GltfMaterial {
            name:               "m".into(),
            base_color_texture: base,
            base_color_factor:  [1.0; 4],
            normal_texture:     normal,
            emissive_texture:   None,
            occlusion_texture:  None,
            metallic_factor:    0.0,
            roughness_factor:   1.0,
            emissive_factor:    [0.0; 3],
            alpha_mode:         None,
            alpha_cutoff:       0.5,
            double_sided:       false,
        }
    }

    #[test]
    fn base_and_normal_keeps_both() {
        let mats = [mat(Some(0), Some(1))];
        assert_eq!(prim_attr_accessors(Some(0), &mats, 7, 9), (Some(7), Some(9)));
    }

    #[test]
    fn base_only_drops_tangent_keeps_uv() {
        let mats = [mat(Some(0), None)];
        assert_eq!(prim_attr_accessors(Some(0), &mats, 7, 9), (None, Some(9)));
    }

    #[test]
    fn no_textures_drops_both() {
        let mats = [mat(None, None)];
        assert_eq!(prim_attr_accessors(Some(0), &mats, 7, 9), (None, None));
    }

    #[test]
    fn no_material_drops_both() {
        let mats = [mat(Some(0), Some(1))];
        assert_eq!(prim_attr_accessors(None, &mats, 7, 9), (None, None));
        // out-of-range index is also treated as no material
        assert_eq!(prim_attr_accessors(Some(5), &mats, 7, 9), (None, None));
    }

    #[test]
    fn madd_slots_by_suffix() {
        assert_eq!(MaddSlot::from_filename("a/B_DIFF.dds"), Some(MaddSlot::Diff));
        assert_eq!(MaddSlot::from_filename("b_emis2.dds"), Some(MaddSlot::Emis));
        assert_eq!(MaddSlot::from_filename("c_spec.dds"), None);
        assert_eq!(MaddSlot::from_filename(""), None);
    }
}
