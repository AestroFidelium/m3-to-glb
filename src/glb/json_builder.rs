//! glTF 2.0 JSON manifest builder.
//!
//! Deliberately serde-free — plain string concatenation for speed and zero
//! dependencies. Every string goes through [`json::string`] and every float
//! through [`json::num`], so names and values taken from the file cannot break
//! the JSON.

use crate::json;
use std::fmt::Write as _;

// ─── Intermediate data structures ────────────────────────────────────────────

/// `componentType` values.
pub const UNSIGNED_BYTE: u32 = 5121;
/// `componentType` values.
pub const UNSIGNED_SHORT: u32 = 5123;
/// `componentType` values.
pub const UNSIGNED_INT: u32 = 5125;
/// `componentType` values.
pub const FLOAT: u32 = 5126;
/// `bufferView.target` for vertex attributes.
pub const ARRAY_BUFFER: u32 = 34962;
/// `bufferView.target` for index data.
pub const ELEMENT_ARRAY_BUFFER: u32 = 34963;

pub struct Accessor {
    pub buffer_view:    usize,
    pub byte_offset:    usize,
    pub component_type: u32,
    pub count:          usize,
    /// `"SCALAR"`, `"VEC2"`, `"VEC3"`, `"VEC4"` or `"MAT4"`.
    pub element_type:   &'static str,
    pub normalized:     bool,
    pub min:            Option<Vec<f64>>,
    pub max:            Option<Vec<f64>>,
}

pub struct BufferView {
    pub offset: usize,
    pub length: usize,
    pub target: Option<u32>,
}

pub struct Primitive {
    pub position_accessor: usize,
    pub normal_accessor:   usize,
    // TANGENT is only emitted when the material has a normal texture, and
    // TEXCOORD_0 only when the material samples some texture — otherwise the
    // glTF validator flags them as UNUSED (tangent/object).
    pub tangent_accessor:  Option<usize>,
    pub texcoord_accessor: Option<usize>,
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
    pub base_color_factor:  [f32; 4],
    pub normal_texture:     Option<usize>,
    pub emissive_texture:   Option<usize>,
    pub occlusion_texture:  Option<usize>,
    pub metallic_factor:    f32,
    pub roughness_factor:   f32,
    pub emissive_factor:    [f32; 3],
    pub alpha_mode:         Option<&'static str>, // "MASK" or "BLEND"; None = OPAQUE
    pub alpha_cutoff:       f32,
    pub double_sided:       bool,
}

pub struct GltfMesh {
    pub name:       String,
    pub primitives: Vec<Primitive>,
}

/// glTF scene node — may be a bone, a mesh, or just a grouping node.
#[derive(Default)]
pub struct GltfNode {
    pub name:        Option<String>,
    pub translation: Option<[f32; 3]>,
    pub rotation:    Option<[f32; 4]>, // xyzw
    pub scale:       Option<[f32; 3]>,
    pub mesh:        Option<usize>,
    pub skin:        Option<usize>,
    pub children:    Vec<usize>,
    /// Raw JSON object written verbatim as the node's `extras`. Effects ride
    /// here (see `crate::fx`); glTF treats `extras` as opaque application data,
    /// and Bevy surfaces it as a `GltfExtras` component on the node's entity.
    pub extras:      Option<String>,
}

pub struct GltfSkin {
    pub joints:                Vec<usize>,    // node indices
    pub inverse_bind_matrices: Option<usize>, // accessor index (MAT4)
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

/// Everything the manifest describes.
#[derive(Default)]
pub struct Document<'a> {
    pub meshes:       &'a [GltfMesh],
    pub accessors:    &'a [Accessor],
    pub buffer_views: &'a [BufferView],
    pub bin_length:   usize,
    pub images:       &'a [GltfImage],
    pub materials:    &'a [GltfMaterial],
    pub nodes:        &'a [GltfNode],
    pub skins:        &'a [GltfSkin],
    pub scene_roots:  &'a [usize],
    pub animations:   &'a [GltfAnimation],
    /// Wire KTX2 images through the standard `texture.source` field with
    /// `mimeType: "image/ktx2"` and do NOT declare `KHR_texture_basisu`. This is
    /// non-canonical glTF — only Bevy 0.17 accepts it (it dispatches images by
    /// MIME type via `bevy_image`, but only when the extension is absent).
    pub bevy_compat:  bool,
}

// ─── JSON builder ────────────────────────────────────────────────────────────

/// Build the full glTF JSON manifest. Every section appends itself followed by
/// a comma, and glTF forbids empty arrays, so empty sections append nothing.
pub fn build_json(doc: &Document<'_>) -> String {
    let mut j = String::with_capacity(8192);
    j.push('{');
    j.push_str(r#""asset":{"version":"2.0","generator":"m3-to-glb"},"#);
    write_extensions(&mut j, doc);
    write_scene(&mut j, doc.scene_roots, doc.nodes);
    write_skins(&mut j, doc.skins);
    write_meshes(&mut j, doc.meshes);
    write_materials(&mut j, doc.materials);
    write_textures(&mut j, doc.images, doc.bevy_compat);
    write_animations(&mut j, doc.animations);
    write_accessors(&mut j, doc.accessors);
    write_buffers(&mut j, doc.buffer_views, doc.bin_length);
    if j.ends_with(',') {
        j.pop();
    }
    j.push('}');
    j
}

/// Comma-separated `items`, each written by `f`, inside `open` … `close`.
fn list<T>(j: &mut String, open: &str, items: &[T], close: &str, mut f: impl FnMut(&mut String, &T)) {
    j.push_str(open);
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            j.push(',');
        }
        f(j, item);
    }
    j.push_str(close);
}

fn nums(j: &mut String, vs: &[f32]) {
    list(j, "[", vs, "]", |j, v| j.push_str(&json::num(*v)));
}

/// `KHR_texture_basisu` is required whenever any image is KTX2 — engines that
/// don't support the extension cannot read these textures. In `bevy_compat`
/// mode the declaration is deliberately left out.
fn write_extensions(j: &mut String, doc: &Document<'_>) {
    let uses_basisu = doc.images.iter().any(|img| img.mime_type == "image/ktx2");
    if uses_basisu && !doc.bevy_compat {
        j.push_str(r#""extensionsUsed":["KHR_texture_basisu"],"#);
        j.push_str(r#""extensionsRequired":["KHR_texture_basisu"],"#);
    }
}

/// A model with nothing to show (every region hidden, no skeleton) declares no
/// scene at all: glTF forbids empty `nodes` arrays, and the `gltf` crate
/// (Bevy's loader) rejects a scene object without one.
fn write_scene(j: &mut String, roots: &[usize], nodes: &[GltfNode]) {
    if !roots.is_empty() {
        list(j, r#""scene":0,"scenes":[{"nodes":["#, roots, "]}],", |j, r| {
            let _ = write!(j, "{r}");
        });
    }
    if !nodes.is_empty() {
        list(j, r#""nodes":["#, nodes, "],", write_node);
    }
}

fn write_node(j: &mut String, n: &GltfNode) {
    let mut o = json::Obj::new();
    if let Some(ref name) = n.name {
        o.string("name", name);
    }
    if let Some(t) = n.translation {
        o.vec3("translation", t);
    }
    if let Some(r) = n.rotation {
        o.vec4("rotation", r);
    }
    if let Some(s) = n.scale {
        o.vec3("scale", s);
    }
    if let Some(m) = n.mesh {
        o.int("mesh", m as u64);
    }
    if let Some(s) = n.skin {
        o.int("skin", s as u64);
    }
    if let Some(ref extras) = n.extras {
        o.raw("extras", extras);
    }
    if !n.children.is_empty() {
        let mut c = String::new();
        list(&mut c, "[", &n.children, "]", |j, c| {
            let _ = write!(j, "{c}");
        });
        o.raw("children", &c);
    }
    j.push_str(&o.finish());
}

fn write_skins(j: &mut String, skins: &[GltfSkin]) {
    if skins.is_empty() {
        return;
    }
    list(j, r#""skins":["#, skins, "],", |j, s| {
        list(j, r#"{"joints":["#, &s.joints, "]", |j, jt| {
            let _ = write!(j, "{jt}");
        });
        if let Some(ibm) = s.inverse_bind_matrices {
            let _ = write!(j, r#","inverseBindMatrices":{ibm}"#);
        }
        j.push('}');
    });
}

fn write_meshes(j: &mut String, meshes: &[GltfMesh]) {
    if meshes.is_empty() {
        return;
    }
    list(j, r#""meshes":["#, meshes, "],", |j, mesh| {
        let open = format!(r#"{{"name":{},"primitives":["#, json::string(&mesh.name));
        list(j, &open, &mesh.primitives, "]}", write_primitive);
    });
}

fn write_primitive(j: &mut String, p: &Primitive) {
    let _ = write!(j, r#"{{"attributes":{{"POSITION":{},"NORMAL":{}"#, p.position_accessor, p.normal_accessor);
    let optional = [
        ("TANGENT", p.tangent_accessor),
        ("TEXCOORD_0", p.texcoord_accessor),
        ("JOINTS_0", p.joints_accessor),
        ("WEIGHTS_0", p.weights_accessor),
    ];
    for (name, acc) in optional {
        if let Some(acc) = acc {
            let _ = write!(j, r#","{name}":{acc}"#);
        }
    }
    let _ = write!(j, r#"}},"indices":{}"#, p.indices_accessor);
    if let Some(mat) = p.material {
        let _ = write!(j, r#","material":{mat}"#);
    }
    j.push_str(r#","mode":4}"#);
}

fn write_materials(j: &mut String, materials: &[GltfMaterial]) {
    if materials.is_empty() {
        return;
    }
    list(j, r#""materials":["#, materials, "],", write_material);
}

fn write_material(j: &mut String, mat: &GltfMaterial) {
    let _ = write!(
        j,
        r#"{{"name":{},"pbrMetallicRoughness":{{"metallicFactor":{},"roughnessFactor":{}"#,
        json::string(&mat.name),
        json::num(mat.metallic_factor),
        json::num(mat.roughness_factor),
    );
    if let Some(tex) = mat.base_color_texture {
        let _ = write!(j, r#","baseColorTexture":{{"index":{tex}}}"#);
    }
    #[expect(clippy::float_cmp, reason = "exact default: the factor is assigned, never computed")]
    let custom_color = mat.base_color_factor != [1.0; 4];
    if custom_color {
        j.push_str(r#","baseColorFactor":"#);
        nums(j, &mat.base_color_factor);
    }
    j.push('}');
    let textures = [
        ("normalTexture", mat.normal_texture),
        ("occlusionTexture", mat.occlusion_texture),
        ("emissiveTexture", mat.emissive_texture),
    ];
    for (name, tex) in textures {
        if let Some(tex) = tex {
            let _ = write!(j, r#","{name}":{{"index":{tex}}}"#);
        }
    }
    if mat.emissive_factor.iter().any(|&c| c != 0.0) {
        j.push_str(r#","emissiveFactor":"#);
        nums(j, &mat.emissive_factor);
    }
    if let Some(mode) = mat.alpha_mode {
        let _ = write!(j, r#","alphaMode":{}"#, json::string(mode));
        if mode == "MASK" {
            let _ = write!(j, r#","alphaCutoff":{}"#, json::num(mat.alpha_cutoff));
        }
    }
    if mat.double_sided {
        j.push_str(r#","doubleSided":true"#);
    }
    j.push('}');
}

fn write_textures(j: &mut String, images: &[GltfImage], bevy_compat: bool) {
    if images.is_empty() {
        return;
    }
    j.push_str(r#""samplers":[{"magFilter":9729,"minFilter":9986,"wrapS":10497,"wrapT":10497}],"#);
    let indexed: Vec<(usize, &GltfImage)> = images.iter().enumerate().collect();
    list(j, r#""textures":["#, &indexed, "],", |j, &(i, img)| {
        if img.mime_type == "image/ktx2" && !bevy_compat {
            // KHR_texture_basisu form: the `source` lives inside the extension.
            let _ = write!(j, r#"{{"sampler":0,"extensions":{{"KHR_texture_basisu":{{"source":{i}}}}}}}"#);
        } else {
            // PNG/JPEG, and the bevy_compat KTX2 path: the standard `source`;
            // Bevy picks the decoder from `image.mimeType`.
            let _ = write!(j, r#"{{"sampler":0,"source":{i}}}"#);
        }
    });
    list(j, r#""images":["#, images, "],", |j, img| {
        let _ = write!(j, r#"{{"bufferView":{},"mimeType":{}}}"#, img.buffer_view, json::string(&img.mime_type));
    });
}

fn write_animations(j: &mut String, animations: &[GltfAnimation]) {
    if animations.is_empty() {
        return;
    }
    list(j, r#""animations":["#, animations, "],", |j, anim| {
        let open = format!(r#"{{"name":{},"samplers":["#, json::string(&anim.name));
        list(j, &open, &anim.samplers, "]", |j, s| {
            let _ = write!(j, r#"{{"input":{},"output":{},"interpolation":"{}"}}"#, s.input, s.output, s.interpolation);
        });
        list(j, r#","channels":["#, &anim.channels, "]}", |j, c| {
            let _ = write!(j, r#"{{"sampler":{},"target":{{"node":{},"path":"{}"}}}}"#, c.sampler, c.target_node, c.path);
        });
    });
}

fn write_accessors(j: &mut String, accessors: &[Accessor]) {
    if accessors.is_empty() {
        return;
    }
    list(j, r#""accessors":["#, accessors, "],", |j, acc| {
        let _ = write!(
            j,
            r#"{{"bufferView":{},"componentType":{},"count":{},"type":"{}""#,
            acc.buffer_view, acc.component_type, acc.count, acc.element_type,
        );
        if acc.byte_offset > 0 {
            let _ = write!(j, r#","byteOffset":{}"#, acc.byte_offset);
        }
        if acc.normalized {
            j.push_str(r#","normalized":true"#);
        }
        for (key, bound) in [("min", &acc.min), ("max", &acc.max)] {
            if let Some(vs) = bound {
                list(j, &format!(r#","{key}":["#), vs, "]", |j, v| j.push_str(&json::num64(*v)));
            }
        }
        j.push('}');
    });
}

/// A buffer must be at least one byte long, so a model whose BIN chunk is empty
/// declares neither views nor a buffer.
fn write_buffers(j: &mut String, views: &[BufferView], bin_length: usize) {
    if !views.is_empty() {
        list(j, r#""bufferViews":["#, views, "],", |j, bv| {
            let _ = write!(j, r#"{{"buffer":0,"byteOffset":{},"byteLength":{}"#, bv.offset, bv.length);
            if let Some(target) = bv.target {
                let _ = write!(j, r#","target":{target}"#);
            }
            j.push('}');
        });
    }
    if bin_length > 0 {
        let _ = write!(j, r#""buffers":[{{"byteLength":{bin_length}}}],"#);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(doc: &Document<'_>) -> serde_json::Value {
        let text = build_json(doc);
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}: {text}"))
    }

    #[test]
    fn empty_model_declares_nothing_empty() {
        // glTF forbids empty arrays, and a scene may only list nodes that exist.
        let v = parse(&Document::default());
        assert_eq!(v["asset"]["version"], "2.0");
        for key in ["scene", "scenes", "nodes", "meshes", "accessors", "bufferViews", "buffers"] {
            assert!(v.get(key).is_none(), "{key} present: {v}");
        }
    }

    #[test]
    fn bevy_compat_omits_basisu_extension() {
        let images = [GltfImage { buffer_view: 0, mime_type: "image/ktx2".into() }];
        let views = [BufferView { offset: 0, length: 16, target: None }];
        let doc = Document { images: &images, buffer_views: &views, bin_length: 16, ..Document::default() };

        let canonical = build_json(&doc);
        assert!(canonical.contains(r#""extensionsRequired":["KHR_texture_basisu"]"#));
        assert!(canonical.contains(r#""KHR_texture_basisu":{"source":0}"#));

        let bevy = build_json(&Document { bevy_compat: true, ..doc });
        assert!(!bevy.contains("KHR_texture_basisu"));
        assert!(!bevy.contains("extensionsRequired"));
        assert!(bevy.contains(r#""sampler":0,"source":0"#));
        assert!(bevy.contains(r#""mimeType":"image/ktx2""#));
    }

    #[test]
    fn nodes_serialize_every_field() {
        let nodes = [
            GltfNode {
                name: Some("a\"b".into()),
                translation: Some([1.0, f32::NAN, 0.5]),
                rotation: Some([0.0, 0.0, 0.0, 1.0]),
                scale: Some([1.0; 3]),
                mesh: Some(0),
                skin: Some(0),
                children: vec![1],
                extras: Some(r#"{"k":1}"#.into()),
            },
            GltfNode::default(),
        ];
        let v = parse(&Document { nodes: &nodes, scene_roots: &[0], ..Document::default() });
        let n = &v["nodes"][0];
        assert_eq!(n["name"], "a\"b");
        assert_eq!(n["translation"][1], 0, "NaN is written as 0");
        assert_eq!(n["children"][0], 1);
        assert_eq!(n["extras"]["k"], 1);
        assert_eq!(v["nodes"][1], serde_json::json!({}));
    }
}
