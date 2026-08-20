//! M3 effect extraction — particle systems (`PAR_`), lights (`LITE`) and
//! projections (`PROJ`) → glTF node `extras`.
//!
//! glTF has no concept of a particle system, so effects cannot be *converted*
//! the way geometry is. What this module does instead is carry the emitter
//! **description** inside the same `.glb`: one empty node per effect, parented
//! to the bone the effect rides on, with the parameters written to that node's
//! `extras` object under the key `m3fx`.
//!
//! That placement is the whole point. The bone is already exported, already
//! animated, and already positioned by the glTF node hierarchy — so an engine
//! that spawns an emitter on the node entity gets the effect's motion for free,
//! with no bone-name lookup and no second asset to load. In Bevy the node's
//! `extras` arrive as a `GltfExtras` component on the spawned entity.
//!
//! ## What is faithful and what is not
//!
//! Every value here is the field's **static default** — the value the emitter
//! holds when no animation drives it. M3 can animate most of them through the
//! `STC_`/`STS_` sequence data; that resolution is not done here (see
//! `processor::anim` for the machinery that does it for bones). For most HotS
//! ability effects the defaults are what the effect looks like, because the
//! emitters are switched on and off by *spawning the model*, not by animating
//! the rate to zero.
//!
//! Units, axes and conventions are already in glTF terms:
//!   * lengths and speeds are M3 units, unscaled (the converter does not scale);
//!   * emitter-local directions stay in the **bone's** local frame, where +Z is
//!     the emission axis — unchanged by the Z-up → Y-up fix, which is baked into
//!     the root bone only;
//!   * `gravity` is a scalar pulling along **world −Y** (glTF world), because
//!     the M3 world −Z became −Y.

pub mod curves;

use curves::FxCurves;

use crate::m3::reader::M3File;
use crate::m3::structures::{Col, Lite, Par, Proj};

// ─── Material resolution supplied by the GLB packer ──────────────────────────

/// How an effect's M3 material came out on the glTF side.
///
/// The packer owns the image/material arrays, so it resolves the emitter's
/// `material_reference_index` and hands the result back here to be written into
/// `extras`.
#[derive(Debug, Clone, Copy, Default)]
pub struct MaterialResolve {
    /// Index into the glTF `textures` array — addressable from Bevy as
    /// `"<path>.glb#Texture{n}"`.
    pub texture: Option<usize>,
    /// Blend mode name: `"opaque"`, `"blend"`, `"add"` or `"multiply"`.
    pub blend:   &'static str,
    /// Flat colour of a textureless (colour-layer) material, if any.
    pub color:   Option<[f32; 4]>,
}

// ─── Collected effects ───────────────────────────────────────────────────────

/// One effect, still in M3 terms, with the bone it hangs off.
pub struct FxItem {
    /// Node name to emit — `PAR0_<bone>`, `LITE0_<bone>`, `PROJ0_<bone>`.
    pub name:       String,
    /// Bone index; the packer parents the effect node to this bone's node.
    pub bone:       usize,
    /// `MATM` index whose texture/blend the effect draws with, when it has one.
    pub matm_index: Option<usize>,
    pub kind:       FxKind,
}

pub enum FxKind {
    // `Par` is 1496 bytes and `Proj` 388; both are boxed so that a model whose
    // effects are mostly lights does not pay for the largest variant.
    Particle(Box<Par>),
    Light(Box<Lite>),
    Decal(Box<Proj>),
}

/// Read every effect in the model. Never fails: a model whose effects cannot be
/// read still converts its geometry, so unreadable sections are skipped.
pub fn collect(m3: &M3File<'_>) -> Vec<FxItem> {
    let bones = m3.bones().unwrap_or_default();
    let bone_name = |i: usize| -> String {
        bones
            .get(i)
            .and_then(|b| m3.read_char(&b.name).ok())
            .filter(|s| !s.is_empty())
            .map_or_else(|| format!("bone_{i}"), sanitize)
    };

    let mut out = Vec::new();

    for (i, par) in m3.particle_systems().unwrap_or_default().into_iter().enumerate() {
        let bone = par.bone as usize;
        if bone >= bones.len() {
            tracing::warn!("PAR_[{i}] references bone {bone} of {} — skipped", bones.len());
            continue;
        }
        out.push(FxItem {
            name:       format!("PAR{i}_{}", bone_name(bone)),
            bone,
            matm_index: Some(par.material_reference_index as usize),
            kind:       FxKind::Particle(Box::new(par)),
        });
    }

    for (i, lite) in m3.lights().unwrap_or_default().into_iter().enumerate() {
        let bone = lite.bone as usize;
        if bone >= bones.len() {
            continue;
        }
        out.push(FxItem {
            name:       format!("LITE{i}_{}", bone_name(bone)),
            bone,
            matm_index: None,
            kind:       FxKind::Light(Box::new(lite)),
        });
    }

    for (i, proj) in m3.projections().unwrap_or_default().into_iter().enumerate() {
        let bone = proj.bone as usize;
        if bone >= bones.len() {
            continue;
        }
        out.push(FxItem {
            name:       format!("PROJ{i}_{}", bone_name(bone)),
            bone,
            matm_index: Some(proj.material_reference_index as usize),
            kind:       FxKind::Decal(Box::new(proj)),
        });
    }

    out
}

impl FxItem {
    /// The effect node's local translation, when the effect carries an offset
    /// from its bone. Particles and lights sit on the bone; a projection has an
    /// explicit offset.
    #[must_use]
    pub fn translation(&self) -> Option<[f32; 3]> {
        match &self.kind {
            FxKind::Decal(p) => {
                let o = p.offset.default;
                (o.x != 0.0 || o.y != 0.0 || o.z != 0.0).then_some([o.x, o.y, o.z])
            }
            _ => None,
        }
    }

    /// The `extras` object for this effect — `{"m3fx":{…}}`.
    #[must_use]
    pub fn extras_json(&self, mat: &MaterialResolve, curves: &FxCurves) -> String {
        let mut j = Obj::new();
        let body = match &self.kind {
            FxKind::Particle(p) => particle_json(p, mat, curves),
            FxKind::Light(l) => light_json(l, curves),
            FxKind::Decal(p) => decal_json(p, mat),
        };
        j.raw("m3fx", &body);
        j.finish()
    }
}

// ─── Particle systems ────────────────────────────────────────────────────────

/// `emit_shape` — m3studio `bl_enum.particle_shape`.
const EMIT_SHAPES: [&str; 8] =
    ["point", "plane", "sphere", "cube", "cylinder", "disc", "spline", "mesh"];

/// `emit_type` — m3studio `bl_enum.particle_emit_type`.
const EMIT_TYPES: [&str; 5] = ["constant", "radial", "zaxis", "random", "mesh"];

/// `particle_type` — how a particle is oriented. m3studio `bl_enum.particle_type`.
const PARTICLE_TYPES: [&str; 11] = [
    "billboard",   // square to the camera
    "tail",        // camera-facing, stretched along velocity
    "emission",    // faces its velocity vector
    "world",       // fixed world yaw/pitch
    "single",      // camera-facing on Z, given yaw/pitch
    "ground",      // faces away from the terrain
    "ground_tail", // ground-facing, stretched along velocity
    "emitter",     // faces the emitter's local Z
    "collision",   // oriented by the collision that spawned it
    "ray",         // stretched from the emitter, camera-facing
    "tail_alt",
];

// `flags` bits, per structures.xml.
const F_SORT_DISTANCE: u32 = 0x1;
const F_COLLIDE_TERRAIN: u32 = 0x2;
const F_COLLIDE_OBJECTS: u32 = 0x4;
const F_INHERIT_PARENT_VELOCITY: u32 = 0x40;
const F_SORT_HEIGHT: u32 = 0x80;
const F_RANDOM_UV_FLIPBOOK_START: u32 = 0x10000;
const F_TAIL_CLAMP: u32 = 0x40000;
const F_TAIL_FIX: u32 = 0x100000;
const F_MODEL_PARTICLES: u32 = 0x400000;

// `additional_flags` bits (PAR_ v17+).
const AF_EMIT_SPEED_RANDOMIZE: u32 = 0x1;
const AF_LIFESPAN_RANDOMIZE: u32 = 0x2;
const AF_MASS_RANDOMIZE: u32 = 0x4;
const AF_WORLD_SPACE: u32 = 0x8;

fn particle_json(p: &Par, mat: &MaterialResolve, curves: &FxCurves) -> String {
    let mut j = Obj::new();
    j.string("kind", "particle");

    let lifespan = nz(p.lifespan.default, 1.0);
    // The rate an engine should actually spawn at. A HotS emitter is normally
    // authored with a static rate of zero and a curve that bursts inside one
    // sequence; taking the default alone yields an emitter that emits nothing.
    let rate_curves = curves.real(p.emit_rate.header.id);
    let burst = curves::peak_window(&rate_curves);
    let rate = burst.map_or(p.emit_rate.default, |(peak, _, _)| peak.max(p.emit_rate.default))
        .max(0.0);

    // Capacity: what the runtime has to allocate. M3's `emit_max` is the
    // emitter's own cap; when it is unset, the steady state of rate × lifetime
    // is the honest number. Clamped so one bad field cannot ask for a
    // million-particle buffer.
    let steady = (rate * lifespan * 1.25).ceil().max(1.0);
    let capacity = if p.emit_max > 0 { p.emit_max.min(65536) } else { steady as u32 };
    j.int("capacity", u64::from(capacity.clamp(4, 65536)));

    // ── spawning ─────────────────────────────────────────────────────────────
    // A burst emitter carries no rate at all: it spawns a count on one frame,
    // driven by a 16-bit track. That count is what makes an impact effect
    // appear, so it gets the same treatment as the rate.
    let count_curves = curves.int(p.emit_count.header.id);
    let count_peak = curves::peak_window(&count_curves);
    {
        let mut s = Obj::new();
        s.num("rate", rate);
        let burst_count = count_peak
            .map_or(f32::from(p.emit_count.default), |(peak, _, _)| {
                peak.max(f32::from(p.emit_count.default))
            });
        if burst_count > 0.0 {
            s.int("burst", burst_count as u64);
        }
        if p.emit_max > 0 {
            s.int("max", u64::from(p.emit_max));
        }
        // When the rate comes from a curve, the burst's shape travels with it:
        // how long after its sequence starts the emitter switches on, and how
        // long it stays on. A spawner set to `rate` for `duration` reproduces
        // the effect closely without replaying the curve at all.
        if let Some((_, delay, duration)) = burst.or(count_peak) {
            s.bool("driven", true);
            if delay > 0.0 {
                s.num("delay", delay);
            }
            if duration > 0.0 {
                s.num("duration", duration);
            }
        }
        j.raw("spawn", &s.finish());
    }

    j.raw(
        "lifetime",
        &pair(lifespan, p.lifespan_random.default, p.additional_flags & AF_LIFESPAN_RANDOMIZE != 0),
    );
    j.raw(
        "speed",
        &pair(
            p.emit_speed.default,
            p.emit_speed_random.default,
            p.additional_flags & AF_EMIT_SPEED_RANDOMIZE != 0,
        ),
    );

    // ── emission geometry ────────────────────────────────────────────────────
    j.string("emit_type", pick(&EMIT_TYPES, p.emit_type));
    {
        let mut s = Obj::new();
        s.string("kind", pick(&EMIT_SHAPES, p.emit_shape));
        let sz = p.emit_shape_size.default;
        if sz.x != 0.0 || sz.y != 0.0 || sz.z != 0.0 {
            s.vec3("size", [sz.x, sz.y, sz.z]);
        }
        if p.emit_shape_radius.default != 0.0 {
            s.num("radius", p.emit_shape_radius.default);
        }
        if p.emit_shape_radius_cutout.default != 0.0 {
            s.num("radius_cutout", p.emit_shape_radius_cutout.default);
        }
        j.raw("shape", &s.finish());
    }
    // Emission direction: the bone's +Z, turned by these angles, spread over a
    // cone of these half-angles. Radians.
    if p.emit_angle_x.default != 0.0 || p.emit_angle_y.default != 0.0 {
        j.vec2("angle", [p.emit_angle_x.default, p.emit_angle_y.default]);
    }
    if p.emit_spread_x.default != 0.0 || p.emit_spread_y.default != 0.0 {
        j.vec2("spread", [p.emit_spread_x.default, p.emit_spread_y.default]);
    }

    // ── forces ───────────────────────────────────────────────────────────────
    if p.gravity != 0.0 {
        j.num("gravity", p.gravity);
    }
    if p.drag != 0.0 {
        j.num("drag", p.drag);
    }
    if p.mass != 0.0 || p.mass2 != 0.0 {
        j.raw("mass", &pair(p.mass, p.mass2, p.additional_flags & AF_MASS_RANDOMIZE != 0));
    }
    if p.noise_amplitude != 0.0 {
        let mut s = Obj::new();
        s.num("amplitude", p.noise_amplitude);
        s.num("frequency", p.noise_frequency);
        s.num("cohesion", p.noise_cohesion);
        s.num("edge", p.noise_edge);
        j.raw("noise", &s.finish());
    }
    if p.flags & F_INHERIT_PARENT_VELOCITY != 0 {
        j.num("parent_velocity", nz(p.parent_velocity.default, 1.0));
    }

    // ── over-lifetime curves ─────────────────────────────────────────────────
    // M3 stores these as a (start, middle, end) triple plus the position of the
    // middle key along the lifetime — exactly a three-key gradient.
    let sz = p.size.default;
    let sz2 = p.size2.default;
    j.raw(
        "size",
        &gradient1(
            [sz.x, sz.y, sz.z],
            p.size_anim_mid,
            p.size_randomize != 0,
            [sz2.x, sz2.y, sz2.z],
        ),
    );
    let rot = p.rotation.default;
    let rot2 = p.rotation2.default;
    if rot.x != 0.0 || rot.y != 0.0 || rot.z != 0.0 {
        j.raw(
            "rotation",
            &gradient1(
                [rot.x, rot.y, rot.z],
                p.rotation_anim_mid,
                p.rotation_randomize != 0,
                [rot2.x, rot2.y, rot2.z],
            ),
        );
    }
    j.raw(
        "color",
        &gradient4(
            [p.color_init.default, p.color_mid.default, p.color_end.default],
            p.color_anim_mid,
            p.alpha_anim_mid,
            (p.color_randomize != 0 || p.alpha_randomize != 0).then_some([
                p.color2_init.default,
                p.color2_mid.default,
                p.color2_end.default,
            ]),
        ),
    );

    // ── look ─────────────────────────────────────────────────────────────────
    j.string("orient", pick(&PARTICLE_TYPES, p.particle_type));
    if matches!(p.particle_type, 1 | 6 | 9 | 10) {
        let mut s = Obj::new();
        s.num("length", p.instance_tail);
        s.string(
            "mode",
            if p.flags & F_TAIL_FIX != 0 {
                "fix"
            } else if p.flags & F_TAIL_CLAMP != 0 {
                "clamp"
            } else {
                "free"
            },
        );
        j.raw("tail", &s.finish());
    }
    let cells = u32::from(p.uv_flipbook_cols) * u32::from(p.uv_flipbook_rows);
    if cells > 1 {
        let mut s = Obj::new();
        s.int("cols", u64::from(p.uv_flipbook_cols));
        s.int("rows", u64::from(p.uv_flipbook_rows));
        // Start frame is picked from [start_init, start_stop], then advances to
        // a frame in [end_init, end_stop] over `lifespan_factor` of the life.
        s.vec2i("start", [p.uv_flipbook_start_init_index, p.uv_flipbook_start_stop_index]);
        s.vec2i("end", [p.uv_flipbook_end_init_index, p.uv_flipbook_end_stop_index]);
        if p.uv_flipbook_start_lifespan_factor != 0.0 {
            s.num("lifespan_factor", p.uv_flipbook_start_lifespan_factor);
        }
        if p.flags & F_RANDOM_UV_FLIPBOOK_START != 0 {
            s.bool("random_start", true);
        }
        j.raw("flipbook", &s.finish());
    }

    material_json(&mut j, mat);

    // ── simulation ───────────────────────────────────────────────────────────
    j.string("space", if p.additional_flags & AF_WORLD_SPACE != 0 { "world" } else { "local" });
    if p.flags & (F_SORT_DISTANCE | F_SORT_HEIGHT) != 0 {
        j.string("sort", if p.flags & F_SORT_HEIGHT != 0 { "height" } else { "distance" });
    }
    if p.flags & (F_COLLIDE_TERRAIN | F_COLLIDE_OBJECTS) != 0 {
        let mut s = Obj::new();
        s.bool("terrain", p.flags & F_COLLIDE_TERRAIN != 0);
        s.bool("objects", p.flags & F_COLLIDE_OBJECTS != 0);
        s.num("bounce", p.bounce);
        s.num("friction", p.friction);
        j.raw("collide", &s.finish());
    }
    // Model particles draw a mesh per particle instead of a quad, and nothing in
    // the glTF carries those meshes. Passed through so a runtime can skip the
    // emitter instead of drawing wrong-looking quads in its place.
    if p.flags & F_MODEL_PARTICLES != 0 {
        j.bool("model_particles", true);
    }
    if p.trail_system >= 0 {
        j.int("trail_of", p.trail_system as u64);
    }

    anim_json(&mut j, p, curves);

    j.finish()
}

/// The emitter's animated tracks, grouped by the sequence that drives them:
/// `"anim":{"<sequence>":{"rate":[[t,v],…],…}}`.
///
/// The sequence names match the glTF animation names exactly, so a runtime that
/// plays `Birth_full` knows which curves belong to it. Only the tracks that
/// decide whether — and how big — an effect appears are carried; everything else
/// stays at its default.
fn anim_json(j: &mut Obj, p: &Par, curves: &FxCurves) {
    if curves.is_empty() {
        return;
    }
    // (key, anim id) for the float tracks, then the vec3 ones.
    let ints: [(&str, u32); 1] = [("burst", p.emit_count.header.id)];
    let reals: [(&str, u32); 4] = [
        ("rate", p.emit_rate.header.id),
        ("speed", p.emit_speed.header.id),
        ("lifetime", p.lifespan.header.id),
        ("radius", p.emit_shape_radius.header.id),
    ];
    let vec3s: [(&str, u32); 3] = [
        ("shape_size", p.emit_shape_size.header.id),
        ("size", p.size.header.id),
        ("rotation", p.rotation.header.id),
    ];

    // Sequence name → the tracks it drives. Built as a list to keep the output
    // ordered by the first track that mentions each sequence.
    let mut by_seq: Vec<(String, Obj)> = Vec::new();
    fn slot(by_seq: &mut Vec<(String, Obj)>, name: &str) -> usize {
        if let Some(i) = by_seq.iter().position(|(n, _)| n == name) {
            i
        } else {
            by_seq.push((name.to_owned(), Obj::new()));
            by_seq.len() - 1
        }
    }

    for (key, id) in reals {
        for (seq, curve) in curves.real(id) {
            let i = slot(&mut by_seq, seq);
            by_seq[i].1.raw(key, &curves::real_json(curve));
        }
    }
    for (key, id) in ints {
        for (seq, curve) in curves.int(id) {
            let i = slot(&mut by_seq, seq);
            by_seq[i].1.raw(key, &curves::real_json(curve));
        }
    }
    for (key, id) in vec3s {
        for (seq, curve) in curves.vec3(id) {
            let i = slot(&mut by_seq, seq);
            by_seq[i].1.raw(key, &curves::vec3_json(curve));
        }
    }
    if by_seq.is_empty() {
        return;
    }

    let mut anim = Obj::new();
    for (name, tracks) in by_seq {
        anim.raw(&name, &tracks.finish());
    }
    j.raw("anim", &anim.finish());
}

// ─── Lights ──────────────────────────────────────────────────────────────────

fn light_json(l: &Lite, curves: &FxCurves) -> String {
    let mut j = Obj::new();
    j.string("kind", "light");
    // LITE.shape: 1 = point, 2 = spot (0 is an unused placeholder).
    j.string("light", if l.shape == 2 { "spot" } else { "point" });
    let c = l.color.default;
    j.vec3("color", [c.x, c.y, c.z]);
    // Like an emitter's rate, a light's intensity is normally animated rather
    // than static — a static zero would export an invisible light.
    let intensity_curves = curves.real(l.intensity.header.id);
    let peak = curves::peak_window(&intensity_curves).map_or(0.0, |(p, _, _)| p);
    j.num("intensity", l.intensity.default.max(peak));
    j.num("range", l.attenuation_far.default);
    if l.attenuation_near.default != 0.0 {
        j.num("range_near", l.attenuation_near.default);
    }
    if l.shape == 2 {
        // Hotspot / falloff are the inner and outer cone angles, in radians.
        j.num("inner_angle", l.hotspot.default);
        j.num("outer_angle", l.falloff.default);
    }
    j.finish()
}

// ─── Projections (ground decals) ─────────────────────────────────────────────

fn decal_json(p: &Proj, mat: &MaterialResolve) -> String {
    let mut j = Obj::new();
    j.string("kind", "decal");
    j.string("projection", if p.projection_type == 0 { "perspective" } else { "ortho" });
    // The projection box, in bone-local units.
    j.vec3("size", [
        p.box_offset_x_right.default - p.box_offset_x_left.default,
        p.box_offset_y_back.default - p.box_offset_y_front.default,
        p.box_offset_z_top.default - p.box_offset_z_bottom.default,
    ]);
    if p.pitch.default != 0.0 || p.yaw.default != 0.0 || p.roll.default != 0.0 {
        j.vec3("euler", [p.pitch.default, p.yaw.default, p.roll.default]);
    }
    // Alpha follows an attack → hold → decay envelope rather than a lifetime.
    j.vec3("alpha", [p.alpha_init, p.alpha_mid, p.alpha_end]);
    {
        // Each stage is a `[min, max]` range the engine picks a duration from,
        // in seconds; the `_to` field is the upper bound, and is left at zero
        // when the stage has a fixed length.
        let mut s = Obj::new();
        s.raw("attack", &range(p.lifetime_attack, p.lifetime_attack_to));
        s.raw("hold", &range(p.lifetime_hold, p.lifetime_hold_to));
        s.raw("decay", &range(p.lifetime_decay, p.lifetime_decay_to));
        j.raw("envelope", &s.finish());
    }
    material_json(&mut j, mat);
    j.finish()
}

// ─── Shared writers ──────────────────────────────────────────────────────────

fn material_json(j: &mut Obj, mat: &MaterialResolve) {
    if let Some(t) = mat.texture {
        // Bevy loads this as `asset_server.load(format!("{glb}#Texture{n}"))`.
        j.string("texture", &format!("#Texture{t}"));
    }
    if let Some(c) = mat.color {
        j.vec4("tint", c);
    }
    if !mat.blend.is_empty() {
        j.string("blend", mat.blend);
    }
}

/// A `[min, max]` range. M3 leaves the upper bound at zero for a fixed value.
fn range(min: f32, max: f32) -> String {
    format!("[{},{}]", num(min), num(max.max(min)))
}

/// A `{value, random}` pair, collapsed to just the value when nothing randomises.
fn pair(value: f32, random: f32, randomize: bool) -> String {
    let mut j = Obj::new();
    j.num("value", value);
    if randomize && random != 0.0 {
        j.num("random", random);
    }
    j.finish()
}

/// A scalar three-key gradient: `start` at 0, `mid` at `mid_at`, `end` at 1.
fn gradient1(v: [f32; 3], mid_at: f32, randomize: bool, v2: [f32; 3]) -> String {
    let mid_at = mid_at.clamp(0.0, 1.0);
    let mut j = Obj::new();
    j.raw(
        "keys",
        &format!("[[0,{}],[{},{}],[1,{}]]", num(v[0]), num(mid_at), num(v[1]), num(v[2])),
    );
    if randomize {
        j.raw("random", &format!("[{},{},{}]", num(v2[0]), num(v2[1]), num(v2[2])));
    }
    j.finish()
}

/// An RGBA three-key gradient. M3 places the middle colour key and the middle
/// alpha key independently, so both positions are carried.
fn gradient4(c: [Col; 3], mid_at: f32, alpha_mid_at: f32, c2: Option<[Col; 3]>) -> String {
    let mid_at = mid_at.clamp(0.0, 1.0);
    let mut j = Obj::new();
    j.raw("keys", &format!("[[0,{}],[{},{}],[1,{}]]", col(c[0]), num(mid_at), col(c[1]), col(c[2])));
    if (alpha_mid_at - mid_at).abs() > 1e-6 {
        j.num("alpha_mid", alpha_mid_at.clamp(0.0, 1.0));
    }
    if let Some(c2) = c2 {
        j.raw("random", &format!("[{},{},{}]", col(c2[0]), col(c2[1]), col(c2[2])));
    }
    j.finish()
}

/// M3 `COL` is stored B, G, R, A. Emitted as straight 0…1 RGBA, *not* linearised
/// — the same convention the material path uses for colour layers.
fn col(c: Col) -> String {
    format!(
        "[{},{},{},{}]",
        num(f32::from(c.r) / 255.0),
        num(f32::from(c.g) / 255.0),
        num(f32::from(c.b) / 255.0),
        num(f32::from(c.a) / 255.0)
    )
}

fn pick(table: &[&'static str], i: u32) -> &'static str {
    table.get(i as usize).copied().unwrap_or("unknown")
}

/// Replace a non-positive or non-finite value with a sane fallback.
fn nz(v: f32, fallback: f32) -> f32 {
    if v.is_finite() && v > 0.0 { v } else { fallback }
}

/// JSON number, never `NaN` / `Infinity` — neither is valid JSON, and either
/// one inside the JSON chunk makes the whole GLB unreadable.
fn num(v: f32) -> String {
    if !v.is_finite() {
        return "0".to_owned();
    }
    if v == v.trunc() && v.abs() < 1e9 {
        return format!("{}", v as i64);
    }
    format!("{v}")
}

/// A minimal JSON object writer — the same string-concatenation approach the
/// glTF manifest builder uses, so effects add no serialisation dependency.
struct Obj {
    s:     String,
    first: bool,
}

impl Obj {
    fn new() -> Self {
        Self { s: String::from("{"), first: true }
    }
    fn key(&mut self, k: &str) {
        if !self.first {
            self.s.push(',');
        }
        self.first = false;
        self.s.push('"');
        self.s.push_str(k);
        self.s.push_str("\":");
    }
    fn raw(&mut self, k: &str, v: &str) {
        self.key(k);
        self.s.push_str(v);
    }
    fn num(&mut self, k: &str, v: f32) {
        self.key(k);
        self.s.push_str(&num(v));
    }
    fn int(&mut self, k: &str, v: u64) {
        self.key(k);
        self.s.push_str(&v.to_string());
    }
    fn bool(&mut self, k: &str, v: bool) {
        self.key(k);
        self.s.push_str(if v { "true" } else { "false" });
    }
    fn string(&mut self, k: &str, v: &str) {
        self.key(k);
        // Effect strings are engine identifiers and sanitized bone names, but
        // escape defensively: an unescaped quote would corrupt the whole GLB.
        self.s.push_str(&format!("{v:?}"));
    }
    fn vec2(&mut self, k: &str, v: [f32; 2]) {
        self.raw(k, &format!("[{},{}]", num(v[0]), num(v[1])));
    }
    fn vec2i(&mut self, k: &str, v: [u8; 2]) {
        self.raw(k, &format!("[{},{}]", v[0], v[1]));
    }
    fn vec3(&mut self, k: &str, v: [f32; 3]) {
        self.raw(k, &format!("[{},{},{}]", num(v[0]), num(v[1]), num(v[2])));
    }
    fn vec4(&mut self, k: &str, v: [f32; 4]) {
        self.raw(k, &format!("[{},{},{},{}]", num(v[0]), num(v[1]), num(v[2]), num(v[3])));
    }
    fn finish(mut self) -> String {
        self.s.push('}');
        self.s
    }
}

/// Bone names come from the file and end up in a glTF node name; keep them to
/// characters that survive a JSON string and read well in an engine's hierarchy.
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect()
}
