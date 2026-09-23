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

mod animation;
mod buffers;
mod json_builder;
mod ktx2;
mod materials;
mod scene;

use crate::assets::TextureCache;
use crate::fx::{self, curves::FxCurves};
use crate::m3::reader::M3File;
use crate::processor::MeshDataSoA;
use crate::processor::anim;
use anyhow::Result;
use buffers::{Buffers, Images};
use json_builder::{
    ARRAY_BUFFER, Accessor, ELEMENT_ARRAY_BUFFER, FLOAT, GltfMesh, GltfSkin, Primitive, UNSIGNED_BYTE,
    UNSIGNED_INT, UNSIGNED_SHORT,
};
use materials::{MaterialTable, Materials};
use scene::Scene;
use tracing::debug;

/// Per-conversion options that affect how textures and geometry are packed.
#[derive(Debug, Clone, Copy, Default)]
pub struct PackOptions {
    /// Transcode every embedded texture to KTX2 (ETC1S for color, UASTC+Zstd
    /// for data) and emit the `KHR_texture_basisu` glTF extension. Requires
    /// `toktx` on PATH.
    pub ktx2: bool,
    /// Non-canonical workaround for Bevy 0.17: reference KTX2 images via
    /// `texture.source` + `mimeType:"image/ktx2"` and omit the
    /// `KHR_texture_basisu` extension declaration. Implies `ktx2`.
    pub bevy_compat: bool,
    /// If non-zero, downscale every embedded texture so its largest
    /// dimension is at most this many pixels (aspect-preserving Lanczos3).
    pub max_tex_size: u32,
    /// Export particle systems, lights and projections as effect nodes carrying
    /// their parameters in glTF `extras` (see [`crate::fx`]). On by default;
    /// `--no-fx` turns it off.
    pub fx: bool,
}

// ─── Magic & type constants ──────────────────────────────────────────────────
const GLB_MAGIC:       u32 = 0x4654_6C67; // "glTF"
const GLB_VERSION:     u32 = 2;
const CHUNK_TYPE_JSON: u32 = 0x4E4F_534A; // "JSON"
const CHUNK_TYPE_BIN:  u32 = 0x004E_4942; // "BIN\0"

/// Assemble the GLB in memory.
///
/// `anim_sources` are the companion `.m3a` files (the base model is always
/// consulted too). The result is a complete `glTF` binary: header, JSON chunk
/// and — when anything needs it — a BIN chunk.
///
/// # Errors
///
/// Fails when the model is too malformed to lay out, or when the assembled
/// file would exceed the 4 GiB a GLB header can describe.
pub fn pack(
    meshes:       &[MeshDataSoA],
    textures:     &TextureCache,
    m3:           &M3File<'_>,
    anim_sources: &[&M3File<'_>],
    options:      PackOptions,
) -> Result<Vec<u8>> {
    pack_counted(meshes, textures, m3, anim_sources, options).map(|(glb, _)| glb)
}

/// [`pack`], also returning how many images were embedded.
pub(crate) fn pack_counted(
    meshes:       &[MeshDataSoA],
    textures:     &TextureCache,
    m3:           &M3File<'_>,
    anim_sources: &[&M3File<'_>],
    options:      PackOptions,
) -> Result<(Vec<u8>, usize)> {
    let (json_bytes, bin_bytes, images) = build_glb_content(meshes, textures, m3, anim_sources, options);
    Ok((assemble(&json_bytes, &bin_bytes)?, images))
}

/// Frame a JSON manifest and a binary buffer as a GLB file.
fn assemble(json: &[u8], bin: &[u8]) -> Result<Vec<u8>> {
    // JSON pads with spaces, BIN with zeros — both to a 4-byte boundary.
    let json_len = json.len().next_multiple_of(4);
    let bin_len  = bin.len().next_multiple_of(4);
    // The BIN chunk is omitted entirely when there is nothing to put in it — an
    // effect-only model can consist of nodes alone, and the JSON then declares
    // no buffer either.
    let bin_chunk = if bin_len == 0 { 0 } else { 8 + bin_len };
    let total = 12 + 8 + json_len + bin_chunk;
    let total_u32 = u32::try_from(total)
        .map_err(|_| anyhow::anyhow!("GLB would be {total} bytes — over the 4 GiB format limit"))?;

    debug!("GLB: JSON {}B, BIN {}B, total {}B", json_len, bin_len, total);

    let mut out = Vec::with_capacity(total);
    // Every length below is at most `total`, which was just checked to fit.
    #[expect(clippy::cast_possible_truncation, reason = "bounded by `total`, checked above")]
    let len32 = |n: usize| n as u32;
    out.extend_from_slice(&GLB_MAGIC.to_le_bytes());
    out.extend_from_slice(&GLB_VERSION.to_le_bytes());
    out.extend_from_slice(&total_u32.to_le_bytes());

    out.extend_from_slice(&len32(json_len).to_le_bytes());
    out.extend_from_slice(&CHUNK_TYPE_JSON.to_le_bytes());
    out.extend_from_slice(json);
    out.resize(out.len() + (json_len - json.len()), b' ');

    if bin_chunk != 0 {
        out.extend_from_slice(&len32(bin_len).to_le_bytes());
        out.extend_from_slice(&CHUNK_TYPE_BIN.to_le_bytes());
        out.extend_from_slice(bin);
        out.resize(out.len() + (bin_len - bin.len()), 0);
    }
    debug_assert_eq!(out.len(), total);
    Ok(out)
}

/// Convert the meshes into glTF meshes: one accessor per vertex attribute,
/// one index accessor and primitive per region. Returns the meshes and, for
/// each, whether it is skinned.
fn write_meshes(
    bufs: &mut Buffers,
    meshes: &[MeshDataSoA],
    materials: &MaterialTable,
) -> (Vec<GltfMesh>, Vec<bool>) {
    let mut out = Vec::new();
    let mut skinned = Vec::new();
    for (mesh_idx, mesh) in meshes.iter().enumerate() {
        // A mesh is only worth emitting when some region contributes
        // triangles: glTF accessors must have `count >= 1`, and a primitive
        // with nothing to draw is noise.
        if mesh.vertex_count() == 0 || !mesh.region_primitives.iter().any(|rp| rp.index_count > 0) {
            continue;
        }
        out.push(write_mesh(bufs, mesh, mesh_idx, materials));
        skinned.push(mesh.has_skin);
    }
    (out, skinned)
}

fn write_mesh(bufs: &mut Buffers, mesh: &MeshDataSoA, index: usize, materials: &MaterialTable) -> GltfMesh {
    let n = mesh.vertex_count();
    let target = Some(ARRAY_BUFFER);
    let position = {
        let buffer_view = bufs.view(&mesh.positions_as_bytes(), target);
        bufs.accessor(Accessor {
            buffer_view,
            byte_offset: 0,
            component_type: FLOAT,
            count: n,
            element_type: "VEC3",
            normalized: false,
            min: Some(mesh.aabb_min.iter().map(|&v| f64::from(v)).collect()),
            max: Some(mesh.aabb_max.iter().map(|&v| f64::from(v)).collect()),
        })
    };
    let normal = bufs.attribute(&mesh.normals_as_bytes(), target, FLOAT, n, "VEC3");
    let tangent = bufs.attribute(&mesh.tangents_as_bytes(), target, FLOAT, n, "VEC4");
    let texcoord = bufs.attribute(&mesh.uvs_as_bytes(), target, FLOAT, n, "VEC2");
    let (joints, weights) = if mesh.has_skin {
        let joints = bufs.attribute(mesh.joints_as_bytes(), target, UNSIGNED_SHORT, n, "VEC4");
        let weights = bufs.attribute(mesh.weights_as_bytes(), target, UNSIGNED_BYTE, n, "VEC4");
        // WEIGHTS_0 as UNSIGNED_BYTE must be normalized: 255 means 1.0.
        bufs.accessors[weights].normalized = true;
        (Some(joints), Some(weights))
    } else {
        (None, None)
    };

    let index_view = bufs.view(bytemuck::cast_slice(&mesh.indices), Some(ELEMENT_ARRAY_BUFFER));
    let primitives = mesh
        .region_primitives
        .iter()
        .filter(|rp| rp.index_count > 0)
        .map(|rp| {
            let indices_accessor = bufs.accessor(Accessor {
                buffer_view: index_view,
                byte_offset: rp.index_start * 4,
                component_type: UNSIGNED_INT,
                count: rp.index_count,
                element_type: "SCALAR",
                normalized: false,
                min: None,
                max: None,
            });
            let material = materials.resolve(rp.material_index);
            let (tangent_accessor, texcoord_accessor) =
                materials::prim_attr_accessors(material, &materials.json, tangent, texcoord);
            Primitive {
                position_accessor: position,
                normal_accessor: normal,
                tangent_accessor,
                texcoord_accessor,
                indices_accessor,
                material,
                joints_accessor: joints,
                weights_accessor: weights,
            }
        })
        .collect();
    GltfMesh { name: format!("mesh_{index}"), primitives }
}

/// Build the glTF JSON manifest and the binary buffer.
///
/// Effects are collected first because they change what the rest of the file
/// needs: most ability effects have no geometry at all, and bones — with their
/// animations — are otherwise emitted only for skinned meshes, which would
/// leave the emitters pinned to the origin instead of riding their bones.
fn build_glb_content(
    meshes:       &[MeshDataSoA],
    textures:     &TextureCache,
    m3:           &M3File<'_>,
    anim_sources: &[&M3File<'_>],
    options:      PackOptions,
) -> (Vec<u8>, Vec<u8>, usize) {
    let fx_items = if options.fx { fx::collect(m3) } else { Vec::new() };
    // Emitter tracks are resolved only when there is an effect to attach them
    // to — the lookup walks every sequence in the file.
    let fx_curves = if fx_items.is_empty() { FxCurves::default() } else { FxCurves::build(m3) };
    debug!("effects: {} node(s)", fx_items.len());

    let mut bufs = Buffers::default();
    let mut images = Images::new(textures, options);
    let mats = Materials::new(m3);
    let material_table = mats.for_meshes(meshes, &mut bufs, &mut images);
    let (meshes_json, mesh_skinned) = write_meshes(&mut bufs, meshes, &material_table);
    let fx_materials = mats.for_effects(&fx_items, &mut bufs, &mut images);

    // Bones are needed by skinned meshes, by effects (an emitter node is a
    // child of its bone, whose animation moves it) and by attachment points
    // (which ride bone nodes).
    let any_skinned = mesh_skinned.iter().any(|&s| s);
    let want_bones = any_skinned || !fx_items.is_empty() || m3.has_attachment_points();
    let bones = if want_bones { m3.bones().unwrap_or_default() } else { Vec::new() };

    let mut scene = Scene::with_bones(m3, &bones);
    scene.add_effects(&fx_items, &fx_materials, &fx_curves);
    let skins: Vec<GltfSkin> =
        if any_skinned && !bones.is_empty() { vec![scene::skin(&mut bufs, &bones)] } else { Vec::new() };
    scene.add_meshes(&mesh_skinned, (!skins.is_empty()).then_some(0));
    let scene_roots = scene.roots();

    // Clips come from the model itself and every companion `.m3a`; bone `i`
    // is node `i`.
    let clips = if bones.is_empty() {
        Vec::new()
    } else {
        let sources: Vec<&M3File<'_>> = std::iter::once(m3).chain(anim_sources.iter().copied()).collect();
        anim::build_animations(m3, &sources, 0).unwrap_or_default()
    };
    let animations = animation::animations(&mut bufs, &clips);

    let json = json_builder::build_json(&json_builder::Document {
        meshes:       &meshes_json,
        accessors:    &bufs.accessors,
        buffer_views: &bufs.views,
        bin_length:   bufs.bin.len(),
        images:       &images.json,
        materials:    &material_table.json,
        nodes:        &scene.nodes,
        skins:        &skins,
        scene_roots:  &scene_roots,
        animations:   &animations,
        bevy_compat:  options.bevy_compat,
    });
    (json.into_bytes(), bufs.bin, images.json.len())
}
