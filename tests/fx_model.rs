//! Effect `extras` checked against a model of what each field means.
//!
//! `fx_extras.rs` pins a handful of hand-picked emitters; this file states the
//! rule behind every key — when it is written and what it holds — and lets
//! bolero search the field combinations. With no material and no animation
//! curves, the model covers the *whole* `m3fx` object, so a key that appears
//! when it should not is caught as surely as one that goes missing.
//!
//! Field values are drawn from [`Real`], a pool of the values the rules turn
//! on: zero of both signs, a value just at the gradient's key tolerance,
//! negatives and the non-finite floats a corrupt file can hold.

use bolero::{TypeGenerator, check};
use bytemuck::Zeroable;
use m3_to_glb::fx::curves::FxCurves;
use m3_to_glb::fx::{FxItem, FxKind, MaterialResolve};
use m3_to_glb::m3::structures::{Col, Par, Proj, Vec3};
use serde_json::{Map, Value, json};

// ─── Values ──────────────────────────────────────────────────────────────────

/// A float field, drawn from the values the writer's rules distinguish.
#[derive(Debug, Clone, Copy, TypeGenerator)]
enum Real {
    Zero,
    NegZero,
    /// The colour gradient's key tolerance: exactly at it is not "apart".
    Tolerance,
    Quarter,
    Half,
    One,
    Seven,
    Negative,
    Huge,
    NaN,
    Inf,
    NegInf,
}

impl Real {
    fn get(self) -> f32 {
        match self {
            Self::Zero => 0.0,
            Self::NegZero => -0.0,
            Self::Tolerance => 1e-6,
            Self::Quarter => 0.25,
            Self::Half => 0.5,
            Self::One => 1.0,
            Self::Seven => 7.0,
            Self::Negative => -1.5,
            Self::Huge => 1e6,
            Self::NaN => f32::NAN,
            Self::Inf => f32::INFINITY,
            Self::NegInf => f32::NEG_INFINITY,
        }
    }
}

fn vec3(v: [Real; 3]) -> Vec3 {
    Vec3 {
        x: v[0].get(),
        y: v[1].get(),
        z: v[2].get(),
    }
}

/// An M3 `u32` used as a boolean: anything but zero is on.
#[derive(Debug, Clone, Copy, TypeGenerator)]
enum M3Bool {
    Off,
    On,
    OtherNonZero,
}

impl M3Bool {
    fn raw(self) -> u32 {
        match self {
            Self::Off => 0,
            Self::On => 1,
            Self::OtherNonZero => 0xFFFF_FFFF,
        }
    }
}

/// `PAR_.flags` bits the writer reads, plus one it ignores.
#[derive(Debug, Clone, Copy, PartialEq, TypeGenerator)]
enum ParFlag {
    SortDistance,
    CollideTerrain,
    CollideObjects,
    InheritParentVelocity,
    SortHeight,
    RandomFlipbookStart,
    TailClamp,
    TailFix,
    ModelParticles,
    /// A bit with no meaning to the writer.
    Ignored,
}

impl ParFlag {
    fn bit(self) -> u32 {
        match self {
            Self::SortDistance => 0x1,
            Self::CollideTerrain => 0x2,
            Self::CollideObjects => 0x4,
            Self::InheritParentVelocity => 0x40,
            Self::SortHeight => 0x80,
            Self::RandomFlipbookStart => 0x0001_0000,
            Self::TailClamp => 0x0004_0000,
            Self::TailFix => 0x0010_0000,
            Self::ModelParticles => 0x0040_0000,
            Self::Ignored => 0x8,
        }
    }
}

/// `PAR_.additional_flags` bits the writer reads, plus one it ignores.
#[derive(Debug, Clone, Copy, PartialEq, TypeGenerator)]
enum AdditionalFlag {
    SpeedRandomize,
    LifespanRandomize,
    MassRandomize,
    WorldSpace,
    Ignored,
}

impl AdditionalFlag {
    fn bit(self) -> u32 {
        match self {
            Self::SpeedRandomize => 0x1,
            Self::LifespanRandomize => 0x2,
            Self::MassRandomize => 0x4,
            Self::WorldSpace => 0x8,
            Self::Ignored => 0x10,
        }
    }
}

/// `emit_max`: unset, a cap, or one past what a runtime may allocate.
#[derive(Debug, Clone, Copy, TypeGenerator)]
enum EmitMax {
    Unset,
    Cap(#[generator(1u32..=200)] u32),
    OverLimit,
}

impl EmitMax {
    fn raw(self) -> u32 {
        match self {
            Self::Unset => 0,
            Self::Cap(n) => n,
            Self::OverLimit => 100_000,
        }
    }
}

/// `emit_count`: the burst a 16-bit track spawns on one frame.
#[derive(Debug, Clone, Copy, TypeGenerator)]
enum Burst {
    None,
    Count(#[generator(1i16..=i16::MAX)] i16),
    Negative(#[generator(i16::MIN..=-1)] i16),
}

impl Burst {
    fn raw(self) -> i16 {
        match self {
            Self::None => 0,
            Self::Count(n) | Self::Negative(n) => n,
        }
    }
}

/// `trail_system`: an index, or negative for none.
#[derive(Debug, Clone, Copy, TypeGenerator)]
enum Trail {
    None,
    Of(u8),
}

impl Trail {
    fn raw(self) -> i32 {
        match self {
            Self::None => -1,
            Self::Of(i) => i32::from(i),
        }
    }
}

// ─── Particle scenario ───────────────────────────────────────────────────────

#[derive(Debug, TypeGenerator)]
struct Particle {
    #[generator(bolero::generator::produce::<Vec<ParFlag>>().with().len(0usize..=6))]
    flags: Vec<ParFlag>,
    #[generator(bolero::generator::produce::<Vec<AdditionalFlag>>().with().len(0usize..=4))]
    additional: Vec<AdditionalFlag>,
    rate: Real,
    lifespan: Real,
    lifespan_random: Real,
    speed: Real,
    speed_random: Real,
    emit_max: EmitMax,
    burst: Burst,
    #[generator(0u32..=6)]
    emit_type: u32,
    #[generator(0u32..=9)]
    emit_shape: u32,
    shape_size: [Real; 3],
    radius: Real,
    radius_cutout: Real,
    angle: [Real; 2],
    spread: [Real; 2],
    gravity: Real,
    drag: Real,
    mass: [Real; 2],
    noise: [Real; 4],
    parent_velocity: Real,
    size: [[Real; 3]; 2],
    size_mid: Real,
    size_randomize: M3Bool,
    rotation: [[Real; 3]; 2],
    rotation_mid: Real,
    rotation_randomize: M3Bool,
    /// Initial, middle, end; then the randomised set.
    colors: [[u8; 4]; 6],
    color_mid: Real,
    alpha_mid: Real,
    color_randomize: M3Bool,
    alpha_randomize: M3Bool,
    #[generator(0u32..=12)]
    orient: u32,
    tail: Real,
    #[generator(0u16..=3)]
    flipbook_cols: u16,
    #[generator(0u16..=3)]
    flipbook_rows: u16,
    /// Start init / stop, end init / stop.
    flipbook_frames: [u8; 4],
    flipbook_lifespan_factor: Real,
    bounce: Real,
    friction: Real,
    trail: Trail,
}

fn has(flags: &[ParFlag], f: ParFlag) -> bool {
    flags.contains(&f)
}

fn has_additional(flags: &[AdditionalFlag], f: AdditionalFlag) -> bool {
    flags.contains(&f)
}

fn col(c: [u8; 4]) -> Col {
    Col {
        r: c[0],
        g: c[1],
        b: c[2],
        a: c[3],
    }
}

impl Particle {
    fn to_par(&self) -> Par {
        let mut p = Par::zeroed();
        p.flags = self.flags.iter().fold(0, |acc, f| acc | f.bit());
        p.additional_flags = self.additional.iter().fold(0, |acc, f| acc | f.bit());
        p.emit_rate.default = self.rate.get();
        p.lifespan.default = self.lifespan.get();
        p.lifespan_random.default = self.lifespan_random.get();
        p.emit_speed.default = self.speed.get();
        p.emit_speed_random.default = self.speed_random.get();
        p.emit_max = self.emit_max.raw();
        p.emit_count.default = self.burst.raw();
        p.emit_type = self.emit_type;
        p.emit_shape = self.emit_shape;
        p.emit_shape_size.default = vec3(self.shape_size);
        p.emit_shape_radius.default = self.radius.get();
        p.emit_shape_radius_cutout.default = self.radius_cutout.get();
        p.emit_angle_x.default = self.angle[0].get();
        p.emit_angle_y.default = self.angle[1].get();
        p.emit_spread_x.default = self.spread[0].get();
        p.emit_spread_y.default = self.spread[1].get();
        p.gravity = self.gravity.get();
        p.drag = self.drag.get();
        p.mass = self.mass[0].get();
        p.mass2 = self.mass[1].get();
        p.noise_amplitude = self.noise[0].get();
        p.noise_frequency = self.noise[1].get();
        p.noise_cohesion = self.noise[2].get();
        p.noise_edge = self.noise[3].get();
        p.parent_velocity.default = self.parent_velocity.get();
        p.size.default = vec3(self.size[0]);
        p.size2.default = vec3(self.size[1]);
        p.size_anim_mid = self.size_mid.get();
        p.size_randomize = self.size_randomize.raw();
        p.rotation.default = vec3(self.rotation[0]);
        p.rotation2.default = vec3(self.rotation[1]);
        p.rotation_anim_mid = self.rotation_mid.get();
        p.rotation_randomize = self.rotation_randomize.raw();
        let [c0, c1, c2, r0, r1, r2] = self.colors.map(col);
        p.color_init.default = c0;
        p.color_mid.default = c1;
        p.color_end.default = c2;
        p.color2_init.default = r0;
        p.color2_mid.default = r1;
        p.color2_end.default = r2;
        p.color_anim_mid = self.color_mid.get();
        p.alpha_anim_mid = self.alpha_mid.get();
        p.color_randomize = self.color_randomize.raw();
        p.alpha_randomize = self.alpha_randomize.raw();
        p.particle_type = self.orient;
        p.instance_tail = self.tail.get();
        p.uv_flipbook_cols = self.flipbook_cols;
        p.uv_flipbook_rows = self.flipbook_rows;
        [
            p.uv_flipbook_start_init_index,
            p.uv_flipbook_start_stop_index,
            p.uv_flipbook_end_init_index,
            p.uv_flipbook_end_stop_index,
        ] = self.flipbook_frames;
        p.uv_flipbook_start_lifespan_factor = self.flipbook_lifespan_factor.get();
        p.bounce = self.bounce.get();
        p.friction = self.friction.get();
        p.trail_system = self.trail.raw();
        p
    }
}

// ─── The model ───────────────────────────────────────────────────────────────

/// A JSON number as the writer must emit it: never `NaN` / `Infinity`.
fn n(v: f32) -> Value {
    json!(if v.is_finite() { f64::from(v) } else { 0.0 })
}

/// Set only when the value says something: non-zero (`NaN` counts — it is not
/// zero, it is just unreadable).
fn set(v: f32) -> bool {
    v != 0.0
}

/// A lifetime / speed that must be positive and finite, else its fallback.
fn positive_or(v: f32, fallback: f32) -> f32 {
    if v.is_finite() && v > 0.0 {
        v
    } else {
        fallback
    }
}

fn pair(value: f32, random: f32, randomize: bool) -> Value {
    let mut o = Map::new();
    o.insert("value".into(), n(value));
    if randomize && set(random) {
        o.insert("random".into(), n(random));
    }
    Value::Object(o)
}

fn rgba(c: [u8; 4]) -> Value {
    json!(c.map(|ch| n(f32::from(ch) / 255.0)))
}

fn gradient1(v: [Real; 3], mid: Real, randomize: M3Bool, v2: [Real; 3]) -> Value {
    let mut o = Map::new();
    o.insert(
        "keys".into(),
        json!([
            [0, n(v[0].get())],
            [n(mid.get().clamp(0.0, 1.0)), n(v[1].get())],
            [1, n(v[2].get())]
        ]),
    );
    if randomize.raw() != 0 {
        o.insert("random".into(), json!(v2.map(|r| n(r.get()))));
    }
    Value::Object(o)
}

fn color(p: &Particle) -> Value {
    let [c0, c1, c2, r0, r1, r2] = p.colors;
    let mid = p.color_mid.get().clamp(0.0, 1.0);
    let mut o = Map::new();
    o.insert(
        "keys".into(),
        json!([[0, rgba(c0)], [n(mid), rgba(c1)], [1, rgba(c2)]]),
    );
    // Alpha's middle key is only carried when it sits apart from colour's.
    let alpha = p.alpha_mid.get();
    if (alpha - mid).abs() > 1e-6 {
        o.insert("alpha_mid".into(), n(alpha.clamp(0.0, 1.0)));
    }
    if p.color_randomize.raw() != 0 || p.alpha_randomize.raw() != 0 {
        o.insert("random".into(), json!([rgba(r0), rgba(r1), rgba(r2)]));
    }
    Value::Object(o)
}

fn name_of(table: &[&str], i: u32) -> Value {
    json!(
        usize::try_from(i)
            .ok()
            .and_then(|i| table.get(i))
            .unwrap_or(&"unknown")
    )
}

fn capacity(p: &Particle) -> u64 {
    match p.emit_max.raw() {
        0 => {
            // Steady state: rate × lifetime, with headroom, at least one.
            let rate = f64::from(p.rate.get().max(0.0));
            let life = f64::from(positive_or(p.lifespan.get(), 1.0));
            let steady = (rate * life * 1.25).ceil().clamp(1.0, 65536.0);
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "clamped to 1..=65536 first"
            )]
            let steady = steady as u64;
            steady.clamp(4, 65536)
        }
        cap => u64::from(cap).clamp(4, 65536),
    }
}

fn spawn(p: &Particle) -> Value {
    let mut o = Map::new();
    o.insert("rate".into(), n(p.rate.get().max(0.0)));
    if let Burst::Count(n) = p.burst {
        o.insert("burst".into(), json!(n));
    }
    if p.emit_max.raw() > 0 {
        o.insert("max".into(), json!(p.emit_max.raw()));
    }
    Value::Object(o)
}

fn shape(p: &Particle) -> Value {
    const SHAPES: [&str; 8] = [
        "point", "plane", "sphere", "cube", "cylinder", "disc", "spline", "mesh",
    ];
    let mut o = Map::new();
    o.insert("kind".into(), name_of(&SHAPES, p.emit_shape));
    if p.shape_size.iter().any(|r| set(r.get())) {
        o.insert("size".into(), json!(p.shape_size.map(|r| n(r.get()))));
    }
    if set(p.radius.get()) {
        o.insert("radius".into(), n(p.radius.get()));
    }
    if set(p.radius_cutout.get()) {
        o.insert("radius_cutout".into(), n(p.radius_cutout.get()));
    }
    Value::Object(o)
}

fn tail(p: &Particle) -> Option<Value> {
    // Tail-like orientations: tail, ground tail, ray, tail_alt.
    matches!(p.orient, 1 | 6 | 9 | 10).then(|| {
        let mode = if has(&p.flags, ParFlag::TailFix) {
            "fix"
        } else if has(&p.flags, ParFlag::TailClamp) {
            "clamp"
        } else {
            "free"
        };
        json!({ "length": n(p.tail.get()), "mode": mode })
    })
}

fn flipbook(p: &Particle) -> Option<Value> {
    let cells = u32::from(p.flipbook_cols) * u32::from(p.flipbook_rows);
    (cells > 1).then(|| {
        let [si, ss, ei, es] = p.flipbook_frames;
        let mut o = Map::new();
        o.insert("cols".into(), json!(p.flipbook_cols));
        o.insert("rows".into(), json!(p.flipbook_rows));
        o.insert("start".into(), json!([si, ss]));
        o.insert("end".into(), json!([ei, es]));
        if set(p.flipbook_lifespan_factor.get()) {
            o.insert(
                "lifespan_factor".into(),
                n(p.flipbook_lifespan_factor.get()),
            );
        }
        if has(&p.flags, ParFlag::RandomFlipbookStart) {
            o.insert("random_start".into(), json!(true));
        }
        Value::Object(o)
    })
}

fn collide(p: &Particle) -> Option<Value> {
    let terrain = has(&p.flags, ParFlag::CollideTerrain);
    let objects = has(&p.flags, ParFlag::CollideObjects);
    (terrain || objects).then(|| {
        json!({
            "terrain": terrain,
            "objects": objects,
            "bounce": n(p.bounce.get()),
            "friction": n(p.friction.get()),
        })
    })
}

fn sort(p: &Particle) -> Option<Value> {
    if has(&p.flags, ParFlag::SortHeight) {
        Some(json!("height"))
    } else if has(&p.flags, ParFlag::SortDistance) {
        Some(json!("distance"))
    } else {
        None
    }
}

/// A key of the `m3fx` object, `None` when the writer must leave it out.
type Entry = (&'static str, Option<Value>);

fn any_set(v: &[Real]) -> bool {
    v.iter().any(|r| set(r.get()))
}

fn spawn_entries(p: &Particle) -> Vec<Entry> {
    let additional = |f| has_additional(&p.additional, f);
    vec![
        ("kind", Some(json!("particle"))),
        ("capacity", Some(json!(capacity(p)))),
        ("spawn", Some(spawn(p))),
        (
            "lifetime",
            Some(pair(
                positive_or(p.lifespan.get(), 1.0),
                p.lifespan_random.get(),
                additional(AdditionalFlag::LifespanRandomize),
            )),
        ),
        (
            "speed",
            Some(pair(
                p.speed.get(),
                p.speed_random.get(),
                additional(AdditionalFlag::SpeedRandomize),
            )),
        ),
    ]
}

fn emission_entries(p: &Particle) -> Vec<Entry> {
    const EMIT_TYPES: [&str; 5] = ["constant", "radial", "zaxis", "random", "mesh"];
    let [amplitude, frequency, cohesion, edge] = p.noise.map(Real::get);
    vec![
        ("emit_type", Some(name_of(&EMIT_TYPES, p.emit_type))),
        ("shape", Some(shape(p))),
        (
            "angle",
            any_set(&p.angle).then(|| json!(p.angle.map(|r| n(r.get())))),
        ),
        (
            "spread",
            any_set(&p.spread).then(|| json!(p.spread.map(|r| n(r.get())))),
        ),
        ("gravity", set(p.gravity.get()).then(|| n(p.gravity.get()))),
        ("drag", set(p.drag.get()).then(|| n(p.drag.get()))),
        (
            "mass",
            any_set(&p.mass).then(|| {
                pair(
                    p.mass[0].get(),
                    p.mass[1].get(),
                    has_additional(&p.additional, AdditionalFlag::MassRandomize),
                )
            }),
        ),
        (
            "noise",
            set(amplitude).then(|| {
                json!({
                    "amplitude": n(amplitude),
                    "frequency": n(frequency),
                    "cohesion": n(cohesion),
                    "edge": n(edge),
                })
            }),
        ),
        (
            "parent_velocity",
            has(&p.flags, ParFlag::InheritParentVelocity)
                .then(|| n(positive_or(p.parent_velocity.get(), 1.0))),
        ),
    ]
}

fn look_entries(p: &Particle) -> Vec<Entry> {
    const ORIENTS: [&str; 11] = [
        "billboard",
        "tail",
        "emission",
        "world",
        "single",
        "ground",
        "ground_tail",
        "emitter",
        "collision",
        "ray",
        "tail_alt",
    ];
    vec![
        (
            "size",
            Some(gradient1(
                p.size[0],
                p.size_mid,
                p.size_randomize,
                p.size[1],
            )),
        ),
        (
            "rotation",
            any_set(&p.rotation[0]).then(|| {
                gradient1(
                    p.rotation[0],
                    p.rotation_mid,
                    p.rotation_randomize,
                    p.rotation[1],
                )
            }),
        ),
        ("color", Some(color(p))),
        ("orient", Some(name_of(&ORIENTS, p.orient))),
        ("tail", tail(p)),
        ("flipbook", flipbook(p)),
    ]
}

fn simulation_entries(p: &Particle) -> Vec<Entry> {
    let world = has_additional(&p.additional, AdditionalFlag::WorldSpace);
    vec![
        ("space", Some(json!(if world { "world" } else { "local" }))),
        ("sort", sort(p)),
        ("collide", collide(p)),
        (
            "model_particles",
            has(&p.flags, ParFlag::ModelParticles).then_some(json!(true)),
        ),
        (
            "trail_of",
            u64::try_from(p.trail.raw()).ok().map(|t| json!(t)),
        ),
    ]
}

/// Everything the writer emits for `p` with no material and no curves.
fn expected_particle(p: &Particle) -> Value {
    [
        spawn_entries(p),
        emission_entries(p),
        look_entries(p),
        simulation_entries(p),
    ]
    .into_iter()
    .flatten()
    .filter_map(|(key, v)| Some((key.to_owned(), v?)))
    .collect::<Map<_, _>>()
    .into()
}

// ─── Decal scenario ──────────────────────────────────────────────────────────

#[derive(Debug, TypeGenerator)]
struct Decal {
    perspective: bool,
    offset: [Real; 3],
    /// Left, right, front, back, bottom, top.
    box_offsets: [Real; 6],
    /// Pitch, yaw, roll.
    euler: [Real; 3],
    alpha: [Real; 3],
    /// Attack, hold, decay: `(min, max)` each.
    envelope: [[Real; 2]; 3],
    lasting: bool,
    other_flags: M3Bool,
}

impl Decal {
    fn to_proj(&self) -> Proj {
        let mut p = Proj::zeroed();
        p.projection_type = u32::from(!self.perspective);
        p.offset.default = vec3(self.offset);
        let [left, right, front, back, bottom, top] = self.box_offsets.map(Real::get);
        p.box_offset_x_left.default = left;
        p.box_offset_x_right.default = right;
        p.box_offset_y_front.default = front;
        p.box_offset_y_back.default = back;
        p.box_offset_z_bottom.default = bottom;
        p.box_offset_z_top.default = top;
        [p.pitch.default, p.yaw.default, p.roll.default] = self.euler.map(Real::get);
        [p.alpha_init, p.alpha_mid, p.alpha_end] = self.alpha.map(Real::get);
        [
            [p.lifetime_attack, p.lifetime_attack_to],
            [p.lifetime_hold, p.lifetime_hold_to],
            [p.lifetime_decay, p.lifetime_decay_to],
        ] = self.envelope.map(|stage| stage.map(Real::get));
        // Bit 0x4 means lasting; the other bits must not.
        p.flags = (self.other_flags.raw() & !0x4) | if self.lasting { 0x4 } else { 0 };
        p
    }
}

fn expected_decal(d: &Decal) -> Value {
    let [left, right, front, back, bottom, top] = d.box_offsets.map(Real::get);
    // A stage with no upper bound has a fixed length: `[min, min]`.
    let range = |[min, max]: [Real; 2]| json!([n(min.get()), n(max.get().max(min.get()))]);
    let [attack, hold, decay] = d.envelope;
    let mut o = Map::new();
    o.insert("kind".into(), json!("decal"));
    o.insert(
        "projection".into(),
        json!(if d.perspective {
            "perspective"
        } else {
            "ortho"
        }),
    );
    o.insert(
        "size".into(),
        json!([n(right - left), n(back - front), n(top - bottom)]),
    );
    if d.euler.iter().any(|v| set(v.get())) {
        o.insert("euler".into(), json!(d.euler.map(|v| n(v.get()))));
    }
    o.insert("alpha".into(), json!(d.alpha.map(|v| n(v.get()))));
    o.insert(
        "envelope".into(),
        json!({ "attack": range(attack), "hold": range(hold), "decay": range(decay) }),
    );
    if d.lasting {
        o.insert("persistent".into(), json!(true));
    }
    Value::Object(o)
}

// ─── Comparison ──────────────────────────────────────────────────────────────

/// Structural equality with numbers compared at `f32` precision: the writer
/// prints the shortest text that reads back as the same `f32`, which is not
/// always the `f64` the model computed it from.
fn same(actual: &Value, expected: &Value) -> bool {
    #[expect(clippy::cast_possible_truncation, reason = "comparing at f32")]
    let f32_of = |v: &Value| v.as_f64().map(|x| x as f32);
    match (actual, expected) {
        (Value::Number(_), Value::Number(_)) => f32_of(actual) == f32_of(expected),
        (Value::Array(a), Value::Array(e)) => {
            a.len() == e.len() && a.iter().zip(e).all(|(a, e)| same(a, e))
        }
        (Value::Object(a), Value::Object(e)) => {
            a.len() == e.len() && a.iter().all(|(k, v)| e.get(k).is_some_and(|e| same(v, e)))
        }
        _ => actual == expected,
    }
}

fn m3fx(item: &FxItem) -> Value {
    let raw = item.extras_json(&MaterialResolve::default(), &FxCurves::default());
    let mut v: Value =
        serde_json::from_str(&raw).unwrap_or_else(|e| panic!("not valid JSON: {e}\n{raw}"));
    v["m3fx"].take()
}

fn item(kind: FxKind) -> FxItem {
    FxItem {
        name: "FX0_test".into(),
        bone: 0,
        matm_index: None,
        kind,
    }
}

// ─── Properties ──────────────────────────────────────────────────────────────

#[test]
fn particle_extras_match_the_model() {
    check!().with_type::<Particle>().for_each(|p| {
        let item = item(FxKind::Particle(Box::new(p.to_par())));
        let (actual, expected) = (m3fx(&item), expected_particle(p));
        assert!(
            same(&actual, &expected),
            "\nactual:   {actual}\nexpected: {expected}"
        );
        assert_eq!(item.translation(), None, "a particle sits on its bone");
    });
}

#[test]
fn decal_extras_match_the_model() {
    check!().with_type::<Decal>().for_each(|d| {
        let item = item(FxKind::Decal(Box::new(d.to_proj())));
        let (actual, expected) = (m3fx(&item), expected_decal(d));
        assert!(
            same(&actual, &expected),
            "\nactual:   {actual}\nexpected: {expected}"
        );
        // The node is moved off its bone exactly when the decal has an offset.
        let offset = d.offset.map(Real::get);
        let expected = offset.iter().any(|&v| set(v)).then_some(offset);
        assert_eq!(
            item.translation().map(|t| t.map(f32::to_bits)),
            expected.map(|t| t.map(f32::to_bits))
        );
    });
}
