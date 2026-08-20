# Effects — the `m3fx` node extras

glTF has no particle systems. It has no lights that behave like M3's, and no
projected decals. So the effects in an M3 file cannot be *converted* the way
geometry is — before this existed, `Storm_FX_Jaina_Base_RingofFrost.m3` came out
as a 188-byte GLB with nothing in it, and files like
`Storm_FX_Jaina_Base_Chilled.m3` — five emitters and no mesh at all — failed to
convert outright.

They now travel **inside the same `.glb`**: one empty node per effect, parented
to the bone it rides on, with its parameters written to that node's `extras`
object under the key `m3fx`.

```
armature
└── Bone_RingOfFrost            ← already exported, already animated
    ├── PAR0_WhispySmoke        ← empty node, extras.m3fx = {…}
    ├── PAR1_Flakes_Channel
    └── LITE0_LightOmni
```

The placement is the whole point. The bone is already positioned by the node
hierarchy and already driven by the glTF animations, so an engine that spawns an
emitter on the node's entity gets the effect's motion for free — no bone-name
lookup, no second asset to load, no per-model code. Nothing else in the file
references these nodes, so a viewer that knows nothing about them simply sees an
empty node.

Effects are on by default. `--no-fx` turns them off.

## Reading them in Bevy

Bevy inserts a `GltfExtras` component on the node's entity when the scene
spawns, so the whole integration is one system:

```rust
fn attach(nodes: Query<(Entity, &GltfExtras), Added<GltfExtras>>) {
    for (entity, extras) in &nodes {
        let Ok(fx) = serde_json::from_str::<Extras>(&extras.value) else { continue };
        // …build an emitter on `entity`
    }
}
```

A complete one — parsing every field below and building a `bevy_hanabi`
`EffectAsset` from it — is in [`integration/bevy/m3fx.rs`](../integration/bevy/m3fx.rs).
Drop it into your project, add `bevy_hanabi = "0.18"` (Bevy 0.18), and add
`M3FxPlugin`. It is checked against Bevy 0.18.1 + hanabi 0.18 and builds every
one of the 127 emitters in the Jaina asset set.

## Conventions

| | |
|---|---|
| Lengths, speeds | M3 units, unscaled — the converter does not scale geometry either |
| Angles | radians |
| Times | seconds |
| Colours | straight `[r, g, b, a]` in `0…1`, **not** linearised (same convention the material path uses) |
| Local directions | the **bone's** frame, where `+Z` is the emission axis. The Z-up → Y-up fix is baked into the root bone only, so a bone-local axis is unchanged |
| `gravity` | a scalar pulling along **world −Y** — M3's world −Z after the axis fix |
| `texture` | `"#TextureN"`, an index into this same file's glTF `textures` array. In Bevy: `asset_server.load(format!("{glb_path}{slot}"))` |

## Static values and animated ones

Every field is the emitter's **static default**, with one deliberate exception.

A HotS emitter is normally authored with `emit_rate` at `0` and a curve that
bursts inside one sequence — exporting defaults alone left 72 of 127 emitters
silent. So the tracks that decide whether an effect appears at all are resolved
out of the `STC_` sequence data and folded back into values a spawner can use:

* `spawn.rate` is the **peak** of the rate curve, not its default;
* `spawn.burst` likewise, for count-driven emitters;
* `spawn.driven`, `spawn.delay` and `spawn.duration` describe the burst window —
  how long after the sequence starts the emitter switches on, and for how long.

That leaves 10 of 127 silent, all in models that ship no animation data of their
own (pass the companion `.m3a` with `-a` and they resolve too).

The exact curves are also written under `anim`, keyed by sequence name — the
same names the glTF animations carry, so a runtime that plays `Birth_full` knows
which curves belong to it:

```json
"anim": {
  "Birth_full": { "rate": [[0, 0], [1.43, 180], [1.63, 0]] },
  "Stand_full": { "speed": [[0, 10], [2, 4]] }
}
```

A runtime that ignores `anim` entirely still gets a reasonable effect from the
folded values.

## Schema

### `"kind": "particle"`

```json
{"m3fx": {
  "kind": "particle",
  "capacity": 136,
  "spawn": {"rate": 120, "burst": 30, "max": 136,
            "driven": true, "delay": 1.466, "duration": 0.867},
  "lifetime": {"value": 0.4, "random": 1},
  "speed": {"value": -1.5, "random": -1},
  "emit_type": "constant",
  "shape": {"kind": "cylinder", "size": [0.46, 0.49, 2],
            "radius": 3.9, "radius_cutout": 2.65},
  "angle": [0, 0],
  "spread": [0.174, 0.174],
  "gravity": -13,
  "drag": 3,
  "mass": {"value": 1, "random": 1.25},
  "noise": {"amplitude": 0, "frequency": 0, "cohesion": 0, "edge": 0},
  "parent_velocity": 0.2,
  "size": {"keys": [[0, 0.8], [0.996, 1.5], [1, 1.5]], "random": [1.2, 3, 3]},
  "rotation": {"keys": [[0, 0], [0.996, -0.52], [1, 0]]},
  "color": {"keys": [[0, [0.66, 0.84, 0.96, 0]],
                     [0.996, [0.51, 0.69, 0.84, 0.59]],
                     [1, [0.51, 0.69, 0.84, 0]]],
            "alpha_mid": 0.5},
  "orient": "billboard",
  "tail": {"length": 1, "mode": "free"},
  "flipbook": {"cols": 8, "rows": 8, "start": [1, 4], "end": [60, 63],
               "random_start": true},
  "texture": "#Texture0",
  "tint": [1, 1, 1, 1],
  "blend": "add",
  "space": "local",
  "sort": "distance",
  "collide": {"terrain": true, "objects": false, "bounce": 0.3, "friction": 0.1},
  "model_particles": false,
  "trail_of": 2,
  "anim": {"…": {}}
}}
```

Fields that are zero or absent in the source are omitted, so read defensively —
only `kind`, `capacity`, `spawn`, `lifetime`, `speed`, `emit_type`, `shape`,
`size`, `color`, `orient` and `space` are always present.

| Field | Meaning |
|---|---|
| `capacity` | Particles the emitter can have alive at once — `emit_max`, or `rate × lifetime` when it is unset. Clamped to `4…65536` |
| `spawn.rate` | Particles per second (peak of the curve, see above) |
| `spawn.burst` | Particles emitted at once, for count-driven emitters |
| `spawn.max` | The emitter's own cap, when it sets one |
| `spawn.driven` | The rate or burst came from an animation, not a static field |
| `spawn.delay` / `.duration` | The burst window, in seconds from the start of its sequence |
| `lifetime`, `speed`, `mass` | `{value, random}` — `random` is signed and widens the range in that direction |
| `emit_type` | `constant` (along the axis, with spread) · `radial` · `zaxis` · `random` · `mesh` |
| `shape.kind` | `point` · `plane` · `sphere` · `cube` · `cylinder` · `disc` · `spline` · `mesh`. `spline` and `mesh` sample geometry the glTF does not carry — treat as `point` |
| `angle` | Emission axis: the bone's `+Z` turned about X, then Y |
| `spread` | Cone half-angles about that axis |
| `size`, `rotation` | Three-key curves over the particle's life: `[[0, start], [mid_at, mid], [1, end]]`. `random` is the per-particle variation of the same three keys |
| `color` | The same, RGBA. `alpha_mid` is where the alpha's middle key sits when it differs from the colour's |
| `orient` | `billboard` · `tail` · `emission` · `world` · `single` · `ground` · `ground_tail` · `emitter` · `collision` · `ray` · `tail_alt` |
| `tail` | Present for the stretched orientations. `mode` is `free` · `clamp` · `fix` |
| `flipbook` | Sprite-sheet atlas. The frame starts somewhere in `start` and walks to somewhere in `end` over `lifespan_factor` of the life |
| `blend` | `opaque` · `blend` · `add` · `multiply` |
| `space` | `local` (particles ride the emitter) or `world` (detached once spawned) |
| `model_particles` | The emitter draws a **mesh** per particle. The glTF carries no such mesh — skip the emitter rather than drawing quads |
| `trail_of` | This emitter trails the particles of emitter *n* in the same file |

### `"kind": "light"`

```json
{"m3fx": {"kind": "light", "light": "point", "color": [0.16, 0.64, 1],
          "intensity": 2, "range": 6, "range_near": 3,
          "inner_angle": 0.3, "outer_angle": 0.9}}
```

`light` is `point` or `spot`; the cone angles appear only for a spot. `intensity`
is M3's own artist-facing number with no photometric meaning — scale it to taste.
Like an emitter's rate, it is taken from the animation curve when the static
value is zero.

### `"kind": "decal"`

```json
{"m3fx": {"kind": "decal", "projection": "ortho", "size": [9.5, 9.5, 3],
          "euler": [0, 0, 0], "alpha": [0, 1, 0],
          "envelope": {"attack": [0.33, 0.33], "hold": [20, 20], "decay": [1, 1]},
          "texture": "#Texture3", "blend": "blend"}}
```

A ground projection — a cast indicator, an AoE ring. `size` is the projection box
in bone-local units, and the node carries the projection's offset as its own
translation. Alpha runs an attack → hold → decay envelope rather than a lifetime;
each stage is a `[min, max]` range in seconds. These map onto Bevy's decal
support, not onto a particle system.

## Limits

* Only `PAR_` versions 22, 23 and 24 are read — Heroes of the Storm, War3
  Reforged and Legacy of the Void. Older versions grew in too many places to
  splice into the v24 layout and are skipped with a warning.
* Colour curves are not resolved from animation data (only rate, burst, speed,
  lifetime, radius and the size/rotation vec3s are).
* `PARC` particle copies, `RIB_` ribbons, forces (`FOR_`) and warps (`WRP_`) are
  not exported.
* Effect textures raise `UNUSED_OBJECT` at info severity in the glTF validator.
  That is accurate — nothing in the scene draws with them; the runtime does.
