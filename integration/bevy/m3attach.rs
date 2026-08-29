//! `m3attach` — turn the attachment points in an m3-to-glb `.glb` into
//! queryable Bevy entities.
//!
//! The converter writes each M3 attachment point (`Ref_Head`,
//! `Ref_Weapon Right`, `Ref_Target`, …) into the `extras` of the **bone node
//! that carries it**, under the key `m3attach`. Bevy surfaces that as a
//! [`GltfExtras`] component when the scene spawns, so all this module does is
//! watch for them and tag the entity with [`M3Attachment`].
//!
//! ```ignore
//! App::new().add_plugins((DefaultPlugins, M3AttachPlugin)).run();
//! ```
//!
//! Because the tagged entity *is* the animated bone, hanging something on an
//! attachment point is an ordinary child spawn — the child follows the
//! animation with no per-frame work:
//!
//! ```ignore
//! fn equip(mut commands: Commands, points: AttachPoints, model: Single<Entity, With<Player>>) {
//!     if let Some(hand) = points.find(*model, "Ref_Weapon Right") {
//!         commands.entity(hand).with_child(SceneRoot(weapon.clone()));
//!     }
//! }
//! ```
//!
//! Do not look these up by node *name*: the bone behind `Ref_Target` is called
//! `Vol_Target`, and there are more like it. The name in `m3attach` is the
//! authored one — that is the whole reason it is exported. See
//! `docs/attachments.md`.

use bevy::ecs::system::SystemParam;
use bevy::gltf::GltfExtras;
use bevy::prelude::*;
use serde::Deserialize;

/// Tags every attachment-point node in a loaded scene with [`M3Attachment`].
pub struct M3AttachPlugin;

impl Plugin for M3AttachPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, index_attachments);
    }
}

// ─── Components ──────────────────────────────────────────────────────────────

/// An attachment point. Sits on the bone entity itself, so its
/// `GlobalTransform` is the attachment's world pose.
#[derive(Component, Debug, Clone)]
pub struct M3Attachment {
    /// Authored name — `Ref_Head`, `Ref_Weapon Right`, `Ref_Target 02`, …
    pub name:   String,
    /// Hit / target volume bound to the same bone, when the model has one.
    pub volume: Option<M3AttachVolume>,
}

/// The volume bound to an attachment point, in the bone's local frame.
#[derive(Debug, Clone, Copy)]
pub struct M3AttachVolume {
    pub shape: M3VolumeShape,
    /// Bone-local transform of the volume.
    pub local: Transform,
}

/// Sizes are M3 units, unscaled — the same units as the model's geometry.
#[derive(Debug, Clone, Copy)]
pub enum M3VolumeShape {
    Sphere { radius: f32 },
    Cuboid { extents: Vec3 },
    Cylinder { radius: f32, height: f32 },
    /// A shape id the converter did not recognise; the raw triple is kept.
    Unknown { size: Vec3 },
}

// ─── Lookup ──────────────────────────────────────────────────────────────────

/// Finds attachment points below a spawned model root.
///
/// A `SceneRoot` spawns a whole node tree, so the attachment is a *descendant*
/// of the entity you spawned, not that entity itself.
#[derive(SystemParam)]
pub struct AttachPoints<'w, 's> {
    children: Query<'w, 's, &'static Children>,
    points:   Query<'w, 's, &'static M3Attachment>,
}

impl AttachPoints<'_, '_> {
    /// The entity of the named attachment point below `root`, if the model has
    /// one. Names are compared exactly, as authored.
    #[must_use]
    pub fn find(&self, root: Entity, name: &str) -> Option<Entity> {
        self.iter(root).find(|(_, a)| a.name == name).map(|(e, _)| e)
    }

    /// Every attachment point below `root`.
    pub fn iter(&self, root: Entity) -> impl Iterator<Item = (Entity, &M3Attachment)> + '_ {
        self.children
            .iter_descendants(root)
            .filter_map(|e| self.points.get(e).ok().map(|a| (e, a)))
    }
}

// ─── Parsing ─────────────────────────────────────────────────────────────────

fn index_attachments(
    mut commands: Commands,
    nodes: Query<(Entity, &GltfExtras), Added<GltfExtras>>,
) {
    for (entity, extras) in &nodes {
        // Effect nodes carry `m3fx` in the same field and simply fail to parse
        // here — the skip is the filter.
        let Ok(raw) = serde_json::from_str::<RawExtras>(&extras.value) else {
            continue;
        };
        let raw = raw.m3attach;
        commands.entity(entity).insert(M3Attachment {
            name:   raw.name,
            volume: raw.volume.map(volume_from_raw),
        });
    }
}

fn volume_from_raw(v: RawVolume) -> M3AttachVolume {
    let [x, y, z] = v.size;
    let shape = match v.shape.as_str() {
        "sphere" => M3VolumeShape::Sphere { radius: x },
        "cuboid" => M3VolumeShape::Cuboid { extents: Vec3::new(x, y, z) },
        "cylinder" => M3VolumeShape::Cylinder { radius: x, height: y },
        _ => M3VolumeShape::Unknown { size: Vec3::new(x, y, z) },
    };
    // The converter writes the matrix in glTF's own column-major order, so it
    // goes straight into `Mat4` with no transpose.
    M3AttachVolume {
        shape,
        local: Transform::from_matrix(Mat4::from_cols_array(&v.matrix)),
    }
}

#[derive(Deserialize)]
struct RawExtras {
    m3attach: RawAttachment,
}

#[derive(Deserialize)]
struct RawAttachment {
    name:   String,
    #[serde(default)]
    volume: Option<RawVolume>,
}

#[derive(Deserialize)]
struct RawVolume {
    shape:  String,
    size:   [f32; 3],
    matrix: [f32; 16],
}
