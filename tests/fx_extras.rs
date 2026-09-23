//! The effect `extras` written into the GLB's JSON chunk.
//!
//! These are hand-rolled JSON, embedded in the one chunk that has to parse for
//! the file to be readable at all: a stray `NaN` or an unescaped quote does not
//! corrupt an effect, it corrupts the model. So every shape the writer can
//! produce is parsed back here with a real JSON parser.

use bytemuck::Zeroable;
use m3_to_glb::fx::curves::FxCurves;
use m3_to_glb::fx::{FxItem, FxKind, MaterialResolve};
use m3_to_glb::m3::structures::{Col, Lite, Par, Proj};

fn particle(par: Par) -> serde_json::Value {
    let item = FxItem {
        name:       "PAR0_test".into(),
        bone:       0,
        matm_index: Some(0),
        kind:       FxKind::Particle(Box::new(par)),
    };
    let mat = MaterialResolve { texture: Some(3), blend: "add", color: None };
    let raw = item.extras_json(&mat, &FxCurves::default());
    serde_json::from_str::<serde_json::Value>(&raw)
        .unwrap_or_else(|e| panic!("not valid JSON: {e}\n{raw}"))
}

#[test]
fn particle_extras_parse_and_carry_the_emitter() {
    let mut par = Par::zeroed();
    par.emit_rate.default = 60.0;
    par.lifespan.default = 0.75;
    par.emit_speed.default = 4.0;
    par.emit_max = 128;
    par.uv_flipbook_cols = 4;
    par.uv_flipbook_rows = 4;
    par.color_init.default = Col { b: 255, g: 128, r: 64, a: 255 };
    par.size.default.x = 0.5;

    let v = particle(par);
    let fx = &v["m3fx"];
    assert_eq!(fx["kind"], "particle");
    assert_eq!(fx["spawn"]["rate"], 60.0);
    assert_eq!(fx["lifetime"]["value"], 0.75);
    assert_eq!(fx["capacity"], 128);
    assert_eq!(fx["texture"], "#Texture3");
    assert_eq!(fx["blend"], "add");
    assert_eq!(fx["flipbook"]["cols"], 4);
    // COL is stored B,G,R,A — the first colour key must come back as R,G,B,A.
    let first = &fx["color"]["keys"][0][1];
    let chan = |i: usize| first[i].as_f64().expect("colour channel");
    assert!((chan(0) - f64::from(64.0_f32 / 255.0)).abs() < 1e-6, "red: {}", chan(0));
    assert!((chan(2) - 1.0).abs() < 1e-6, "blue: {}", chan(2));
}

#[test]
fn non_finite_fields_stay_valid_json() {
    // A truncated or hand-edited M3 can hold garbage floats. `NaN` and
    // `Infinity` are not JSON, so they must never reach the chunk.
    let mut par = Par::zeroed();
    par.emit_rate.default = f32::NAN;
    par.lifespan.default = f32::INFINITY;
    par.emit_speed.default = f32::NEG_INFINITY;
    par.gravity = f32::NAN;
    par.size.default.x = f32::NAN;
    par.size_anim_mid = f32::NAN;

    let v = particle(par);
    let text = v.to_string();
    assert!(!text.contains("NaN"), "{text}");
    assert!(!text.contains("Infinity"), "{text}");
    assert!(!text.contains("inf"), "{text}");
}

#[test]
fn unknown_enum_values_do_not_panic() {
    let mut par = Par::zeroed();
    par.particle_type = 9999;
    par.emit_shape = 200;
    par.emit_type = u32::MAX;

    let v = particle(par);
    assert_eq!(v["m3fx"]["orient"], "unknown");
    assert_eq!(v["m3fx"]["shape"]["kind"], "unknown");
    assert_eq!(v["m3fx"]["emit_type"], "unknown");
}

#[test]
fn light_and_decal_extras_parse() {
    let mut lite = Lite::zeroed();
    lite.shape = 2;
    lite.intensity.default = 3.0;
    lite.attenuation_far.default = 8.0;
    let item = FxItem {
        name:       "LITE0_test".into(),
        bone:       0,
        matm_index: None,
        kind:       FxKind::Light(Box::new(lite)),
    };
    let raw = item.extras_json(&MaterialResolve::default(), &FxCurves::default());
    let v: serde_json::Value = serde_json::from_str(&raw).expect("light JSON");
    assert_eq!(v["m3fx"]["light"], "spot");
    assert_eq!(v["m3fx"]["intensity"], 3.0);
    assert_eq!(v["m3fx"]["range"], 8.0);

    let mut proj = Proj::zeroed();
    proj.projection_type = 1;
    proj.box_offset_x_right.default = 2.0;
    proj.box_offset_x_left.default = -2.0;
    proj.lifetime_attack = 0.25;
    let item = FxItem {
        name:       "PROJ0_test".into(),
        bone:       0,
        matm_index: Some(0),
        kind:       FxKind::Decal(Box::new(proj)),
    };
    let raw = item.extras_json(&MaterialResolve::default(), &FxCurves::default());
    let v: serde_json::Value = serde_json::from_str(&raw).expect("decal JSON");
    assert_eq!(v["m3fx"]["projection"], "ortho");
    assert_eq!(v["m3fx"]["size"][0], 4.0);
    // A stage with no upper bound is a fixed duration, not a zero-width range.
    assert_eq!(v["m3fx"]["envelope"]["attack"][0], 0.25);
    assert_eq!(v["m3fx"]["envelope"]["attack"][1], 0.25);
}

#[test]
fn effect_names_survive_a_json_string() {
    // Bone names come from the file; a quote in one would otherwise split the
    // JSON chunk in half.
    let item = FxItem {
        name:       "PAR0_a\"b".into(),
        bone:       0,
        matm_index: None,
        kind:       FxKind::Particle(Box::new(Par::zeroed())),
    };
    let mat = MaterialResolve { texture: None, blend: "blend", color: Some([1.0, 0.0, 0.0, 1.0]) };
    let raw = item.extras_json(&mat, &FxCurves::default());
    serde_json::from_str::<serde_json::Value>(&raw).expect("quoted name JSON");
}

#[test]
fn every_optional_particle_field_is_written() {
    let mut par = Par::zeroed();
    par.additional_flags = 0xF; // speed / lifespan / mass randomize, world space
    par.emit_speed_random.default = 1.5;
    par.lifespan_random.default = 0.25;
    par.emit_shape_size.default.x = 1.0;
    par.emit_shape_radius.default = 2.0;
    par.emit_shape_radius_cutout.default = 0.5;
    par.emit_angle_x.default = 0.1;
    par.emit_spread_x.default = 0.2;
    par.gravity = -9.8;
    par.drag = 0.1;
    par.mass = 1.0;
    par.mass2 = 2.0;
    par.noise_amplitude = 0.3;
    // inherit velocity, sort by height, collide with both, random flipbook
    // start, tail fix, model particles
    par.flags = 0x40 | 0x80 | 0x2 | 0x4 | 0x1_0000 | 0x10_0000 | 0x40_0000;
    par.particle_type = 1;
    par.uv_flipbook_cols = 2;
    par.uv_flipbook_rows = 2;
    par.uv_flipbook_start_lifespan_factor = 0.5;
    par.trail_system = 0;

    let fx = particle(par)["m3fx"].clone();
    for key in ["shape", "angle", "spread", "gravity", "drag", "mass", "noise", "parent_velocity", "tail", "flipbook", "collide"] {
        assert!(fx.get(key).is_some(), "missing {key}: {fx}");
    }
    assert_eq!(fx["space"], "world");
    assert_eq!(fx["sort"], "height");
    assert_eq!(fx["tail"]["mode"], "fix");
    assert_eq!(fx["model_particles"], true);
    assert_eq!(fx["flipbook"]["random_start"], true);
    assert_eq!(fx["speed"]["random"], 1.5);
    assert_eq!(fx["mass"]["random"], 2.0);

    par.flags = 0x1 | 0x4_0000; // sort by distance, tail clamp
    let fx = particle(par)["m3fx"].clone();
    assert_eq!(fx["sort"], "distance");
    assert_eq!(fx["tail"]["mode"], "clamp");
    par.flags = 0;
    assert_eq!(particle(par)["m3fx"]["tail"]["mode"], "free");
}

#[test]
fn point_light_omits_cone_and_near_range() {
    let lite = Lite::zeroed();
    let item = FxItem { name: "LITE0".into(), bone: 0, matm_index: None, kind: FxKind::Light(Box::new(lite)) };
    let raw = item.extras_json(&MaterialResolve::default(), &FxCurves::default());
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(v["m3fx"]["light"], "point");
    assert!(v["m3fx"].get("inner_angle").is_none() && v["m3fx"].get("range_near").is_none());
    assert!(item.translation().is_none());
}

#[test]
fn colour_gradient_keeps_an_independent_alpha_key() {
    let mut par = Par::zeroed();
    par.color_anim_mid = 0.5;
    par.alpha_anim_mid = 0.25;
    let fx = particle(par)["m3fx"].clone();
    assert_eq!(fx["color"]["alpha_mid"], 0.25);
}
