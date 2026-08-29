# Attachment points — the `m3attach` node extras

An attachment point is where the game hangs something on a model: a weapon in
`Ref_Weapon Right`, a status icon over `Ref_Overhead`, a projectile spawning at
`Ref_Hand Left`, the targeting sphere at `Ref_Target`.

Unlike effects, these need no new nodes. In M3 an attachment point *is* a bone
plus a name, and the bone is already exported — named, parented, animated. So
the name is written into the `extras` of that bone's own node, under the key
`m3attach`:

```
Bone_Head
└── Ref_Head          extras.m3attach = {"name":"Ref_Head"}
Vol_Target            extras.m3attach = {"name":"Ref_Target",
                                         "volume":{"shape":"sphere","size":[0.25,0,0],…}}
```

Nothing else in the file references the key, so a viewer that knows nothing
about it just sees an ordinary bone.

## Why the `ATT_` table is read instead of matching `Ref_*` bone names

Because the bone's name is not reliably the attachment's name. On
`Storm_Hero_Chromie_Spring19` 28 of the 30 attachment points sit on a bone
named exactly like them — and then:

| `ATT_` name | bone name |
|---|---|
| `Ref_Target` | `Vol_Target` |
| `Ref_Shield` | `Vol_Shield` |

An engine scanning for `Ref_*` node names loses exactly those two, which are the
targeting and shield-impact points a game most needs. Reading the table also
gets the duplicates right for free: models with several of one kind carry the
numbering in the name itself (`Ref_Target 02`, `Ref_Target 03`, … on
`Ultralisk_Remastered`).

## Volumes

`ATVL` binds a shape to the same bone — the hit / target volume for that
attachment. When one matches, it rides along in the same object:

```json
{"m3attach":{"name":"Ref_Shield","volume":{
  "shape":"sphere",
  "size":[0.4,0,0],
  "matrix":[1,0,0,0, 0,1,0,0, 0,0,1,0, 0.0016,0.0073,0.4708,1]
}}}
```

| | |
|---|---|
| `shape` | `"cuboid"`, `"sphere"`, `"cylinder"` — or `"unknown"` for a value structures.xml does not name |
| `size` | the raw M3 triple; its meaning follows `shape` — radius for a sphere, extents for a cuboid, radius + height for a cylinder |
| `matrix` | bone-local transform, in glTF's own column-major order (translation at `[12..15]`), so it feeds `Mat4::from_cols_array` unchanged |

Sizes and offsets are M3 units, unscaled — the same convention as everything
else the converter emits.

## Reading them in Bevy

Bevy inserts a `GltfExtras` component on the node's entity when the scene
spawns, so the integration is one system:

```rust
fn index_attachments(mut commands: Commands, nodes: Query<(Entity, &GltfExtras), Added<GltfExtras>>) {
    for (entity, extras) in &nodes {
        let Ok(a) = serde_json::from_str::<RawExtras>(&extras.value) else { continue };
        commands.entity(entity).insert(M3Attachment { /* … */ });
    }
}
```

A complete one is in [`integration/bevy/m3attach.rs`](../integration/bevy/m3attach.rs).
Drop it into your project and add `M3AttachPlugin`; it tags every attachment
node with an `M3Attachment { name, volume }` component and gives you an
`AttachPoints` system param to find them below a spawned model:

```rust
fn equip(mut commands: Commands, points: AttachPoints, model: Single<Entity, With<Player>>) {
    if let Some(hand) = points.find(*model, "Ref_Weapon Right") {
        commands.entity(hand).with_child(SceneRoot(weapon.clone()));
    }
}
```

The tagged entity *is* the animated bone, so the child follows the animation
with no per-frame work. The file is checked against Bevy 0.18.1.

Note that `GltfExtras` arrives on effect nodes too (key `m3fx`, see
[fx-extras.md](fx-extras.md)); `serde_json::from_str` simply fails on those,
which is why the `let … else` skips rather than warns.

## Not exported

`attachment_points_addon` — the parallel `U16_` list that numbers same-named
attachments. Every attachment already has its own node, and the numbering it
encodes is already present in the name string, so it carries nothing an engine
can use.
