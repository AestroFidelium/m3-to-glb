//! glTF 2.0 JSON manifest builder.
//!
//! Deliberately serde-free — plain string concatenation for speed and zero
//! dependencies. The JSON is minimal but valid per the glTF 2.0 spec.

use std::fmt::Write as FmtWrite;

// ─── Intermediate data structures ────────────────────────────────────────────

pub struct Accessor {
    pub buffer_view:    usize,
    pub byte_offset:    usize,
    pub component_type: u32,    // 5121=UBYTE, 5123=USHORT, 5125=UINT, 5126=FLOAT
    pub count:          usize,
    pub accessor_type:  String, // "VEC2"/"VEC3"/"VEC4"/"SCALAR"/"MAT4"
    pub normalized:     bool,
    pub min:            Option<Vec<f64>>,
    pub max:            Option<Vec<f64>>,
}

pub struct BufferView {
    pub offset: usize,
    pub length: usize,
    pub target: Option<u32>, // 34962=ARRAY_BUFFER, 34963=ELEMENT_ARRAY_BUFFER
}

pub struct Primitive {
    pub position_accessor: usize,
    pub normal_accessor:   usize,
    pub tangent_accessor:  usize,
    pub texcoord_accessor: usize,
    pub indices_accessor:  usize,
    pub material:          Option<usize>,
    pub joints_accessor:   Option<usize>,
    pub weights_accessor:  Option<usize>,
}

pub struct GltfImage {
    pub buffer_view: usize,
    pub mime_type:   String,
}

pub struct GltfMaterial {
    pub name:               String,
    pub base_color_texture: Option<usize>,
    pub normal_texture:     Option<usize>,
    pub emissive_texture:   Option<usize>,
    pub occlusion_texture:  Option<usize>,
    pub metallic_factor:    f32,
    pub roughness_factor:   f32,
    pub emissive_factor:    [f32; 3],
    pub alpha_mode:         Option<String>, // "OPAQUE", "MASK", "BLEND"
    pub alpha_cutoff:       f32,
    pub double_sided:       bool,
}

pub struct GltfMesh {
    pub name:       String,
    pub primitives: Vec<Primitive>,
}

/// glTF scene node — may be a bone, a mesh, or just a grouping node.
pub struct GltfNode {
    pub name:        Option<String>,
    pub translation: Option<[f32; 3]>,
    pub rotation:    Option<[f32; 4]>, // xyzw
    pub scale:       Option<[f32; 3]>,
    pub mesh:        Option<usize>,
    pub skin:        Option<usize>,
    pub children:    Vec<usize>,
}

pub struct GltfSkin {
    pub joints:                Vec<usize>,    // node indices
    pub inverse_bind_matrices: Option<usize>, // accessor index (MAT4)
    pub skeleton:              Option<usize>, // root joint node
}

pub struct GltfAnimSampler {
    pub input:         usize, // accessor index for time
    pub output:        usize, // accessor index for values
    pub interpolation: &'static str, // "LINEAR" or "STEP"
}

pub struct GltfAnimChannel {
    pub sampler:     usize,
    pub target_node: usize,
    pub path:        &'static str, // "translation"/"rotation"/"scale"
}

pub struct GltfAnimation {
    pub name:     String,
    pub samplers: Vec<GltfAnimSampler>,
    pub channels: Vec<GltfAnimChannel>,
}

// ─── JSON builder ────────────────────────────────────────────────────────────

/// Build the full glTF JSON manifest.
///
/// `bevy_compat`: when true, KTX2 images are wired through the standard
/// `texture.source` field with `mimeType: "image/ktx2"`, and the
/// `KHR_texture_basisu` extension is NOT declared. This is non-canonical
/// glTF — only Bevy 0.17 accepts it (it dispatches images by MIME type via
/// `bevy_image`, but only when the extension is absent). Every other
/// loader / validator will reject the file. When false, the canonical
/// `KHR_texture_basisu` extension form is emitted.
#[allow(clippy::too_many_arguments)]
pub fn build_json(
    meshes:       &[GltfMesh],
    accessors:    &[Accessor],
    buffer_views: &[BufferView],
    bin_length:   usize,
    images:       &[GltfImage],
    materials:    &[GltfMaterial],
    nodes:        &[GltfNode],
    skins:        &[GltfSkin],
    scene_roots:  &[usize],
    animations:   &[GltfAnimation],
    bevy_compat:  bool,
) -> String {
    let mut j = String::with_capacity(8192);

    j.push('{');
    j.push_str(r#""asset":{"version":"2.0","generator":"m3-to-glb"},"#);

    // ── glTF extensions ──────────────────────────────────────────────────────
    // KHR_texture_basisu is required whenever any image is KTX2 — engines
    // that don't support the extension cannot read these textures, so we
    // also list it under `extensionsRequired`. In bevy_compat mode we
    // deliberately omit the declaration so Bevy 0.17 falls through to its
    // MIME-based `bevy_image` decoder.
    let uses_basisu = images.iter().any(|img| img.mime_type == "image/ktx2");
    if uses_basisu && !bevy_compat {
        j.push_str(r#""extensionsUsed":["KHR_texture_basisu"],"#);
        j.push_str(r#""extensionsRequired":["KHR_texture_basisu"],"#);
    }

    // ── scene & nodes ─────────────────────────────────────────────────────────
    j.push_str(r#""scene":0,"scenes":[{"nodes":["#);
    for (i, root) in scene_roots.iter().enumerate() {
        if i > 0 { j.push(','); }
        write!(j, "{}", root).unwrap();
    }
    j.push_str("]}],");

    // ── nodes ─────────────────────────────────────────────────────────────────
    j.push_str(r#""nodes":["#);
    for (i, n) in nodes.iter().enumerate() {
        if i > 0 { j.push(','); }
        write_node(&mut j, n);
    }
    j.push_str("],");

    // ── skins ─────────────────────────────────────────────────────────────────
    if !skins.is_empty() {
        j.push_str(r#""skins":["#);
        for (i, s) in skins.iter().enumerate() {
            if i > 0 { j.push(','); }
            j.push('{');
            j.push_str(r#""joints":["#);
            for (k, jt) in s.joints.iter().enumerate() {
                if k > 0 { j.push(','); }
                write!(j, "{}", jt).unwrap();
            }
            j.push(']');
            if let Some(ibm) = s.inverse_bind_matrices {
                write!(j, r#","inverseBindMatrices":{}"#, ibm).unwrap();
            }
            if let Some(sk) = s.skeleton {
                write!(j, r#","skeleton":{}"#, sk).unwrap();
            }
            j.push('}');
        }
        j.push_str("],");
    }

    // ── meshes ────────────────────────────────────────────────────────────────
    if !meshes.is_empty() {
        j.push_str(r#""meshes":["#);
        for (i, mesh) in meshes.iter().enumerate() {
            if i > 0 { j.push(','); }
            write!(j, r#"{{"name":{:?},"primitives":["#, mesh.name).unwrap();
            for (pi, prim) in mesh.primitives.iter().enumerate() {
                if pi > 0 { j.push(','); }
                j.push('{');
                write!(
                    j,
                    r#""attributes":{{"POSITION":{},"NORMAL":{},"TANGENT":{},"TEXCOORD_0":{}"#,
                    prim.position_accessor,
                    prim.normal_accessor,
                    prim.tangent_accessor,
                    prim.texcoord_accessor,
                ).unwrap();
                if let Some(j_acc) = prim.joints_accessor {
                    write!(j, r#","JOINTS_0":{}"#, j_acc).unwrap();
                }
                if let Some(w_acc) = prim.weights_accessor {
                    write!(j, r#","WEIGHTS_0":{}"#, w_acc).unwrap();
                }
                j.push('}');
                write!(j, r#","indices":{}"#, prim.indices_accessor).unwrap();
                if let Some(mat) = prim.material {
                    write!(j, r#","material":{}"#, mat).unwrap();
                }
                j.push_str(r#","mode":4"#);
                j.push('}');
            }
            j.push_str("]}");
        }
        j.push_str("],");
    }

    // ── materials ─────────────────────────────────────────────────────────────
    if !materials.is_empty() {
        j.push_str(r#""materials":["#);
        for (i, mat) in materials.iter().enumerate() {
            if i > 0 { j.push(','); }
            j.push('{');
            write!(j, r#""name":{:?},"#, mat.name).unwrap();
            j.push_str(r#""pbrMetallicRoughness":{"#);
            write!(j, r#""metallicFactor":{},"roughnessFactor":{}"#,
                format_f32(mat.metallic_factor), format_f32(mat.roughness_factor)).unwrap();
            if let Some(tex_idx) = mat.base_color_texture {
                write!(j, r#","baseColorTexture":{{"index":{}}}"#, tex_idx).unwrap();
            }
            j.push('}');
            if let Some(tex_idx) = mat.normal_texture {
                write!(j, r#","normalTexture":{{"index":{}}}"#, tex_idx).unwrap();
            }
            if let Some(tex_idx) = mat.occlusion_texture {
                write!(j, r#","occlusionTexture":{{"index":{}}}"#, tex_idx).unwrap();
            }
            if let Some(tex_idx) = mat.emissive_texture {
                write!(j, r#","emissiveTexture":{{"index":{}}}"#, tex_idx).unwrap();
                write!(j, r#","emissiveFactor":[{},{},{}]"#,
                    format_f32(mat.emissive_factor[0]),
                    format_f32(mat.emissive_factor[1]),
                    format_f32(mat.emissive_factor[2])).unwrap();
            }
            if let Some(ref mode) = mat.alpha_mode {
                write!(j, r#","alphaMode":{:?}"#, mode).unwrap();
                if mode == "MASK" {
                    write!(j, r#","alphaCutoff":{}"#, format_f32(mat.alpha_cutoff)).unwrap();
                }
            }
            if mat.double_sided {
                j.push_str(r#","doubleSided":true"#);
            }
            j.push('}');
        }
        j.push_str("],");
    }

    // ── textures ──────────────────────────────────────────────────────────────
    if !images.is_empty() {
        j.push_str(r#""samplers":[{"magFilter":9729,"minFilter":9986,"wrapS":10497,"wrapT":10497}],"#);
        j.push_str(r#""textures":["#);
        for (i, img) in images.iter().enumerate() {
            if i > 0 { j.push(','); }
            if img.mime_type == "image/ktx2" && !bevy_compat {
                // KHR_texture_basisu form: the `source` lives inside the
                // extension. Top-level `source` is omitted.
                write!(
                    j,
                    r#"{{"sampler":0,"extensions":{{"KHR_texture_basisu":{{"source":{}}}}}}}"#,
                    i
                ).unwrap();
            } else {
                // Canonical PNG/JPEG path AND the bevy_compat KTX2 path:
                // standard top-level `source`. Bevy's loader picks the
                // decoder via `image.mimeType`.
                write!(j, r#"{{"sampler":0,"source":{}}}"#, i).unwrap();
            }
        }
        j.push_str("],");
        j.push_str(r#""images":["#);
        for (i, img) in images.iter().enumerate() {
            if i > 0 { j.push(','); }
            write!(j, r#"{{"bufferView":{},"mimeType":{:?}}}"#, img.buffer_view, img.mime_type).unwrap();
        }
        j.push_str("],");
    }

    // ── animations ────────────────────────────────────────────────────────────
    if !animations.is_empty() {
        j.push_str(r#""animations":["#);
        for (i, anim) in animations.iter().enumerate() {
            if i > 0 { j.push(','); }
            j.push('{');
            write!(j, r#""name":{:?},"samplers":["#, anim.name).unwrap();
            for (si, samp) in anim.samplers.iter().enumerate() {
                if si > 0 { j.push(','); }
                write!(
                    j,
                    r#"{{"input":{},"output":{},"interpolation":{:?}}}"#,
                    samp.input, samp.output, samp.interpolation,
                ).unwrap();
            }
            j.push_str(r#"],"channels":["#);
            for (ci, ch) in anim.channels.iter().enumerate() {
                if ci > 0 { j.push(','); }
                write!(
                    j,
                    r#"{{"sampler":{},"target":{{"node":{},"path":{:?}}}}}"#,
                    ch.sampler, ch.target_node, ch.path,
                ).unwrap();
            }
            j.push(']');
            j.push('}');
        }
        j.push_str("],");
    }

    // ── accessors ─────────────────────────────────────────────────────────────
    j.push_str(r#""accessors":["#);
    for (i, acc) in accessors.iter().enumerate() {
        if i > 0 { j.push(','); }
        j.push('{');
        write!(
            j,
            r#""bufferView":{},"componentType":{},"count":{},"type":{:?}"#,
            acc.buffer_view, acc.component_type, acc.count, acc.accessor_type,
        ).unwrap();
        if acc.byte_offset > 0 {
            write!(j, r#","byteOffset":{}"#, acc.byte_offset).unwrap();
        }
        if acc.normalized {
            j.push_str(r#","normalized":true"#);
        }
        if let Some(ref mn) = acc.min {
            j.push_str(r#","min":["#);
            for (k, v) in mn.iter().enumerate() {
                if k > 0 { j.push(','); }
                write!(j, "{}", format_f64(*v)).unwrap();
            }
            j.push(']');
        }
        if let Some(ref mx) = acc.max {
            j.push_str(r#","max":["#);
            for (k, v) in mx.iter().enumerate() {
                if k > 0 { j.push(','); }
                write!(j, "{}", format_f64(*v)).unwrap();
            }
            j.push(']');
        }
        j.push('}');
    }
    j.push_str("],");

    // ── bufferViews ───────────────────────────────────────────────────────────
    j.push_str(r#""bufferViews":["#);
    for (i, bv) in buffer_views.iter().enumerate() {
        if i > 0 { j.push(','); }
        j.push('{');
        write!(j, r#""buffer":0,"byteOffset":{},"byteLength":{}"#, bv.offset, bv.length).unwrap();
        if let Some(target) = bv.target {
            write!(j, r#","target":{}"#, target).unwrap();
        }
        j.push('}');
    }
    j.push_str("],");

    // ── buffers ───────────────────────────────────────────────────────────────
    write!(j, r#""buffers":[{{"byteLength":{}}}]"#, bin_length).unwrap();

    j.push('}');
    j
}

fn write_node(j: &mut String, n: &GltfNode) {
    j.push('{');
    let mut first = true;
    macro_rules! sep { () => { if !first { j.push(','); } first = false; }; }

    if let Some(ref name) = n.name {
        sep!();
        write!(j, r#""name":{:?}"#, name).unwrap();
    }
    if let Some(t) = n.translation {
        sep!();
        write!(j, r#""translation":[{},{},{}]"#,
            format_f32(t[0]), format_f32(t[1]), format_f32(t[2])).unwrap();
    }
    if let Some(r) = n.rotation {
        sep!();
        write!(j, r#""rotation":[{},{},{},{}]"#,
            format_f32(r[0]), format_f32(r[1]), format_f32(r[2]), format_f32(r[3])).unwrap();
    }
    if let Some(s) = n.scale {
        sep!();
        write!(j, r#""scale":[{},{},{}]"#,
            format_f32(s[0]), format_f32(s[1]), format_f32(s[2])).unwrap();
    }
    if let Some(m) = n.mesh {
        sep!();
        write!(j, r#""mesh":{}"#, m).unwrap();
    }
    if let Some(s) = n.skin {
        sep!();
        write!(j, r#""skin":{}"#, s).unwrap();
    }
    if !n.children.is_empty() {
        sep!();
        j.push_str(r#""children":["#);
        for (i, c) in n.children.iter().enumerate() {
            if i > 0 { j.push(','); }
            write!(j, "{}", c).unwrap();
        }
        j.push(']');
    }
    j.push('}');
}

/// Compact f32 formatting: `1.0`, `-3.0`, `1.5`.
fn format_f32(v: f32) -> String {
    if v.is_finite() && v == v.trunc() && v.abs() < 1e15 {
        format!("{:.1}", v)
    } else {
        format!("{}", v)
    }
}

fn format_f64(v: f64) -> String {
    if v.is_finite() && v == v.trunc() && v.abs() < 1e15 {
        format!("{:.1}", v)
    } else {
        format!("{}", v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_minimal_json_valid() {
        let json = build_json(&[], &[], &[], 0, &[], &[], &[], &[], &[], &[], false);
        assert!(json.contains(r#""asset""#));
        assert!(json.contains(r#""version":"2.0""#));
        assert!(json.contains(r#""scene":0"#));
    }

    #[test]
    fn test_bevy_compat_omits_basisu_extension() {
        let images = vec![GltfImage { buffer_view: 0, mime_type: "image/ktx2".into() }];
        let bv = vec![BufferView { offset: 0, length: 16, target: None }];

        let canonical = build_json(&[], &[], &bv, 16, &images, &[], &[], &[], &[], &[], false);
        assert!(canonical.contains(r#""extensionsRequired":["KHR_texture_basisu"]"#));
        assert!(canonical.contains(r#""KHR_texture_basisu":{"source":0}"#));

        let bevy = build_json(&[], &[], &bv, 16, &images, &[], &[], &[], &[], &[], true);
        assert!(!bevy.contains("KHR_texture_basisu"));
        assert!(!bevy.contains("extensionsRequired"));
        assert!(bevy.contains(r#""sampler":0,"source":0"#));
        assert!(bevy.contains(r#""mimeType":"image/ktx2""#));
    }

    #[test]
    fn test_format_f32_compact() {
        assert_eq!(format_f32(1.0),  "1.0");
        assert_eq!(format_f32(-3.0), "-3.0");
        assert_eq!(format_f32(1.5),  "1.5");
    }
}
