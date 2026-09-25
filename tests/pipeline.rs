//! End-to-end conversions of synthetic models.
//!
//! Each test describes a model with [`common::ModelSpec`], converts it through
//! the public [`Converter`] API, runs the result through the GLB oracle and then
//! asserts on what the glTF says — material modes, skin layout, animation
//! channels, effect nodes. Nothing here needs a Blizzard asset.

#![allow(clippy::float_cmp, reason = "asserting values the converter copies through or writes as constants")]

mod common;

use bytemuck::Zeroable;
use common::*;
use m3_to_glb::m3::structures::Atvl;
use m3_to_glb::{Converter, Error, PackOptions, TextureCache};
use serde_json::Value;

fn convert(spec: &ModelSpec) -> GlbSummary {
    convert_with(spec, &Converter::new())
}

fn convert_with(spec: &ModelSpec, c: &Converter<'_>) -> GlbSummary {
    let glb = c.convert(&spec.build()).expect("conversion failed");
    check_glb(&glb.bytes).unwrap_or_else(|e| panic!("invalid GLB: {e}"))
}

/// A JSON index as `usize`.
fn at(v: &Value) -> usize {
    usize::try_from(v.as_u64().unwrap_or_else(|| panic!("not an index: {v}"))).unwrap()
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

// ─── Geometry ────────────────────────────────────────────────────────────────

#[test]
fn quad_converts_to_one_indexed_primitive() {
    let g = convert(&ModelSpec::quad());
    assert_eq!(g.count("meshes"), 1);
    let prim = &g.json["meshes"][0]["primitives"][0];
    let idx = &g.json["accessors"][at(&prim["indices"])];
    assert_eq!(idx["count"], 6);
    let pos = &g.json["accessors"][at(&prim["attributes"]["POSITION"])];
    assert_eq!(pos["count"], 4);
    // Z-up → Y-up: the quad lies in M3's XY plane, so glTF Z spans -1..0.
    assert_eq!(f(&pos["min"][2]), -1.0);
    assert_eq!(f(&pos["max"][2]), 0.0);
    assert_eq!(f(&pos["max"][1]), 0.0);
    // No texture was supplied, so nothing samples UVs.
    assert!(prim["attributes"].get("TEXCOORD_0").is_none());
}

#[test]
fn every_region_version_reads_the_same_quad() {
    for v in [1, 2, 3, 4, 5] {
        let mut spec = ModelSpec::quad();
        spec.regn_version = v;
        let g = convert(&spec);
        assert_eq!(g.count("meshes"), 1, "REGN v{v}");
    }
}

#[test]
fn old_regions_hold_absolute_indices() {
    // REGN ≤ v2 stores indices relative to the whole vertex buffer; the second
    // region's faces point at vertices 4..8 and must be rebased to 0..4.
    let mut spec = ModelSpec::quad();
    spec.regn_version = 2;
    let second: Vec<Vtx> = spec.vertices.clone();
    spec.vertices.extend(second);
    spec.faces.extend([4, 5, 6, 4, 6, 7]);
    spec.regions.push(RegionSpec { first_vertex: 4, vertex_count: 4, first_face: 6, face_count: 6, ..spec.regions[0] });
    spec.batches.push(BatchSpec { region: 1, matm: 0, bone: -1 });
    let g = convert(&spec);
    assert_eq!(g.json["meshes"][0]["primitives"].as_array().unwrap().len(), 2);
}

#[test]
fn out_of_range_triangles_are_dropped() {
    let mut spec = ModelSpec::quad();
    spec.faces = vec![0, 1, 2, 0, 2, 99];
    let g = convert(&spec);
    let prim = &g.json["meshes"][0]["primitives"][0];
    assert_eq!(g.json["accessors"][at(&prim["indices"])]["count"], 3);
}

#[test]
fn a_region_with_no_valid_triangles_emits_no_mesh() {
    let mut spec = ModelSpec::quad();
    spec.faces = vec![7, 8, 9];
    spec.regions[0].face_count = 3;
    let g = convert(&spec);
    assert_eq!(g.count("meshes"), 0);
    assert_eq!(g.count("accessors"), 0);
}

#[test]
fn non_finite_positions_and_uvs_are_zeroed() {
    let mut spec = ModelSpec::quad();
    spec.vertices[1].pos = [f32::NAN, f32::INFINITY, 1.0];
    spec.regions[0].uv_offset = f32::NAN;
    let g = convert(&spec);
    let text = g.json.to_string();
    assert!(!text.contains("NaN") && !text.contains("inf"), "{text}");
}

#[test]
fn missing_uv_normal_and_tangent_get_defaults() {
    let mut spec = ModelSpec::quad();
    spec.vertex_flags = 0x1; // position only
    let g = convert(&spec);
    assert_eq!(g.count("meshes"), 1);
}

#[test]
fn uncompressed_normal_layout_converts() {
    let mut spec = ModelSpec::quad();
    // normalf + colour + fuv0 + uv0 + normalf2 + tanf + compressed tangent.
    spec.vertex_flags = 0x1 | 0x80 | 0x200 | 0x2000 | 0x2_0000 | 0x20_0000 | 0x40_0000 | 0x100_0000;
    convert(&spec);
}

#[test]
fn zero_uv_multiply_falls_back_to_sixteen() {
    let mut spec = ModelSpec::quad();
    spec.regions[0].uv_multiply = 0.0;
    convert(&spec);
}

#[test]
fn truncated_vertex_buffer_is_an_error_not_a_panic() {
    let mut spec = ModelSpec::quad();
    spec.vertex_slack = -8;
    let err = Converter::new().convert(&spec.build()).unwrap_err();
    assert!(matches!(err, Error::Convert(_)), "{err}");
    assert!(err.to_string().contains("out of bounds"), "{err}");
}

#[test]
fn hidden_and_placeholder_regions_are_skipped() {
    for flag in [0x1, 0x2] {
        let mut spec = ModelSpec::quad();
        spec.regn_version = 4;
        spec.regions[0].flags = flag;
        assert_eq!(convert(&spec).count("meshes"), 0);
    }
}

#[test]
fn batch_toggled_off_by_an_animated_bone_is_hidden() {
    let mut spec = ModelSpec::quad();
    let mut bone = BoneSpec::named("Toggle", -1);
    bone.batching = (6, 0x1234, 0);
    spec.bones = vec![bone];
    spec.batches[0].bone = 0;
    assert_eq!(convert(&spec).count("meshes"), 0);

    spec.bones[0].batching.2 = 1; // default on
    assert_eq!(convert(&spec).count("meshes"), 1);
}

#[test]
fn a_region_without_a_batch_has_no_material_and_is_skipped() {
    let mut spec = ModelSpec::quad();
    spec.batches.clear();
    assert_eq!(convert(&spec).count("meshes"), 0);
}

#[test]
fn batches_for_missing_regions_are_ignored() {
    let mut spec = ModelSpec::quad();
    spec.batches.insert(0, BatchSpec { region: 9, matm: 0, bone: -1 });
    // A second batch on the same region does not replace the first.
    spec.batches.push(BatchSpec { region: 0, matm: 5, bone: -1 });
    assert_eq!(convert(&spec).count("materials"), 1);
}

#[test]
fn zero_vertex_region_is_skipped() {
    let mut spec = ModelSpec::quad();
    spec.regions.insert(0, RegionSpec::default());
    spec.batches[0].region = 1;
    assert_eq!(convert(&spec).count("meshes"), 1);
}

#[test]
fn effect_only_model_has_no_mesh() {
    let spec = ModelSpec { bones: vec![BoneSpec::named("Root", -1)], ..ModelSpec::default() };
    let g = convert(&spec);
    assert_eq!(g.count("meshes"), 0);
    assert!(g.bin.is_empty());
}

#[test]
fn an_empty_model_is_a_valid_empty_file() {
    let g = convert(&ModelSpec::default());
    assert_eq!(g.count("nodes"), 0);
    assert_eq!(g.count("scenes"), 0);
}

#[test]
fn division_without_regions_is_empty() {
    let spec = ModelSpec { force_division: true, ..ModelSpec::default() };
    assert_eq!(convert(&spec).count("meshes"), 0);
}

// ─── Materials ───────────────────────────────────────────────────────────────

fn material(spec: &ModelSpec) -> Value {
    convert(spec).json["materials"][0].clone()
}

#[test]
fn blend_alpha_test_and_two_sided_map_to_gltf() {
    let mut spec = ModelSpec::quad();
    spec.materials[0].blend = 2;
    assert_eq!(material(&spec)["alphaMode"], "BLEND");

    spec.materials[0].blend = 0;
    spec.materials[0].alpha_threshold = 0xFF80; // only the low byte counts
    let m = material(&spec);
    assert_eq!(m["alphaMode"], "MASK");
    assert!((f(&m["alphaCutoff"]) - 128.0 / 255.0).abs() < 1e-6);

    spec.materials[0].alpha_threshold = 0;
    spec.materials[0].flags = 0x8;
    let m = material(&spec);
    assert!(m.get("alphaMode").is_none());
    assert_eq!(m["doubleSided"], true);
}

#[test]
fn every_material_version_places_its_layers() {
    for v in [15, 16, 17, 18, 19, 20, 21] {
        let mut spec = ModelSpec::quad();
        spec.mat_version = v;
        spec.materials[0].layers = vec![("diff", LayerSpec { color: Some([0, 0, 255, 255]), ..LayerSpec::default() })];
        let m = material(&spec);
        assert_eq!(f(&m["pbrMetallicRoughness"]["baseColorFactor"][0]), 1.0, "MAT_ v{v}");
        assert_eq!(f(&m["pbrMetallicRoughness"]["baseColorFactor"][2]), 0.0, "MAT_ v{v}");
    }
}

#[test]
fn every_layer_version_reads() {
    for v in [20, 22, 23, 24, 25, 26, 99] {
        let mut spec = ModelSpec::quad();
        spec.layr_version = v;
        spec.materials[0].layers[0].1.uv_tiling = [2.0, 0.0];
        material(&spec);
    }
}

#[test]
fn colour_layers_become_factors() {
    let mut spec = ModelSpec::quad();
    spec.materials[0].layers = vec![
        ("diff", LayerSpec { color: Some([255, 0, 0, 255]), ..LayerSpec::default() }),
        ("emis1", LayerSpec { color: Some([0, 255, 0, 255]), ..LayerSpec::default() }),
    ];
    let m = material(&spec);
    let base = &m["pbrMetallicRoughness"]["baseColorFactor"];
    assert_eq!((f(&base[0]), f(&base[2])), (0.0, 1.0));
    assert_eq!(f(&m["emissiveFactor"][1]), 1.0);
}

#[test]
fn a_material_with_no_albedo_source_is_black() {
    let mut spec = ModelSpec::quad();
    spec.materials[0].layers.clear();
    let m = material(&spec);
    assert_eq!(f(&m["pbrMetallicRoughness"]["baseColorFactor"][0]), 0.0);
}

#[test]
fn madd_materials_route_textures_by_suffix() {
    for v in [1, 2, 3] {
        let mut spec = ModelSpec::quad();
        spec.materials.clear();
        spec.madd_version = v;
        spec.madds = vec![vec!["a_diff.dds".into(), "a_spec.dds".into()]];
        spec.matms = vec![(12, 0)];
        let m = material(&spec);
        assert_eq!(m["name"], "madd_0", "MADD v{v}");
    }
}

#[test]
fn madd_blend_mode_makes_the_material_transparent() {
    for v in [1, 2, 3] {
        for (blend, alpha_mode) in [(0, None), (1, Some("BLEND")), (3, Some("BLEND"))] {
            let mut spec = ModelSpec::quad();
            spec.materials.clear();
            spec.madd_version = v;
            spec.madds = vec![vec!["a_diff.dds".into()]];
            spec.madd_blends = vec![blend];
            spec.matms = vec![(12, 0)];
            let m = material(&spec);
            assert_eq!(m["alphaMode"].as_str(), alpha_mode, "MADD v{v} blend {blend}");
        }
    }
}

#[test]
fn composite_materials_fall_back_to_their_first_section() {
    let mut spec = ModelSpec::quad();
    spec.matms = vec![(3, 0), (1, 0)];
    spec.composites = vec![vec![1]];
    assert_eq!(material(&spec)["name"], "material_0");
}

#[test]
fn self_referencing_composite_terminates() {
    let mut spec = ModelSpec::quad();
    spec.matms = vec![(3, 0)];
    spec.composites = vec![vec![0]];
    assert_eq!(convert(&spec).count("meshes"), 0);
}

#[test]
fn unsupported_material_types_skip_the_region() {
    for t in [2, 4, 5, 7, 11] {
        let mut spec = ModelSpec::quad();
        spec.matms = vec![(t, 0)];
        assert_eq!(convert(&spec).count("meshes"), 0, "mat_type {t}");
    }
}

#[test]
fn material_index_past_the_table_is_dropped() {
    let mut spec = ModelSpec::quad();
    spec.matms = vec![(1, 7)];
    let g = convert(&spec);
    assert_eq!(g.count("materials"), 0);
}

// ─── Textures ────────────────────────────────────────────────────────────────

fn texture_dir(names: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("Assets").join("Textures");
    std::fs::create_dir_all(&sub).unwrap();
    for name in names {
        let img = image::RgbaImage::from_fn(8, 4, |x, y| image::Rgba([u8::try_from(x * 30).unwrap(), u8::try_from(y * 60).unwrap(), 0, 255]));
        img.save(sub.join(name)).unwrap();
    }
    std::fs::write(sub.join("notes.txt"), "not a texture").unwrap();
    dir
}

#[test]
fn textures_are_found_by_stem_and_embedded_once() {
    let dir = texture_dir(&["quad_diff.png", "quad_norm.png", "Quad_Emis.PNG", "quad_ao.png"]);
    let cache = TextureCache::build(dir.path().to_str().unwrap()).unwrap();
    assert_eq!(cache.len(), 4);
    assert!(format!("{cache:?}").contains('4'));

    let mut spec = ModelSpec::quad();
    let layer = |p: &str| LayerSpec { texture: p.into(), ..LayerSpec::default() };
    spec.materials[0].layers = vec![
        ("diff", layer("Assets\\Textures\\quad_diff.dds")),
        ("norm", layer("assets/textures/QUAD_NORM.dds")),
        ("emis1", layer("quad_emis.dds")),
        ("ao", layer("quad_ao.tga")),
    ];
    // A second material sharing the diffuse must reuse the embedded image.
    spec.materials.push(spec.materials[0].clone());
    spec.vertices.extend(spec.vertices.clone());
    spec.faces.extend([4, 5, 6]);
    spec.regions.push(RegionSpec { first_vertex: 4, vertex_count: 4, first_face: 6, face_count: 3, uv_multiply: 16.0, ..RegionSpec::default() });
    spec.batches.push(BatchSpec { region: 1, matm: 1, bone: -1 });
    spec.matms.push((1, 1));

    let converter = Converter::new().textures(&cache).max_texture_size(4);
    // Stats count what was embedded, not what the folder holds.
    assert_eq!(converter.convert(&spec.build()).unwrap().stats.textures, 4);
    let g = convert_with(&spec, &converter);
    assert_eq!(g.count("images"), 4);
    assert_eq!(g.count("materials"), 2);
    let m = &g.json["materials"][0];
    assert!(m.get("normalTexture").is_some() && m.get("occlusionTexture").is_some());
    assert_eq!(f(&m["emissiveFactor"][0]), 1.0);
    let prim = &g.json["meshes"][0]["primitives"][0]["attributes"];
    assert!(prim.get("TANGENT").is_some() && prim.get("TEXCOORD_0").is_some());
}

#[test]
fn madd_textures_embed_by_slot() {
    let dir = texture_dir(&["hero_diff.png", "hero_norm.png", "hero_emis2.png", "hero_ao.png"]);
    let cache = TextureCache::build(dir.path().to_str().unwrap()).unwrap();
    let mut spec = ModelSpec::quad();
    spec.materials.clear();
    spec.madds = vec![vec![
        "hero_diff.dds".into(),
        "hero_diff.dds".into(), // second diffuse ignored
        "hero_norm.dds".into(),
        "hero_emis2.dds".into(),
        "hero_ao.dds".into(),
        "hero_spec.dds".into(),
    ]];
    spec.matms = vec![(12, 0)];
    let g = convert_with(&spec, &Converter::new().textures(&cache));
    assert_eq!(g.count("images"), 4);
    assert_eq!(f(&g.json["materials"][0]["emissiveFactor"][0]), 1.0);
}

#[test]
fn textures_pass_through_unless_they_must_be_decoded() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("quad_diff.png"), b"definitely not a png").unwrap();
    let cache = TextureCache::build(dir.path().to_str().unwrap()).unwrap();
    // Embedded byte-for-byte when nothing asks for a decode…
    let g = convert_with(&ModelSpec::quad(), &Converter::new().textures(&cache));
    assert_eq!(g.count("images"), 1);
    // …and skipped when a resize needs the pixels and they are not an image.
    let g = convert_with(&ModelSpec::quad(), &Converter::new().textures(&cache).max_texture_size(64));
    assert_eq!(g.count("images"), 0);
}

#[test]
fn ktx2_without_toktx_falls_back_to_the_source_image() {
    let dir = texture_dir(&["quad_diff.png"]);
    let cache = TextureCache::build(dir.path().to_str().unwrap()).unwrap();
    // An empty PATH guarantees `toktx` is not found, whatever the machine has.
    let old = std::env::var_os("PATH");
    // SAFETY: tests in this binary that read PATH do not run concurrently with
    // this one — none of them shells out.
    unsafe { std::env::set_var("PATH", "") };
    let g = convert_with(&ModelSpec::quad(), &Converter::new().textures(&cache).ktx2(true));
    if let Some(p) = old {
        // SAFETY: as above.
        unsafe { std::env::set_var("PATH", p) };
    }
    assert_eq!(g.json["images"][0]["mimeType"], "image/png");
}

#[test]
fn missing_texture_directory_is_an_error() {
    let err = TextureCache::build("/definitely/not/here").unwrap_err();
    assert!(err.to_string().contains("not found"));
    let e: Error = err.into();
    assert!(matches!(e, Error::Textures(_)));
}

// ─── Skeleton & skin ─────────────────────────────────────────────────────────

#[test]
fn skinned_mesh_gets_joints_weights_and_one_skin() {
    let g = convert(&skinned_quad());
    assert_eq!(g.count("skins"), 1);
    let attrs = &g.json["meshes"][0]["primitives"][0]["attributes"];
    assert!(attrs.get("JOINTS_0").is_some() && attrs.get("WEIGHTS_0").is_some());
    // bones, armature, mesh
    assert_eq!(g.node_named("Child").unwrap()["translation"][1], 1);
    let armature = g.node_named("armature").unwrap();
    assert_eq!(armature["children"].as_array().unwrap().len(), 1);
    // The root bone carries the Z-up → Y-up bake: +Z becomes +Y.
    let root = g.node_named("Root").unwrap();
    assert!((f(&root["translation"][1]) - 1.0).abs() < 1e-6);
}

#[test]
fn two_pair_skin_layout() {
    let mut spec = skinned_quad();
    spec.vertex_flags = FLAGS_STATIC | 0x20;
    convert(&spec);
}

#[test]
fn bone_cycles_become_roots() {
    let mut spec = skinned_quad();
    spec.bones[0].parent = 1; // forward reference
    spec.bones[1].parent = 1; // self reference
    let g = convert(&spec);
    let armature = g.node_named("armature").unwrap();
    assert_eq!(armature["children"].as_array().unwrap().len(), 2);
}

#[test]
fn unnamed_bones_get_generated_names() {
    let mut spec = skinned_quad();
    spec.bones[1].name.clear();
    let g = convert(&spec);
    assert!(g.node_named("bone_1").is_some());
}

#[test]
fn names_from_the_file_are_escaped() {
    let mut spec = skinned_quad();
    spec.bones[1].name = "Bone \"Q\"\u{1}\\ \u{7}".into();
    let g = convert(&spec);
    assert!(g.node_named("Bone \"Q\"\u{1}\\ \u{7}").is_some());
}

#[test]
fn degenerate_bind_pose_still_yields_matrices() {
    let mut spec = skinned_quad();
    spec.bones[0].s = [0.0; 3]; // singular → identity inverse
    convert(&spec);
}

// ─── Animation ───────────────────────────────────────────────────────────────

#[test]
fn bone_tracks_become_gltf_animations() {
    let g = convert(&animated());
    let names: Vec<&str> = g.json["animations"].as_array().unwrap().iter().map(|a| a["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Stand_full", "Loose"]);
    let stand = &g.json["animations"][0];
    assert_eq!(stand["channels"].as_array().unwrap().len(), 4);
    let paths: Vec<&str> = stand["channels"].as_array().unwrap().iter().map(|c| c["target"]["path"].as_str().unwrap()).collect();
    assert_eq!(paths, ["translation", "rotation", "scale", "translation"]);
    // The duplicate 500 ms key keeps the later sample; 4 keys → 3 times.
    let input = &g.json["accessors"][at(&stand["samplers"][0]["input"])];
    assert_eq!(input["count"], 3);
}

#[test]
fn seqs_version_one_reads() {
    let mut spec = animated();
    spec.seqs_version = 1;
    assert_eq!(convert(&spec).count("animations"), 2);
}

#[test]
fn companion_animation_files_contribute_clips() {
    let mut base = skinned_quad();
    base.bones[0].anim_ids = [11, 0, 0];
    let m3a = ModelSpec {
        stcs: vec![StcSpec {
            name: "Walk".into(),
            tracks: vec![(11, Track::Vec3(vec![(0, [0.0; 3]), (33, [1.0; 3])]))],
            ..StcSpec::default()
        }],
        sequences: vec![SeqSpec { name: "Walk".into(), stcs: vec![0] }],
        ..ModelSpec::default()
    }
    .build();
    let glb = Converter::new().animations(&m3a).convert(&base.build()).unwrap();
    assert_eq!(glb.stats.animations, 1);
    let g = check_glb(&glb.bytes).unwrap();
    assert_eq!(g.json["animations"][0]["name"], "Walk");
}

#[test]
fn a_broken_animation_file_names_its_index() {
    let base = ModelSpec::quad().build();
    let ok = ModelSpec::default().build();
    let err = Converter::new().animations(&ok).animations(b"nope").convert(&base).unwrap_err();
    assert!(matches!(err, Error::Animation { index: 1, .. }), "{err}");
    assert!(err.to_string().starts_with("animation file #1"));
}

// ─── Effects & attachments ───────────────────────────────────────────────────

#[test]
fn effects_ride_their_bone_as_extras() {
    let g = convert(&with_effects());
    let fx = g.extras("m3fx");
    let kinds: Vec<&str> = fx.iter().map(|v| v["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["particle", "light", "decal"]);
    assert_eq!(fx[0]["blend"], "add");
    assert!(fx[0]["anim"]["Birth"]["rate"].as_array().unwrap().len() <= 48);
    assert_eq!(fx[1]["light"], "spot");
    assert_eq!(f(&fx[1]["intensity"]), 3.0);
    assert_eq!(fx[2]["blend"], "blend"); // composite → MADD base
    let bone = g.node_named("Bone_FX").unwrap();
    assert_eq!(bone["children"].as_array().unwrap().len(), 3);
    let decal = g.node_named("PROJ0_Bone_FX").unwrap();
    assert_eq!(f(&decal["translation"][2]), 2.0);
}

#[test]
fn effects_can_be_turned_off() {
    let spec = with_effects();
    let g = convert_with(&spec, &Converter::new().effects(false));
    assert!(g.extras("m3fx").is_empty());
    let g = convert_with(&spec, &Converter::new().options(PackOptions::default()));
    assert!(g.extras("m3fx").is_empty());
}

#[test]
fn older_particle_versions_are_widened_and_ancient_ones_skipped() {
    for (v, expect) in [(22, 1), (23, 1), (24, 1), (21, 0)] {
        let mut spec = with_effects();
        spec.par_version = v;
        let g = convert(&spec);
        let n = g.extras("m3fx").iter().filter(|x| x["kind"] == "particle").count();
        assert_eq!(n, expect, "PAR_ v{v}");
    }
}

#[test]
fn unsupported_light_and_projection_versions_are_skipped() {
    let mut spec = with_effects();
    spec.lite_version = 6;
    spec.proj_version = 4;
    let g = convert(&spec);
    assert_eq!(g.extras("m3fx").len(), 1);
}

#[test]
fn attachments_name_their_bone_node() {
    let mut spec = ModelSpec {
        bones: vec![BoneSpec::named("Ref_Head", -1), BoneSpec::named("Vol_Target", 0)],
        attachments: vec![
            ("Ref_Head".into(), 0),
            ("Ref_Target".into(), 1),
            ("Ref_Dupe".into(), 1),
            ("Ref_Missing".into(), 7),
            (String::new(), 0),
        ],
        ..ModelSpec::default()
    };
    let mut vol = Atvl::zeroed();
    vol.bone0 = 1;
    vol.shape = 1;
    vol.size0 = 0.5;
    let mut odd = vol;
    odd.bone0 = 0;
    odd.shape = 42;
    spec.volumes = vec![vol, odd];
    let g = convert(&spec);
    let target = &g.node_named("Vol_Target").unwrap()["extras"]["m3attach"];
    assert_eq!(target["name"], "Ref_Target");
    assert_eq!(target["volume"]["shape"], "sphere");
    assert_eq!(target["volume"]["matrix"].as_array().unwrap().len(), 16);
    let head = &g.node_named("Ref_Head").unwrap()["extras"]["m3attach"];
    assert_eq!(head["volume"]["shape"], "unknown");

    spec.att_version = 2;
    spec.atvl_version = 3;
    assert!(convert(&spec).extras("m3attach").is_empty());
}

// ─── API surface ─────────────────────────────────────────────────────────────

#[test]
fn stats_describe_the_conversion() {
    let glb = Converter::new().convert(&animated().build()).unwrap();
    assert_eq!(glb.stats.meshes, 1);
    assert_eq!(glb.stats.triangles, 2);
    assert_eq!(glb.stats.bones, 2);
    assert_eq!(glb.stats.animations, 6);
    assert_eq!(glb.stats.textures, 0);
}

#[test]
fn convert_file_round_trips_through_disk() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in.m3");
    let output = dir.path().join("out.glb");
    std::fs::write(&input, ModelSpec::quad().build()).unwrap();
    let stats = Converter::new().convert_file(&input, &output).unwrap();
    assert_eq!(stats.meshes, 1);
    check_glb(&std::fs::read(&output).unwrap()).unwrap();

    let err = Converter::new().convert_file(dir.path().join("missing.m3"), &output).unwrap_err();
    assert!(matches!(err, Error::Io { .. }));
    assert!(err.to_string().contains("missing.m3"));
    let err = Converter::new().convert_file(&input, dir.path().join("no/such/dir/out.glb")).unwrap_err();
    assert!(matches!(err, Error::Io { .. }));
}

#[test]
fn garbage_is_a_model_error() {
    let err = Converter::new().convert(b"hello world").unwrap_err();
    assert!(matches!(err, Error::Model(_)));
    assert!(err.to_string().starts_with("model:"));
    let io: Error = std::io::Error::other("x").into();
    assert!(matches!(io, Error::IoOther(_)));
}

#[test]
fn every_magic_spelling_parses() {
    for magic in [*b"43DM", *b"33DM", *b"23DM", *b"MD34", *b"MD33", *b"MD32"] {
        let spec = ModelSpec { magic, ..ModelSpec::quad() };
        let m3 = spec.build();
        let file = m3_to_glb::parse(&m3).unwrap();
        assert_eq!(file.mesh_count(), 1);
        convert(&spec);
    }
}

// ─── Edge cases found while measuring coverage ───────────────────────────────

#[test]
fn a_face_range_past_the_index_buffer_draws_nothing() {
    let mut spec = ModelSpec::quad();
    spec.regions[0].first_face = 4; // 4 + 6 > 6 indices
    assert_eq!(convert(&spec).count("meshes"), 0);
}

#[test]
fn textures_missing_from_the_index_are_left_out() {
    let dir = texture_dir(&["something_else.png"]);
    let cache = TextureCache::build(dir.path().to_str().unwrap()).unwrap();
    let g = convert_with(&ModelSpec::quad(), &Converter::new().textures(&cache));
    assert_eq!(g.count("images"), 0);
    // The diffuse *path* is set, so the base colour stays white rather than
    // the black used for a material with no albedo source at all.
    assert!(g.json["materials"][0]["pbrMetallicRoughness"].get("baseColorFactor").is_none());
    assert!(TextureCache::empty().find("x_diff.dds").is_none());
    assert!(TextureCache::empty().is_empty());
}

#[test]
fn small_textures_pass_through_a_resize_limit_untouched() {
    let dir = texture_dir(&["quad_diff.png"]);
    let cache = TextureCache::build(dir.path().to_str().unwrap()).unwrap();
    let raw = std::fs::read(dir.path().join("Assets/Textures/quad_diff.png")).unwrap();
    let g = convert_with(&ModelSpec::quad(), &Converter::new().textures(&cache).max_texture_size(64));
    let view = &g.json["bufferViews"][at(&g.json["images"][0]["bufferView"])];
    assert_eq!(at(&view["byteLength"]), raw.len());
}

#[test]
fn image_mime_types_follow_the_extension() {
    let dir = tempfile::tempdir().unwrap();
    for f in ["a.dds", "b.TGA", "c.bmp", "d.jpg", "e.jpeg", "f.PNG", "g.txt"] {
        std::fs::write(dir.path().join(f), b"").unwrap();
    }
    let cache = TextureCache::build(dir.path().to_str().unwrap()).unwrap();
    assert_eq!(cache.len(), 6, "every image extension, in any case — not .txt");
    let mime = |p: &str| cache.find_with_mime(p).map(|(_, m)| m);
    assert_eq!(mime("a.dds"), Some("image/vnd-ms.dds"));
    assert_eq!(mime("B.tga"), Some("image/x-tga"));
    assert_eq!(mime("c.dds"), Some("image/bmp"));
    assert_eq!(mime("d.dds"), Some("image/jpeg"));
    assert_eq!(mime("e"), Some("image/jpeg"));
    assert_eq!(mime("Assets\\Textures\\f.dds"), Some("image/png"));
    assert_eq!(mime("g"), None);
    assert_eq!(mime(""), None);
}

#[test]
fn emitter_curves_carry_burst_timing() {
    let mut spec = with_effects();
    // A rate that switches on 100 ms into the sequence, and a burst count
    // stored as an unsigned 16-bit track.
    spec.stcs[0].tracks[0] = (77, Track::Real(vec![(0, 0.0), (100, 0.0), (200, 5.0), (300, 0.0)]));
    // A light whose intensity curve never rises contributes no peak.
    spec.stcs[0].tracks[3] = (80, Track::Real(vec![(0, 0.0), (10, 0.0)]));
    spec.particles[0].emit_count.header.id = 81;
    // A textureless effect material whose colour lives in the diffuse layer.
    spec.materials[0].layers.push(("diff", LayerSpec { color: Some([9, 9, 9, 255]), ..LayerSpec::default() }));
    let g = convert(&spec);
    let fx = g.extras("m3fx");
    let spawn = &fx[0]["spawn"];
    assert_eq!(spawn["driven"], true);
    assert!(f(&spawn["delay"]) > 0.0, "{spawn}");
    assert!(fx[0]["anim"]["Birth"].get("burst").is_some());
    assert!(fx[0].get("tint").is_some());
}

#[test]
fn effect_textures_are_embedded_and_referenced() {
    let dir = texture_dir(&["fx_diff.png", "glow_diff.png"]);
    let cache = TextureCache::build(dir.path().to_str().unwrap()).unwrap();
    let mut spec = with_effects();
    spec.materials[0].layers = vec![("diff", LayerSpec { texture: "glow_diff.dds".into(), ..LayerSpec::default() })];
    let g = convert_with(&spec, &Converter::new().textures(&cache));
    let fx = g.extras("m3fx");
    assert_eq!(fx[0]["texture"], "#Texture0");
    assert_eq!(fx[2]["texture"], "#Texture1", "the MADD diffuse behind the composite");
    assert_eq!(g.count("images"), 2);
}
