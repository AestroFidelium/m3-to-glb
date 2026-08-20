//! `m3fx` — turn the effect nodes in an m3-to-glb `.glb` into live
//! [`bevy_hanabi`] effects.
//!
//! The converter writes each M3 particle system, light and projection as an
//! empty glTF node parented to the bone it rides on, carrying its parameters in
//! that node's `extras` under the key `m3fx`. Bevy surfaces those as a
//! [`GltfExtras`] component when the scene spawns, so all this module does is
//! watch for them and attach the matching effect — no bone-name lookup, no
//! second asset, no per-model code.
//!
//! ```ignore
//! App::new().add_plugins((DefaultPlugins, M3FxPlugin)).run();
//! ```
//!
//! [`M3FxPlugin`] adds [`HanabiPlugin`] itself, so do not add both.
//!
//! # What is reproduced, and what is not
//!
//! An M3 emitter has roughly three times the knobs hanabi exposes. What is
//! carried over is what decides whether an effect reads correctly: spawn rate
//! and bursts, emission shape and cone, initial speed, lifetime, gravity and
//! drag, the size and colour gradients, the sprite-sheet flipbook, billboard
//! orientation, blend mode and simulation space.
//!
//! Deliberately dropped, because hanabi has no equivalent and faking one would
//! look worse than leaving it out: per-particle noise fields, collision and
//! bounce, trail systems, and model particles (an emitter that spawns a *mesh*
//! per particle — those are skipped entirely rather than drawn as quads).
//!
//! Projections (`"kind":"decal"`) are ground decals; Bevy's own decal support
//! is the right home for them, so they are counted and ignored here.

use bevy::gltf::GltfExtras;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::scene::SceneRoot;
use bevy_hanabi::prelude::*;
use serde::Deserialize;

/// Spawns effects for every `m3fx` node in a loaded scene.
pub struct M3FxPlugin;

impl Plugin for M3FxPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(HanabiPlugin)
            .init_resource::<M3FxCache>()
            .add_systems(Update, attach_effects);
    }
}

/// Built effects, keyed by the `extras` text they were built from.
///
/// Every instance of the same model asks for the same emitter, so the
/// [`EffectAsset`] is built once and shared — a hundred frost novas on screen
/// are a hundred `ParticleEffect` components over a handful of assets.
#[derive(Resource, Default)]
struct M3FxCache {
    effects: HashMap<String, Handle<EffectAsset>>,
}

/// Marks a node whose effect has been attached, so a scene that re-triggers
/// `Added<GltfExtras>` cannot stack two emitters on one node.
#[derive(Component)]
struct M3FxAttached;

fn attach_effects(
    mut commands: Commands,
    nodes: Query<(Entity, &GltfExtras), (Added<GltfExtras>, Without<M3FxAttached>)>,
    parents: Query<&ChildOf>,
    scene_roots: Query<&SceneRoot>,
    asset_server: Res<AssetServer>,
    mut effects: ResMut<Assets<EffectAsset>>,
    mut cache: ResMut<M3FxCache>,
) {
    for (entity, extras) in &nodes {
        // Nodes that carry other `extras` are none of our business, and most
        // glTF files in a project have some. Parse, then ignore quietly.
        let Ok(Extras { m3fx }) = serde_json::from_str::<Extras>(&extras.value) else {
            continue;
        };
        commands.entity(entity).insert(M3FxAttached);

        match m3fx {
            Fx::Particle(particle) => {
                if particle.model_particles {
                    // The emitter draws a mesh per particle and the glTF carries
                    // no such mesh — a quad in its place would be visibly wrong.
                    continue;
                }
                let handle = cache
                    .effects
                    .entry(extras.value.clone())
                    .or_insert_with(|| effects.add(build_effect(&particle)))
                    .clone();
                commands.entity(entity).insert(ParticleEffect::new(handle));

                // The texture is a glTF texture inside the same `.glb`, named
                // relative to it — resolve it against the scene this node came
                // from, since the node itself does not know its own file.
                if let Some(slot) = particle.texture.as_deref() {
                    if let Some(path) =
                        source_glb(entity, &parents, &scene_roots, &asset_server)
                    {
                        let image: Handle<Image> = asset_server.load(format!("{path}{slot}"));
                        commands.entity(entity).insert(EffectMaterial {
                            images: vec![image],
                        });
                    }
                }
            }
            Fx::Light(light) => {
                let color = Color::linear_rgb(light.color[0], light.color[1], light.color[2]);
                // M3 intensity is not a photometric unit; scale it into
                // something Bevy's PBR pipeline reads as a local glow.
                let intensity = light.intensity.max(0.01) * LIGHT_INTENSITY_SCALE;
                if light.light == "spot" {
                    commands.entity(entity).insert(SpotLight {
                        color,
                        intensity,
                        range: light.range.max(0.1),
                        inner_angle: light.inner_angle,
                        outer_angle: light.outer_angle.max(light.inner_angle),
                        shadows_enabled: false,
                        ..default()
                    });
                } else {
                    commands.entity(entity).insert(PointLight {
                        color,
                        intensity,
                        range: light.range.max(0.1),
                        shadows_enabled: false,
                        ..default()
                    });
                }
            }
            // Ground decals belong to Bevy's decal support, not to a particle
            // system; the node is left in place for a project that wants it.
            Fx::Decal => {}
        }
    }
}

/// Candela per unit of M3 light intensity. M3 stores an artist-facing number
/// with no physical meaning, so this is a taste constant, not a conversion.
const LIGHT_INTENSITY_SCALE: f32 = 40_000.0;

/// The asset path of the `.glb` an effect node came from, without the `#Scene0`
/// label — `"models/frost.glb"`.
///
/// Walks up to the [`SceneRoot`] the node was spawned under. A node parented
/// under something else (a scene assembled by hand) simply has no source file,
/// and its textures cannot be resolved.
fn source_glb(
    entity: Entity,
    parents: &Query<&ChildOf>,
    scene_roots: &Query<&SceneRoot>,
    asset_server: &AssetServer,
) -> Option<String> {
    let mut current = entity;
    loop {
        if let Ok(root) = scene_roots.get(current) {
            let path = asset_server.get_path(root.0.id())?;
            let text = path.to_string();
            // `models/x.glb#Scene0` → `models/x.glb`
            return Some(text.split('#').next()?.to_owned());
        }
        current = parents.get(current).ok()?.parent();
    }
}

// ─── The `extras` schema ─────────────────────────────────────────────────────

#[derive(Deserialize)]
struct Extras {
    m3fx: Fx,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Fx {
    Particle(Particle),
    Light(LightFx),
    /// A ground decal. Recognised so it is not mistaken for an unknown effect,
    /// then left alone — its fields belong to a decal renderer, not to this one.
    Decal,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Particle {
    capacity:        u32,
    spawn:           Spawn,
    lifetime:        Pair,
    speed:           Pair,
    emit_type:       String,
    shape:           Shape,
    /// Emission axis, as a rotation of the bone's `+Z` about X then Y (radians).
    angle:           [f32; 2],
    /// Cone half-angles about that axis (radians).
    spread:          [f32; 2],
    gravity:         f32,
    drag:            f32,
    size:            Curve<f32>,
    color:           Curve<[f32; 4]>,
    orient:          String,
    flipbook:        Option<Flipbook>,
    texture:         Option<String>,
    tint:            Option<[f32; 4]>,
    blend:           String,
    space:           String,
    model_particles: bool,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Spawn {
    rate:  f32,
    burst: u32,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Pair {
    value:  f32,
    random: f32,
}

impl Pair {
    /// The `[lo, hi]` range to draw from. M3 randomness is signed — a negative
    /// `random` widens the range downwards — so the two are ordered here.
    fn range(&self) -> (f32, f32) {
        let other = self.value + self.random;
        (self.value.min(other), self.value.max(other))
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Shape {
    kind:   String,
    size:   [f32; 3],
    radius: f32,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Curve<T> {
    keys: Vec<(f32, T)>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Flipbook {
    cols:  u32,
    rows:  u32,
    start: [u32; 2],
    end:   [u32; 2],
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct LightFx {
    light:       String,
    color:       [f32; 3],
    intensity:   f32,
    range:       f32,
    inner_angle: f32,
    outer_angle: f32,
}

// ─── Emitter → EffectAsset ───────────────────────────────────────────────────

fn build_effect(p: &Particle) -> EffectAsset {
    let writer = ExprWriter::new();

    // ── initial position ─────────────────────────────────────────────────────
    // M3 emits from a shape centred on the bone, whose axis is the bone's +Z.
    let init_pos: Box<dyn Modifier> = match p.shape.kind.as_str() {
        "sphere" => Box::new(SetPositionSphereModifier {
            center:    writer.lit(Vec3::ZERO).expr(),
            radius:    writer.lit(p.shape.radius).expr(),
            dimension: ShapeDimension::Volume,
        }),
        "cylinder" | "disc" => Box::new(SetPositionCircleModifier {
            center:    writer.lit(Vec3::ZERO).expr(),
            axis:      writer.lit(Vec3::Z).expr(),
            radius:    writer.lit(p.shape.radius).expr(),
            dimension: ShapeDimension::Volume,
        }),
        "cube" | "plane" => {
            // A box has no modifier of its own: draw a point inside it.
            let half = Vec3::from(p.shape.size) * 0.5;
            let pos = writer
                .rand(VectorType::VEC3F)
                .mul(writer.lit(half * 2.0))
                .sub(writer.lit(half));
            Box::new(SetAttributeModifier::new(Attribute::POSITION, pos.expr()))
        }
        // "point", and the shapes with no counterpart (spline, mesh) — those
        // sample geometry this glTF does not carry.
        _ => Box::new(SetAttributeModifier::new(
            Attribute::POSITION,
            writer.lit(Vec3::ZERO).expr(),
        )),
    };

    // ── initial velocity ─────────────────────────────────────────────────────
    let (speed_lo, speed_hi) = p.speed.range();
    let speed = writer.lit(speed_lo).uniform(writer.lit(speed_hi));

    let init_vel: Box<dyn Modifier> = if p.emit_type == "radial" {
        Box::new(SetVelocitySphereModifier {
            center: writer.lit(Vec3::ZERO).expr(),
            speed:  speed.expr(),
        })
    } else {
        // A cone about the emission axis. The axis is the bone's +Z turned by
        // `angle`, and the cone's own frame turns with it, so the basis is
        // rotated on the CPU and only the random spread stays in the shader.
        let rot = Quat::from_euler(EulerRot::XYZ, p.angle[0], p.angle[1], 0.0);
        let (ex, ey, ez) = (rot * Vec3::X, rot * Vec3::Y, rot * Vec3::Z);
        let sx = p.spread[0].tan();
        let sy = p.spread[1].tan();
        let rx = writer.lit(-sx).uniform(writer.lit(sx));
        let ry = writer.lit(-sy).uniform(writer.lit(sy));

        let component = |a: f32, b: f32, c: f32| {
            rx.clone()
                .mul(writer.lit(a))
                .add(ry.clone().mul(writer.lit(b)))
                .add(writer.lit(c))
        };
        let dir = component(ex.x, ey.x, ez.x)
            .vec3(component(ex.y, ey.y, ez.y), component(ex.z, ey.z, ez.z))
            .normalized();
        Box::new(SetAttributeModifier::new(
            Attribute::VELOCITY,
            dir.mul(speed).expr(),
        ))
    };

    // ── age and lifetime ─────────────────────────────────────────────────────
    let init_age = SetAttributeModifier::new(Attribute::AGE, writer.lit(0.0).expr());
    let (life_lo, life_hi) = p.lifetime.range();
    let lifetime = writer
        .lit(life_lo.max(0.01))
        .uniform(writer.lit(life_hi.max(0.01)));
    let init_lifetime = SetAttributeModifier::new(Attribute::LIFETIME, lifetime.expr());

    // ── flipbook frame ───────────────────────────────────────────────────────
    // The frame walks from the start index to the end index over the particle's
    // life; M3 gives each end a range, and the low end of each is used.
    let sprite_index = p.flipbook.as_ref().map(|fb| {
        let first = fb.start[0] as f32;
        let last = fb.end[0].max(fb.start[0]) as f32;
        let cells = (fb.cols * fb.rows).max(1) as i32;
        writer
            .attr(Attribute::AGE)
            .div(writer.attr(Attribute::LIFETIME).max(writer.lit(0.001)))
            .saturate()
            .mul(writer.lit(last - first))
            .add(writer.lit(first))
            .cast(ScalarType::Int)
            .rem(writer.lit(cells))
            .expr()
    });

    let texture_slot = p.texture.is_some().then(|| writer.lit(0u32).expr());

    // Gravity pulls along world −Y. Built here, before the module is closed,
    // like every other expression the effect needs.
    let accel = (p.gravity != 0.0).then(|| writer.lit(Vec3::NEG_Y * p.gravity).expr());
    let drag = writer.lit(p.drag).expr();

    let mut module = writer.finish();
    if p.texture.is_some() {
        module.add_texture_slot("color");
    }

    // ── assembly ─────────────────────────────────────────────────────────────
    // A rate of zero with a burst count is a one-shot: an impact, a cast flash.
    let spawner = if p.spawn.rate > 0.0 {
        SpawnerSettings::rate(p.spawn.rate.into())
    } else {
        SpawnerSettings::once((p.spawn.burst.max(1) as f32).into())
    };

    let capacity = p.capacity.clamp(4, 65_536);
    let mut effect = EffectAsset::new(capacity, spawner, module)
        .with_name("m3fx")
        .with_alpha_mode(match p.blend.as_str() {
            "add" => bevy_hanabi::AlphaMode::Add,
            "multiply" => bevy_hanabi::AlphaMode::Multiply,
            _ => bevy_hanabi::AlphaMode::Blend,
        })
        // M3's own local/world flag decides whether particles ride the bone or
        // are left behind in the world once spawned. Anything under gravity is
        // simulated in the world regardless: gravity pulls along world −Y, which
        // means nothing inside a bone's rotating frame.
        .with_simulation_space(if p.space == "world" || p.gravity != 0.0 {
            SimulationSpace::Global
        } else {
            SimulationSpace::Local
        });

    effect = effect
        .add_modifier(ModifierContext::Init, init_pos)
        .add_modifier(ModifierContext::Init, init_vel)
        .init(init_age)
        .init(init_lifetime);

    if let Some(accel) = accel {
        effect = effect.update(AccelModifier::new(accel));
    }
    if p.drag != 0.0 {
        effect = effect.update(LinearDragModifier::new(drag));
    }

    effect = effect
        .render(SizeOverLifetimeModifier {
            gradient:          size_gradient(&p.size),
            screen_space_size: false,
        })
        .render(ColorOverLifetimeModifier::new(color_gradient(
            &p.color, p.tint,
        )))
        .render(OrientModifier::new(match p.orient.as_str() {
            // A tail or a ray is stretched along its velocity; everything else
            // is a camera-facing quad.
            "tail" | "tail_alt" | "ray" | "emission" | "ground_tail" => OrientMode::AlongVelocity,
            _ => OrientMode::FaceCameraPosition,
        }));

    if let Some(slot) = texture_slot {
        effect = effect.render(ParticleTextureModifier {
            texture_slot:   slot,
            sample_mapping: ImageSampleMapping::Modulate,
        });
    }
    if let Some(fb) = &p.flipbook {
        effect = effect.render(FlipbookModifier {
            sprite_grid_size: UVec2::new(fb.cols.max(1), fb.rows.max(1)),
        });
        if let Some(index) = sprite_index {
            effect = effect.update(SetAttributeModifier::new(Attribute::SPRITE_INDEX, index));
        }
    }

    effect
}

/// M3 sizes are a three-key curve over the particle's life, uniform on all axes.
fn size_gradient(curve: &Curve<f32>) -> Gradient<Vec3> {
    let mut gradient = Gradient::new();
    if curve.keys.is_empty() {
        gradient.add_key(0.0, Vec3::splat(0.1));
        return gradient;
    }
    for &(at, size) in &curve.keys {
        gradient.add_key(at.clamp(0.0, 1.0), Vec3::splat(size));
    }
    gradient
}

/// The colour curve, modulated by a flat-colour material's tint when the emitter
/// has no texture to carry its colour.
fn color_gradient(curve: &Curve<[f32; 4]>, tint: Option<[f32; 4]>) -> Gradient<Vec4> {
    let tint = tint.map_or(Vec4::ONE, Vec4::from);
    let mut gradient = Gradient::new();
    if curve.keys.is_empty() {
        gradient.add_key(0.0, tint);
        return gradient;
    }
    for &(at, color) in &curve.keys {
        gradient.add_key(at.clamp(0.0, 1.0), Vec4::from(color) * tint);
    }
    gradient
}
