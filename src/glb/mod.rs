//! Binary GLB (glTF 2.0 Binary) assembler.
//!
//! # GLB file layout
//!
//! ```text
//! ┌───────────────────────────────────────────────────────┐
//! │ GLB Header (12 bytes)                                 │
//! │   magic:   0x46546C67  ("glTF")                      │
//! │   version: 2                                          │
//! │   length:  total file size                            │
//! ├───────────────────────────────────────────────────────┤
//! │ JSON Chunk (variable)                                 │
//! │   chunk_length: u32  (4-byte aligned)                 │
//! │   chunk_type:   0x4E4F534A ("JSON")                   │
//! │   chunk_data:   UTF-8 JSON + spaces padding           │
//! ├───────────────────────────────────────────────────────┤
//! │ BIN Chunk (variable, optional)                        │
//! │   chunk_length: u32  (4-byte aligned)                 │
//! │   chunk_type:   0x004E4942 ("BIN\0")                  │
//! │   chunk_data:   binary buffer (vertices + indices)    │
//! └───────────────────────────────────────────────────────┘
//! ```
//!
//! All data is little-endian. Chunks are 4-byte aligned.

mod json_builder;
mod ktx2;

use crate::assets::TextureCache;
use crate::m3::reader::M3File;
use crate::processor::MeshDataSoA;
use crate::processor::anim::{self, Path as AnimPath, SamplerData};
use anyhow::{Context, Result};
use bytemuck::cast_slice;
use std::io::Write;
use tracing::{debug, warn};

/// Per-conversion options that affect how textures and geometry are packed.
#[derive(Debug, Clone, Copy, Default)]
pub struct PackOptions {
    /// Transcode every embedded texture to KTX2/UASTC and emit the
    /// `KHR_texture_basisu` glTF extension. Requires `toktx` on PATH.
    pub ktx2: bool,
}

// ─── Magic & type constants ──────────────────────────────────────────────────
const GLB_MAGIC:       u32 = 0x46546C67; // "glTF"
const GLB_VERSION:     u32 = 2;
const CHUNK_TYPE_JSON: u32 = 0x4E4F534A; // "JSON"
const CHUNK_TYPE_BIN:  u32 = 0x004E4942; // "BIN\0"

/// Assemble the GLB and write it to disk.
pub fn pack_and_write(
    meshes:       &[MeshDataSoA],
    textures:     &TextureCache,
    m3:           &M3File<'_>,
    anim_sources: &[&M3File<'_>],
    output_path:  &str,
    options:      &PackOptions,
) -> Result<()> {
    let (json_bytes, bin_bytes) =
        build_glb_content(meshes, textures, m3, anim_sources, options)?;

    // Align JSON to 4 bytes (padding = 0x20 spaces).
    let json_padded = align4_json(&json_bytes);
    // Align BIN to 4 bytes (padding = zero bytes).
    let bin_padded  = align4_zeros(&bin_bytes);

    let total_size: u32 = 12 // GLB header
        + 8 + json_padded.len() as u32  // JSON chunk header + data
        + 8 + bin_padded.len()  as u32; // BIN chunk header + data

    debug!(
        "GLB: JSON {}B, BIN {}B, total {}B",
        json_padded.len(), bin_padded.len(), total_size
    );

    let mut file = std::fs::File::create(output_path)
        .with_context(|| format!("cannot create file: {}", output_path))?;

    // ── GLB Header ────────────────────────────────────────────────────────────
    write_u32(&mut file, GLB_MAGIC)?;
    write_u32(&mut file, GLB_VERSION)?;
    write_u32(&mut file, total_size)?;

    // ── JSON Chunk ────────────────────────────────────────────────────────────
    write_u32(&mut file, json_padded.len() as u32)?;
    write_u32(&mut file, CHUNK_TYPE_JSON)?;
    file.write_all(&json_padded)?;

    // ── BIN Chunk ─────────────────────────────────────────────────────────────
    write_u32(&mut file, bin_padded.len() as u32)?;
    write_u32(&mut file, CHUNK_TYPE_BIN)?;
    file.write_all(&bin_padded)?;

    Ok(())
}

/// Convenience helper to build an Accessor with default `normalized=false`.
fn acc(
    bv: usize, off: usize, ct: u32, count: usize, ty: &str,
    min: Option<Vec<f64>>, max: Option<Vec<f64>>,
) -> json_builder::Accessor {
    json_builder::Accessor {
        buffer_view:    bv,
        byte_offset:    off,
        component_type: ct,
        count,
        accessor_type:  ty.into(),
        normalized:     false,
        min,
        max,
    }
}

fn acc_norm(
    bv: usize, off: usize, ct: u32, count: usize, ty: &str,
) -> json_builder::Accessor {
    json_builder::Accessor {
        buffer_view:    bv,
        byte_offset:    off,
        component_type: ct,
        count,
        accessor_type:  ty.into(),
        normalized:     true,
        min: None,
        max: None,
    }
}

/// Build the glTF JSON manifest and the binary GLB buffer.
fn build_glb_content(
    meshes:       &[MeshDataSoA],
    textures:     &TextureCache,
    m3:           &M3File<'_>,
    anim_sources: &[&M3File<'_>],
    options:      &PackOptions,
) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut bin_buf: Vec<u8> = Vec::new();

    let mut accessors:    Vec<json_builder::Accessor>    = Vec::new();
    let mut buffer_views: Vec<json_builder::BufferView>  = Vec::new();
    let mut meshes_json:  Vec<json_builder::GltfMesh>    = Vec::new();

    // ── Load material textures ───────────────────────────────────────────────
    let mat_count = m3.material_count();
    let madd_count = m3.madd_count();
    let matm_list = m3.material_references().unwrap_or_default();
    debug!(
        "M3 materials: MAT_={}, MADD={}, MATM={}",
        mat_count, madd_count, matm_list.len()
    );

    let mut images_json:    Vec<json_builder::GltfImage>    = Vec::new();
    let mut materials_json: Vec<json_builder::GltfMaterial> = Vec::new();

    // Compact materials: only emit MATM entries that are referenced by at
    // least one region primitive — otherwise the glTF validator reports
    // UNUSED_OBJECT. `region_primitives.material_index` holds the matm_list
    // index.
    let used_matm: ahash::AHashSet<usize> = meshes.iter()
        .flat_map(|m| m.region_primitives.iter().filter_map(|rp| rp.material_index))
        .collect();
    // `matref_remap[matm_idx]` → glTF material index (after compaction).
    let mut matref_remap: Vec<Option<usize>> = vec![None; matm_list.len()];

    let want_ktx2 = options.ktx2;
    let mut load_image = |path: &str,
                          buffer_views: &mut Vec<json_builder::BufferView>,
                          bin_buf: &mut Vec<u8>,
                          images_json: &mut Vec<json_builder::GltfImage>|
     -> Option<usize> {
        if path.is_empty() || textures.is_empty() { return None; }
        let (tex_path, mime) = textures.find_with_mime(path)?;

        // Try KTX2 transcode first when requested. On failure (toktx missing,
        // unsupported source format, etc.) fall back to the original bytes.
        let (bytes, final_mime): (Vec<u8>, String) = if want_ktx2 {
            match ktx2::transcode(tex_path) {
                Ok(b) => {
                    debug!(
                        "transcoded {:?} → KTX2/UASTC ({} bytes)",
                        tex_path,
                        b.len()
                    );
                    (b, "image/ktx2".to_owned())
                }
                Err(e) => {
                    warn!(
                        "KTX2 transcode failed for {:?}: {} — embedding original",
                        tex_path, e
                    );
                    match std::fs::read(tex_path) {
                        Ok(b) => (b, mime.to_owned()),
                        Err(e2) => {
                            debug!("failed to read texture {:?}: {}", tex_path, e2);
                            return None;
                        }
                    }
                }
            }
        } else {
            match std::fs::read(tex_path) {
                Ok(b) => {
                    debug!("loading texture {:?} ({} bytes)", tex_path, b.len());
                    (b, mime.to_owned())
                }
                Err(e) => {
                    debug!("failed to read texture {:?}: {}", tex_path, e);
                    return None;
                }
            }
        };

        let bv_idx = push_buffer_view(buffer_views, bin_buf, &bytes, None);
        let img_idx = images_json.len();
        images_json.push(json_builder::GltfImage {
            buffer_view: bv_idx,
            mime_type:   final_mime,
        });
        Some(img_idx)
    };

    for (matm_idx, matm) in matm_list.iter().enumerate() {
        if !used_matm.contains(&matm_idx) {
            continue;
        }
        let mat_idx = matm.material_index as usize;
        let glt = match matm.mat_type {
            // ── Standard MAT_ ────────────────────────────────────────────────
            1 if mat_idx < mat_count => {
                let base_color_tex = load_image(
                    &m3.texture_path_for_layer(mat_idx, "diff").unwrap_or_default(),
                    &mut buffer_views, &mut bin_buf, &mut images_json,
                );
                let normal_tex = load_image(
                    &m3.texture_path_for_layer(mat_idx, "norm").unwrap_or_default(),
                    &mut buffer_views, &mut bin_buf, &mut images_json,
                );
                let emissive_tex = load_image(
                    &m3.texture_path_for_layer(mat_idx, "emis1").unwrap_or_default(),
                    &mut buffer_views, &mut bin_buf, &mut images_json,
                );
                let occlusion_tex = load_image(
                    &m3.texture_path_for_layer(mat_idx, "ao").unwrap_or_default(),
                    &mut buffer_views, &mut bin_buf, &mut images_json,
                );

                let blend_mode      = m3.mat_blend_mode(mat_idx);
                let alpha_threshold = m3.mat_alpha_threshold(mat_idx);
                let mat_flags       = m3.mat_flags(mat_idx);
                debug!(
                    "MAT_[{}] (matm[{}]) blend_mode={} alpha_threshold={} flags=0x{:08X}",
                    mat_idx, matm_idx, blend_mode, alpha_threshold, mat_flags
                );

                let (alpha_mode, alpha_cutoff) = if blend_mode != 0 {
                    (Some("BLEND".to_owned()), 0.5)
                } else if alpha_threshold > 0 {
                    (Some("MASK".to_owned()), alpha_threshold as f32 / 255.0)
                } else {
                    (None, 0.5)
                };
                let double_sided = (mat_flags & 0x8) != 0;
                let emissive_factor = if emissive_tex.is_some() {
                    [1.0f32, 1.0f32, 1.0f32]
                } else {
                    [0.0f32; 3]
                };

                json_builder::GltfMaterial {
                    name:               format!("material_{}", mat_idx),
                    base_color_texture: base_color_tex,
                    normal_texture:     normal_tex,
                    emissive_texture:   emissive_tex,
                    occlusion_texture:  occlusion_tex,
                    metallic_factor:    0.0,
                    roughness_factor:   1.0,
                    emissive_factor,
                    alpha_mode,
                    alpha_cutoff,
                    double_sided,
                }
            }
            // ── MADD (newer HotS heroes; Tracer etc.) ────────────────────────
            12 if mat_idx < madd_count => {
                let paths = m3.madd_texture_paths(mat_idx).unwrap_or_default();
                debug!("MADD[{}] (matm[{}]): {} texture(s)", mat_idx, matm_idx, paths.len());
                let mut diff: Option<usize>  = None;
                let mut norm: Option<usize>  = None;
                let mut emis: Option<usize>  = None;
                let mut ao:   Option<usize>  = None;
                for p in &paths {
                    match slot_from_filename(p) {
                        Some(MaddSlot::Diff) if diff.is_none() => {
                            diff = load_image(p, &mut buffer_views, &mut bin_buf, &mut images_json);
                        }
                        Some(MaddSlot::Norm) if norm.is_none() => {
                            norm = load_image(p, &mut buffer_views, &mut bin_buf, &mut images_json);
                        }
                        Some(MaddSlot::Emis) if emis.is_none() => {
                            emis = load_image(p, &mut buffer_views, &mut bin_buf, &mut images_json);
                        }
                        Some(MaddSlot::Ao) if ao.is_none() => {
                            ao = load_image(p, &mut buffer_views, &mut bin_buf, &mut images_json);
                        }
                        _ => {} // _spec / unknown / slot already filled
                    }
                }
                let emissive_factor = if emis.is_some() {
                    [1.0f32; 3]
                } else {
                    [0.0f32; 3]
                };
                json_builder::GltfMaterial {
                    name:               format!("madd_{}", mat_idx),
                    base_color_texture: diff,
                    normal_texture:     norm,
                    emissive_texture:   emis,
                    occlusion_texture:  ao,
                    metallic_factor:    0.0,
                    roughness_factor:   1.0,
                    emissive_factor,
                    alpha_mode:         None,
                    alpha_cutoff:       0.5,
                    double_sided:       false,
                }
            }
            _ => {
                debug!(
                    "matm[{}] mat_type={} mat_idx={} — unsupported, skipping",
                    matm_idx, matm.mat_type, mat_idx
                );
                continue;
            }
        };
        matref_remap[matm_idx] = Some(materials_json.len());
        materials_json.push(glt);
    }

    // Per-mesh skinning accessors: skin index (in skins_json) if skinned.
    let mut mesh_skin_idx: Vec<Option<usize>> = Vec::with_capacity(meshes.len());

    for (mesh_idx, mesh) in meshes.iter().enumerate() {
        if mesh.vertex_count() == 0 {
            mesh_skin_idx.push(None);
            continue;
        }

        let mut primitives = Vec::new();

        // ── Positions ───────────────────────────────────────────────────────
        let pos_bytes  = mesh.positions_as_bytes();
        let pos_bv_idx = push_buffer_view(&mut buffer_views, &mut bin_buf, &pos_bytes, Some(34962));
        let pos_acc_idx = accessors.len();
        accessors.push(acc(
            pos_bv_idx, 0, 5126, mesh.vertex_count(), "VEC3",
            Some(vec![mesh.aabb_min[0] as f64, mesh.aabb_min[1] as f64, mesh.aabb_min[2] as f64]),
            Some(vec![mesh.aabb_max[0] as f64, mesh.aabb_max[1] as f64, mesh.aabb_max[2] as f64]),
        ));

        // ── Normals ─────────────────────────────────────────────────────────
        let norm_bytes  = mesh.normals_as_bytes();
        let norm_bv_idx = push_buffer_view(&mut buffer_views, &mut bin_buf, &norm_bytes, Some(34962));
        let norm_acc_idx = accessors.len();
        accessors.push(acc(norm_bv_idx, 0, 5126, mesh.vertex_count(), "VEC3", None, None));

        // ── Tangents (VEC4: xyz + sign w) ───────────────────────────────────
        let tang_bytes  = mesh.tangents_as_bytes();
        let tang_bv_idx = push_buffer_view(&mut buffer_views, &mut bin_buf, &tang_bytes, Some(34962));
        let tang_acc_idx = accessors.len();
        accessors.push(acc(tang_bv_idx, 0, 5126, mesh.vertex_count(), "VEC4", None, None));

        // ── UV ───────────────────────────────────────────────────────────────
        let uv_bytes = mesh.uvs_as_bytes();
        let uv_bv_idx = push_buffer_view(&mut buffer_views, &mut bin_buf, &uv_bytes, Some(34962));
        let uv_acc_idx = accessors.len();
        accessors.push(acc(uv_bv_idx, 0, 5126, mesh.vertex_count(), "VEC2", None, None));

        // ── Skin: JOINTS_0 / WEIGHTS_0 (when the mesh is skinned) ───────────
        let (joints_acc_idx, weights_acc_idx) = if mesh.has_skin {
            let j_bytes  = mesh.joints_as_bytes();
            let j_bv_idx = push_buffer_view(&mut buffer_views, &mut bin_buf, j_bytes, Some(34962));
            let j_acc_idx = accessors.len();
            accessors.push(acc(j_bv_idx, 0, 5123, mesh.vertex_count(), "VEC4", None, None));

            let w_bytes  = mesh.weights_as_bytes();
            let w_bv_idx = push_buffer_view(&mut buffer_views, &mut bin_buf, w_bytes, Some(34962));
            let w_acc_idx = accessors.len();
            accessors.push(acc_norm(w_bv_idx, 0, 5121, mesh.vertex_count(), "VEC4"));

            (Some(j_acc_idx), Some(w_acc_idx))
        } else {
            (None, None)
        };

        mesh_skin_idx.push(if mesh.has_skin { Some(0) } else { None });

        // ── Indices ─────────────────────────────────────────────────────────
        let idx_bytes_slice: &[u8] = cast_slice(&mesh.indices);
        let idx_bv_idx = push_buffer_view(
            &mut buffer_views, &mut bin_buf, idx_bytes_slice, Some(34963),
        );

        if !mesh.region_primitives.is_empty() {
            for rp in &mesh.region_primitives {
                if rp.index_count == 0 { continue; }
                let idx_acc_idx = accessors.len();
                accessors.push(acc(
                    idx_bv_idx, rp.index_start * 4, 5125, rp.index_count, "SCALAR", None, None,
                ));

                let material_idx = rp.material_index
                    .and_then(|mi| matref_remap.get(mi).copied().flatten());

                primitives.push(json_builder::Primitive {
                    position_accessor: pos_acc_idx,
                    normal_accessor:   norm_acc_idx,
                    tangent_accessor:  tang_acc_idx,
                    texcoord_accessor: uv_acc_idx,
                    indices_accessor:  idx_acc_idx,
                    material:          material_idx,
                    joints_accessor:   joints_acc_idx,
                    weights_accessor:  weights_acc_idx,
                });
            }
        }

        if primitives.is_empty() {
            let idx_acc_idx = accessors.len();
            accessors.push(acc(
                idx_bv_idx, 0, 5125, mesh.indices.len(), "SCALAR", None, None,
            ));

            let material_idx = if !materials_json.is_empty() {
                Some(mesh_idx.min(materials_json.len() - 1))
            } else {
                None
            };

            primitives.push(json_builder::Primitive {
                position_accessor: pos_acc_idx,
                normal_accessor:   norm_acc_idx,
                tangent_accessor:  tang_acc_idx,
                texcoord_accessor: uv_acc_idx,
                indices_accessor:  idx_acc_idx,
                material:          material_idx,
                joints_accessor:   joints_acc_idx,
                weights_accessor:  weights_acc_idx,
            });
        }

        meshes_json.push(json_builder::GltfMesh {
            name:       format!("mesh_{}", mesh_idx),
            primitives,
        });
    }

    // ── Skeleton ─────────────────────────────────────────────────────────────
    // One skeleton per skinned mesh (shared if bones are common). In M3 the
    // bones are shared across the whole model, so a single skin is enough.
    let any_skinned = meshes.iter().any(|m| m.has_skin);
    let bones = if any_skinned { m3.bones().unwrap_or_default() } else { Vec::new() };
    let bone_rests = if any_skinned { m3.bone_rests().unwrap_or_default() } else { Vec::new() };

    // Scene layout (no separate rotation root — the skinned mesh node must
    // sit directly in scene roots; otherwise glTF emits NODE_SKINNED_MESH_NON_ROOT
    // and ignores the parent transform):
    //   [0..N_bones] bone nodes (in bone-array order, bone i → index i)
    //   [N_bones]    mesh node (with mesh + skin reference)
    //
    // The Z-up → Y-up rotation is already baked into the vertex positions
    // (see `soa::apply_zup_to_yup`). Root bones get the same rotation so the
    // rest pose lines up with the rotated vertices.
    let mut nodes: Vec<json_builder::GltfNode> = Vec::new();
    let mut skins: Vec<json_builder::GltfSkin> = Vec::new();

    // Z-up → Y-up rotation = rotate -90° around X.
    // quat = (-sin(45°), 0, 0, cos(45°)) = (-√2/2, 0, 0, √2/2)
    let zy_quat = [
        -std::f32::consts::FRAC_1_SQRT_2,
        0.0_f32,
        0.0_f32,
        std::f32::consts::FRAC_1_SQRT_2,
    ];

    let bone_node_base = 0usize;
    for (bi, bone) in bones.iter().enumerate() {
        let name = m3.read_char(&bone.name).unwrap_or("").to_owned();
        let t = bone.location.default;
        let r = bone.rotation.default;
        let s = bone.scale.default;

        let (translation, rotation) = if bone.parent < 0 {
            // Root bone: bake Z-up → Y-up into the local TRS:
            //   T' = R · T (rotate translation vector)
            //   Q' = R_quat ⊗ Q (compose rotations)
            let t_rot = rotate_vec_by_quat([t.x, t.y, t.z], zy_quat);
            let q_rot = quat_mul(zy_quat, [r.x, r.y, r.z, r.w]);
            (t_rot, q_rot)
        } else {
            ([t.x, t.y, t.z], [r.x, r.y, r.z, r.w])
        };

        nodes.push(json_builder::GltfNode {
            name:        Some(if name.is_empty() { format!("bone_{}", bi) } else { name }),
            translation: Some(translation),
            rotation:    Some(rotation),
            scale:       Some([s.x, s.y, s.z]),
            mesh:        None,
            skin:        None,
            children:    Vec::new(),
        });
    }
    // Build parent → children. parent == -1 → root bone (becomes a scene root).
    let mut bone_root_nodes: Vec<usize> = Vec::new();
    for (bi, bone) in bones.iter().enumerate() {
        let child_node = bone_node_base + bi;
        if bone.parent < 0 {
            bone_root_nodes.push(child_node);
        } else {
            let parent_node = bone_node_base + bone.parent as usize;
            if parent_node < nodes.len() {
                nodes[parent_node].children.push(child_node);
            }
        }
    }

    // Inverse Bind Matrices.
    //
    // m3studio uses M3 IREF as an orientation matrix for the Blender bone
    // roll, not as a glTF IBM (which equals `inverse(world bind matrix)`).
    // To produce a correct glTF IBM we forward-walk the bone hierarchy to
    // compute the world bind pose, then invert every matrix. That's what we
    // do here.
    //
    // We pass `zy_quat` — the same rotation baked into the root-bone TRS
    // above. It must also feed into the world-matrix computation, otherwise
    // the IBMs won't match the node TRS and skinning will "fold" the model
    // toward the origin.
    let skin_idx = if any_skinned && !bones.is_empty() {
        let world: Vec<[[f32; 4]; 4]> = compute_world_matrices(&bones, zy_quat);
        let mut ibm_bytes: Vec<u8> = Vec::with_capacity(bones.len() * 64);
        for w in &world {
            let inv = invert_4x4(w);
            for col in inv.iter() {
                ibm_bytes.extend_from_slice(bytemuck::cast_slice(col));
            }
        }
        let ibm_bv_idx = push_buffer_view(&mut buffer_views, &mut bin_buf, &ibm_bytes, None);
        let ibm_acc_idx = accessors.len();
        accessors.push(acc(ibm_bv_idx, 0, 5126, bones.len(), "MAT4", None, None));

        let joints: Vec<usize> = (0..bones.len()).map(|i| bone_node_base + i).collect();
        skins.push(json_builder::GltfSkin {
            joints,
            inverse_bind_matrices: Some(ibm_acc_idx),
            // The `skeleton` field is optional; we leave it None to avoid
            // SKIN_SKELETON_INVALID when there are multiple bone roots.
            skeleton: None,
        });
        let _ = bone_rests; // reader retained for future use (rest-pose tooling)
        Some(0)
    } else {
        None
    };

    // Mesh node — after the bones.
    let mesh_node = if !meshes_json.is_empty() {
        let idx = nodes.len();
        nodes.push(json_builder::GltfNode {
            name:        Some("mesh".into()),
            translation: None,
            rotation:    None,
            scale:       None,
            mesh:        Some(0),
            skin:        skin_idx,
            children:    Vec::new(),
        });
        Some(idx)
    } else {
        None
    };

    // glTF requires all joints in a skin to share a common ancestor
    // (SKIN_NO_COMMON_ROOT otherwise). When there are multiple root bones we
    // wrap them in a transform-free "armature" node — the standard layout
    // for skinned models (e.g., CesiumMan).
    //
    // The mesh node stays a direct scene root — required for skinned meshes
    // (NODE_SKINNED_MESH_NON_ROOT).
    let mut scene_roots: Vec<usize> = Vec::new();
    if !bones.is_empty() {
        let armature_idx = nodes.len();
        nodes.push(json_builder::GltfNode {
            name:        Some("armature".into()),
            translation: None,
            rotation:    None,
            scale:       None,
            mesh:        None,
            skin:        None,
            children:    bone_root_nodes,
        });
        scene_roots.push(armature_idx);
    } else {
        scene_roots.extend(bone_root_nodes);
    }
    if let Some(mn) = mesh_node {
        scene_roots.push(mn);
    }
    if scene_roots.is_empty() {
        scene_roots.push(0);
    }

    // ── Animations ───────────────────────────────────────────────────────────
    // bone_node_base = 0: bones occupy the first [0..bones.len()] node indices.
    // Animation sources: the main `.m3` (in case it carries inline SEQS) +
    // every external `.m3a` passed via --anims. If there are no bones, skip.
    let m3_anims = if any_skinned && !bones.is_empty() {
        let mut sources: Vec<&M3File<'_>> = Vec::with_capacity(1 + anim_sources.len());
        sources.push(m3);
        sources.extend(anim_sources.iter().copied());
        anim::build_animations(m3, &sources, bone_node_base).unwrap_or_default()
    } else {
        Vec::new()
    };

    let mut animations_json: Vec<json_builder::GltfAnimation> = Vec::with_capacity(m3_anims.len());
    for src in &m3_anims {
        let mut samplers_json: Vec<json_builder::GltfAnimSampler> = Vec::with_capacity(src.samplers.len());
        for samp in &src.samplers {
            // Input accessor: time (FLOAT, SCALAR). The spec requires min/max.
            let times_bytes: &[u8] = cast_slice(&samp.times_sec);
            let in_bv = push_buffer_view(&mut buffer_views, &mut bin_buf, times_bytes, None);
            let in_acc = accessors.len();
            let (t_min, t_max) = samp.times_sec.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &t| (lo.min(t), hi.max(t)));
            // If there's a single frame, `min == max` — that's valid.
            accessors.push(acc(
                in_bv, 0, 5126, samp.times_sec.len(), "SCALAR",
                Some(vec![t_min as f64]),
                Some(vec![t_max as f64]),
            ));

            // Output accessor.
            let (out_bv, out_acc, out_count, out_type) = match &samp.data {
                SamplerData::Vec3(arr) => {
                    let bytes: &[u8] = cast_slice(arr);
                    let bv = push_buffer_view(&mut buffer_views, &mut bin_buf, bytes, None);
                    let a = accessors.len();
                    accessors.push(acc(bv, 0, 5126, arr.len(), "VEC3", None, None));
                    (bv, a, arr.len(), "VEC3")
                }
                SamplerData::Quat(arr) => {
                    let bytes: &[u8] = cast_slice(arr);
                    let bv = push_buffer_view(&mut buffer_views, &mut bin_buf, bytes, None);
                    let a = accessors.len();
                    accessors.push(acc(bv, 0, 5126, arr.len(), "VEC4", None, None));
                    (bv, a, arr.len(), "VEC4")
                }
            };
            let _ = (out_bv, out_count, out_type);

            samplers_json.push(json_builder::GltfAnimSampler {
                input:         in_acc,
                output:        out_acc,
                interpolation: if samp.linear { "LINEAR" } else { "STEP" },
            });
        }

        let channels_json: Vec<json_builder::GltfAnimChannel> = src.channels.iter().map(|c| {
            let path = match c.path {
                AnimPath::Translation => "translation",
                AnimPath::Rotation    => "rotation",
                AnimPath::Scale       => "scale",
            };
            json_builder::GltfAnimChannel {
                sampler:     c.sampler,
                target_node: c.target_node,
                path,
            }
        }).collect();

        animations_json.push(json_builder::GltfAnimation {
            name:     src.name.clone(),
            samplers: samplers_json,
            channels: channels_json,
        });
    }

    // ── Build JSON ───────────────────────────────────────────────────────────
    let json = json_builder::build_json(
        &meshes_json,
        &accessors,
        &buffer_views,
        bin_buf.len(),
        &images_json,
        &materials_json,
        &nodes,
        &skins,
        &scene_roots,
        &animations_json,
    );

    let _ = mesh_skin_idx; // currently unused, retained for future per-mesh skin mapping
    Ok((json.into_bytes(), bin_buf))
}

fn push_buffer_view(
    views:   &mut Vec<json_builder::BufferView>,
    bin_buf: &mut Vec<u8>,
    data:    &[u8],
    target:  Option<u32>,
) -> usize {
    let offset = align_to_4(bin_buf.len());
    while bin_buf.len() < offset {
        bin_buf.push(0u8);
    }
    bin_buf.extend_from_slice(data);

    let idx = views.len();
    views.push(json_builder::BufferView {
        offset,
        length: data.len(),
        target,
    });
    idx
}

fn align4_json(data: &[u8]) -> Vec<u8> {
    let mut out = data.to_vec();
    while out.len() % 4 != 0 {
        out.push(b' ');
    }
    out
}

fn align4_zeros(data: &[u8]) -> Vec<u8> {
    let mut out = data.to_vec();
    while out.len() % 4 != 0 {
        out.push(0u8);
    }
    out
}

#[inline]
fn align_to_4(n: usize) -> usize {
    (n + 3) & !3
}

// ─── Bone bind pose → IBM ─────────────────────────────────────────────────────

/// Computes per-bone world bind matrices via forward walk of the bone hierarchy.
/// Each matrix is column-major (`m[col][row]`). Bones must be topologically
/// sorted (parent index < child index), which is the standard M3 layout.
///
/// `zy_quat` is the Z-up → Y-up rotation we baked into root bones' TRS in
/// `build_glb_content`; we apply the same rotation here so IBMs match the
/// rotated TRS published in the node table.
fn compute_world_matrices(
    bones: &[crate::m3::structures::Bone],
    zy_quat: [f32; 4],
) -> Vec<[[f32; 4]; 4]> {
    let mut world = Vec::with_capacity(bones.len());
    for (i, bone) in bones.iter().enumerate() {
        let t = bone.location.default;
        let r = bone.rotation.default;
        let s = bone.scale.default;
        let (t3, q4) = if bone.parent < 0 {
            let t_rot = rotate_vec_by_quat([t.x, t.y, t.z], zy_quat);
            let q_rot = quat_mul(zy_quat, [r.x, r.y, r.z, r.w]);
            (t_rot, q_rot)
        } else {
            ([t.x, t.y, t.z], [r.x, r.y, r.z, r.w])
        };
        let local = trs_to_mat4_explicit(t3, q4, [s.x, s.y, s.z]);
        let m = if bone.parent < 0 || (bone.parent as usize) >= i {
            local
        } else {
            mul_4x4(&world[bone.parent as usize], &local)
        };
        world.push(m);
    }
    world
}

fn trs_to_mat4_explicit(t: [f32; 3], q: [f32; 4], s: [f32; 3]) -> [[f32; 4]; 4] {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let xx = x * x; let yy = y * y; let zz = z * z;
    let xy = x * y; let xz = x * z; let yz = y * z;
    let wx = w * x; let wy = w * y; let wz = w * z;

    let r00 = 1.0 - 2.0 * (yy + zz);
    let r01 = 2.0 * (xy - wz);
    let r02 = 2.0 * (xz + wy);
    let r10 = 2.0 * (xy + wz);
    let r11 = 1.0 - 2.0 * (xx + zz);
    let r12 = 2.0 * (yz - wx);
    let r20 = 2.0 * (xz - wy);
    let r21 = 2.0 * (yz + wx);
    let r22 = 1.0 - 2.0 * (xx + yy);

    [
        [r00 * s[0], r10 * s[0], r20 * s[0], 0.0],
        [r01 * s[1], r11 * s[1], r21 * s[1], 0.0],
        [r02 * s[2], r12 * s[2], r22 * s[2], 0.0],
        [t[0],       t[1],       t[2],       1.0],
    ]
}

fn quat_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let (ax, ay, az, aw) = (a[0], a[1], a[2], a[3]);
    let (bx, by, bz, bw) = (b[0], b[1], b[2], b[3]);
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

fn rotate_vec_by_quat(v: [f32; 3], q: [f32; 4]) -> [f32; 3] {
    // v' = q · v · q^-1 (q is unit quaternion).
    let (qx, qy, qz, qw) = (q[0], q[1], q[2], q[3]);
    // t = 2 · (q.xyz × v)
    let tx = 2.0 * (qy * v[2] - qz * v[1]);
    let ty = 2.0 * (qz * v[0] - qx * v[2]);
    let tz = 2.0 * (qx * v[1] - qy * v[0]);
    // v' = v + qw · t + q.xyz × t
    [
        v[0] + qw * tx + (qy * tz - qz * ty),
        v[1] + qw * ty + (qz * tx - qx * tz),
        v[2] + qw * tz + (qx * ty - qy * tx),
    ]
}

fn trs_to_mat4(
    t: crate::m3::structures::Vec3,
    r: crate::m3::structures::Quat,
    s: crate::m3::structures::Vec3,
) -> [[f32; 4]; 4] {
    // Quat (xyzw) → 3x3 rotation matrix.
    let (x, y, z, w) = (r.x, r.y, r.z, r.w);
    let xx = x * x; let yy = y * y; let zz = z * z;
    let xy = x * y; let xz = x * z; let yz = y * z;
    let wx = w * x; let wy = w * y; let wz = w * z;

    let r00 = 1.0 - 2.0 * (yy + zz);
    let r01 = 2.0 * (xy - wz);
    let r02 = 2.0 * (xz + wy);
    let r10 = 2.0 * (xy + wz);
    let r11 = 1.0 - 2.0 * (xx + zz);
    let r12 = 2.0 * (yz - wx);
    let r20 = 2.0 * (xz - wy);
    let r21 = 2.0 * (yz + wx);
    let r22 = 1.0 - 2.0 * (xx + yy);

    // Column-major: m[col][row].
    [
        [r00 * s.x, r10 * s.x, r20 * s.x, 0.0],
        [r01 * s.y, r11 * s.y, r21 * s.y, 0.0],
        [r02 * s.z, r12 * s.z, r22 * s.z, 0.0],
        [t.x,       t.y,       t.z,       1.0],
    ]
}

fn mul_4x4(a: &[[f32; 4]; 4], b: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut out = [[0.0f32; 4]; 4];
    for c in 0..4 {
        for r in 0..4 {
            let mut s = 0.0;
            for k in 0..4 {
                s += a[k][r] * b[c][k];
            }
            out[c][r] = s;
        }
    }
    out
}

/// General 4×4 matrix inversion via cofactor expansion. Operates on column-major
/// arrays. Returns identity if the matrix is singular.
fn invert_4x4(m: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
    // Flatten to row-major for cofactor math, then transpose at the end.
    let a = [
        m[0][0], m[1][0], m[2][0], m[3][0],
        m[0][1], m[1][1], m[2][1], m[3][1],
        m[0][2], m[1][2], m[2][2], m[3][2],
        m[0][3], m[1][3], m[2][3], m[3][3],
    ];
    let mut inv = [0.0f32; 16];
    inv[0]  =  a[5]*a[10]*a[15] - a[5]*a[11]*a[14] - a[9]*a[6]*a[15] + a[9]*a[7]*a[14] + a[13]*a[6]*a[11] - a[13]*a[7]*a[10];
    inv[4]  = -a[4]*a[10]*a[15] + a[4]*a[11]*a[14] + a[8]*a[6]*a[15] - a[8]*a[7]*a[14] - a[12]*a[6]*a[11] + a[12]*a[7]*a[10];
    inv[8]  =  a[4]*a[9]*a[15]  - a[4]*a[11]*a[13] - a[8]*a[5]*a[15] + a[8]*a[7]*a[13] + a[12]*a[5]*a[11] - a[12]*a[7]*a[9];
    inv[12] = -a[4]*a[9]*a[14]  + a[4]*a[10]*a[13] + a[8]*a[5]*a[14] - a[8]*a[6]*a[13] - a[12]*a[5]*a[10] + a[12]*a[6]*a[9];

    inv[1]  = -a[1]*a[10]*a[15] + a[1]*a[11]*a[14] + a[9]*a[2]*a[15] - a[9]*a[3]*a[14] - a[13]*a[2]*a[11] + a[13]*a[3]*a[10];
    inv[5]  =  a[0]*a[10]*a[15] - a[0]*a[11]*a[14] - a[8]*a[2]*a[15] + a[8]*a[3]*a[14] + a[12]*a[2]*a[11] - a[12]*a[3]*a[10];
    inv[9]  = -a[0]*a[9]*a[15]  + a[0]*a[11]*a[13] + a[8]*a[1]*a[15] - a[8]*a[3]*a[13] - a[12]*a[1]*a[11] + a[12]*a[3]*a[9];
    inv[13] =  a[0]*a[9]*a[14]  - a[0]*a[10]*a[13] - a[8]*a[1]*a[14] + a[8]*a[2]*a[13] + a[12]*a[1]*a[10] - a[12]*a[2]*a[9];

    inv[2]  =  a[1]*a[6]*a[15]  - a[1]*a[7]*a[14]  - a[5]*a[2]*a[15] + a[5]*a[3]*a[14] + a[13]*a[2]*a[7]  - a[13]*a[3]*a[6];
    inv[6]  = -a[0]*a[6]*a[15]  + a[0]*a[7]*a[14]  + a[4]*a[2]*a[15] - a[4]*a[3]*a[14] - a[12]*a[2]*a[7]  + a[12]*a[3]*a[6];
    inv[10] =  a[0]*a[5]*a[15]  - a[0]*a[7]*a[13]  - a[4]*a[1]*a[15] + a[4]*a[3]*a[13] + a[12]*a[1]*a[7]  - a[12]*a[3]*a[5];
    inv[14] = -a[0]*a[5]*a[14]  + a[0]*a[6]*a[13]  + a[4]*a[1]*a[14] - a[4]*a[2]*a[13] - a[12]*a[1]*a[6]  + a[12]*a[2]*a[5];

    inv[3]  = -a[1]*a[6]*a[11]  + a[1]*a[7]*a[10]  + a[5]*a[2]*a[11] - a[5]*a[3]*a[10] - a[9]*a[2]*a[7]   + a[9]*a[3]*a[6];
    inv[7]  =  a[0]*a[6]*a[11]  - a[0]*a[7]*a[10]  - a[4]*a[2]*a[11] + a[4]*a[3]*a[10] + a[8]*a[2]*a[7]   - a[8]*a[3]*a[6];
    inv[11] = -a[0]*a[5]*a[11]  + a[0]*a[7]*a[9]   + a[4]*a[1]*a[11] - a[4]*a[3]*a[9]  - a[8]*a[1]*a[7]   + a[8]*a[3]*a[5];
    inv[15] =  a[0]*a[5]*a[10]  - a[0]*a[6]*a[9]   - a[4]*a[1]*a[10] + a[4]*a[2]*a[9]  + a[8]*a[1]*a[6]   - a[8]*a[2]*a[5];

    let det = a[0]*inv[0] + a[1]*inv[4] + a[2]*inv[8] + a[3]*inv[12];
    if det.abs() < 1e-12 {
        return [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
    }
    let inv_det = 1.0 / det;
    for v in inv.iter_mut() { *v *= inv_det; }
    // Convert back to column-major: out[col][row] = inv[row*4 + col].
    // Bone TRS produce affine matrices: bottom row is exactly [0,0,0,1].
    // Float precision in the cofactor inversion drifts that, so we snap it
    // — required by the glTF validator (ACCESSOR_INVALID_IBM otherwise).
    [
        [inv[0],  inv[4],  inv[8],  0.0],
        [inv[1],  inv[5],  inv[9],  0.0],
        [inv[2],  inv[6],  inv[10], 0.0],
        [inv[3],  inv[7],  inv[11], 1.0],
    ]
}

#[inline]
fn write_u32(w: &mut impl Write, v: u32) -> Result<()> {
    w.write_all(&v.to_le_bytes()).map_err(Into::into)
}

/// MADD material texture slot, derived from the filename suffix.
/// MADD has no explicit diff/norm/emis fields, so we route by HotS naming
/// convention: `..._diff.dds`, `..._norm.dds`, `..._emis.dds`, `..._ao.dds`.
/// `_spec` is ignored for now (no direct PBR analogue; we don't use the
/// specular/glossiness extension).
#[derive(Debug, Clone, Copy)]
enum MaddSlot { Diff, Norm, Emis, Ao }

fn slot_from_filename(path: &str) -> Option<MaddSlot> {
    // Lowercase the file stem (no extension, no path).
    let stem = std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if stem.ends_with("_diff") { Some(MaddSlot::Diff) }
    else if stem.ends_with("_norm") { Some(MaddSlot::Norm) }
    else if stem.ends_with("_emis") || stem.ends_with("_emis1") || stem.ends_with("_emis2") { Some(MaddSlot::Emis) }
    else if stem.ends_with("_ao")   { Some(MaddSlot::Ao)   }
    else { None }
}
