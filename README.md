# m3-to-glb

[![CI](https://github.com/AestroFidelium/m3-to-glb/actions/workflows/ci.yml/badge.svg)](https://github.com/AestroFidelium/m3-to-glb/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/m3-to-glb.svg)](https://crates.io/crates/m3-to-glb)
[![docs.rs](https://img.shields.io/docsrs/m3-to-glb)](https://docs.rs/m3-to-glb)
![line coverage](https://img.shields.io/badge/line%20coverage-99.8%25-brightgreen)
![unsafe](https://img.shields.io/badge/unsafe-forbidden-success)
![MSRV](https://img.shields.io/badge/MSRV-1.88-blue)
[![license](https://img.shields.io/badge/license-GPL--2.0--only-blue)](LICENSE)

**Converts Blizzard's M3 models — StarCraft II and Heroes of the Storm — into
glTF 2.0 binary (`.glb`): geometry, PBR materials with embedded textures, the
skeleton and skin, every animation clip, and the particle systems, lights,
decals and attachment points that glTF has no words for.**

A Rust library and a command-line tool. M3 is an undocumented, versioned binary
format; this reads it zero-copy, decodes vertices with runtime-dispatched SIMD,
converts mesh divisions in parallel, and writes a GLB that an independent glTF
loader accepts — for every one of the 33,365 models that ship with StarCraft II.

```console
$ m3-to-glb Ultralisk_Remastered.m3 -t textures/
✓ Ultralisk_Remastered.m3 → Ultralisk_Remastered.glb (1 mesh, 60 bones, 30 animations, …)
```

<!-- TODO: docs/demo.gif — a converted hero playing its walk cycle in a glTF viewer -->

| | |
|---|---|
| **Real-world corpus** | All 33,365 `.m3` files in StarCraft II convert; 0 produce an invalid glTF (checked by the `gltf` crate plus range and hierarchy checks). 75 s for the whole set. |
| **Speed** | 5 ms for a Heroes of the Storm hero, 61 ms for the largest SC2 model (9.5 MB, 7 MB of output) — whole process, measured with `hyperfine`. |
| **Safety** | The library is `#![forbid(unsafe_code)]`. Every input byte is treated as hostile: malformed files are errors, never panics. |
| **Testing** | ~130 tests, 99.8 % line coverage, a structure-aware libFuzzer target over the whole pipeline. [How it is tested ↓](#how-it-is-tested) |
| **Toolchain** | Stable Rust ≥ 1.88. Nightly is used only to speed up local builds. |

## What it converts

- **Geometry** — positions, normals, tangents with handedness, UVs. The vertex
  layout is not fixed: a 32-bit flag word in the model header decides which of
  ~30 components each vertex carries and in what encoding.
- **Materials** — `MAT_` (explicit diffuse / normal / emissive / AO slots,
  blend and alpha-test modes, two-sided flag, flat-colour layers), `MADD`
  (newer HotS heroes; slots routed by file-name suffix) and `CMP_` composites.
  Textures are found by name in a directory you point at and embedded.
- **Skeleton and skin** — bones, inverse bind matrices derived from the rest
  pose, up to four weighted joints per vertex.
- **Animations** — every clip in the model and in companion `.m3a` files, as
  glTF translation / rotation / scale channels.
- **Effects** — particle systems (`PAR_`), lights (`LITE`) and ground decals
  (`PROJ`) as empty nodes parented to the bone they ride, with their full
  parameter set — including curves resolved from the animation data — in the
  node's `extras`. See [docs/fx-extras.md](docs/fx-extras.md); a drop-in Bevy
  runtime for them is in [integration/bevy](integration/bevy).
- **Attachment points** — `Ref_Head`, `Ref_Weapon Right` and friends, written
  onto the bone node that carries them, with their hit volumes
  ([docs/attachments.md](docs/attachments.md)).
- **Rest-pose visibility** — geometry the game hides until an animation shows
  it (a weapon trail, a death effect) is left out, matching the in-game idle look.

## Why this is harder than it looks

- **Nothing is officially documented.** Everything known about M3 comes from years of
  community reverse engineering. Tag names are stored byte-reversed (`DIV_` is
  `_VID` on disk), and the same record has different sizes in different files:
  four `MAT_` layouts, five `LAYR` layouts, four region layouts, three particle
  layouts that have to be spliced together field by field.
- **Compressed vertex data.** Normals and tangents are packed into bytes, and
  the tangent's handedness lives in the *normal's* fourth byte. UVs are 16-bit
  integers scaled per region.
- **Two coordinate systems.** M3 is Z-up, glTF Y-up. The rotation has to be
  baked consistently into vertices, root bones, every root-bone animation key
  and the bounding box — and the inverse bind matrices have to be derived from
  the bone hierarchy, because the matrices stored in the file are not glTF IBMs.
- **Indirect animation data.** A bone's track is found through its animation
  id → the clip's id table → a packed `(type << 16 | index)` reference → one of
  thirteen typed keyframe arrays. The per-track "animated" flag is zero on
  animated bones and cannot be trusted.
- **glTF is strict.** Joints with zero weight must be joint 0; a skinned mesh
  must be a scene root; all joints need a common ancestor; quaternions must be
  unit length within the validator's tolerance; empty arrays are forbidden.

## Library

```toml
[dependencies]
m3-to-glb = { version = "0.2", default-features = false }  # no CLI dependencies
```

```rust
use m3_to_glb::{Converter, TextureCache};

let model = std::fs::read("Storm_Hero_Anduin_Base.m3")?;
let anims = std::fs::read("Storm_Hero_Anduin_RequiredAnims.m3a")?;
let textures = TextureCache::build("textures/")?;

let glb = Converter::new()
    .animations(&anims)
    .textures(&textures)
    .convert(&model)?;

std::fs::write("anduin.glb", &glb.bytes)?;
println!("{} bones, {} clips", glb.stats.bones, glb.stats.animations);
```

`Converter::convert` works on bytes in memory — no file system needed — and
returns a typed [`Error`](https://docs.rs/m3-to-glb/latest/m3_to_glb/enum.Error.html)
that tells a broken model from a broken animation file. The lower-level stages
(`m3::parse`, `processor::convert_all_meshes`, `glb::pack`) are public too.

## Command line

Install from crates.io, grab a binary for Linux / Windows / macOS from
[Releases](https://github.com/AestroFidelium/m3-to-glb/releases), or run it
through Nix without installing anything:

```bash
cargo install m3-to-glb
nix run github:AestroFidelium/m3-to-glb -- model.m3 -t /path/to/textures
```

### CLI reference

```bash
m3-to-glb INPUT [-o OUT.glb] [-t TEXTURE_DIR] [-a ANIM.m3a ...] [-q | -v LEVEL]
m3-to-glb --completions zsh > ~/.zfunc/_m3-to-glb     # bash, fish, zsh, elvish, powershell
```

| Flag                   | Meaning                                                                         |
| ---------------------- | ------------------------------------------------------------------------------- |
| `INPUT`                | Path to the `.m3` file. Required.                                               |
| `-o`, `--output`       | Output `.glb` path. Defaults to `INPUT` with a `.glb` extension.                |
| `-t`, `--textures`     | Directory holding `.png` / `.dds` / `.tga` textures. Walked recursively, indexed by xxh3 of the lowercase stem. |
| `-a`, `--anims`        | Companion `.m3a` animation file. Repeatable. HotS heroes ship animations separately from the base model. |
| `-q`, `--quiet`        | Suppress all output except errors. Conflicts with `-v`. Useful for batch scripts. |
| `--ktx2`               | Transcode every texture to KTX2/UASTC + Zstd (with mipmaps) and emit the `KHR_texture_basisu` glTF extension. OETF is tagged per material slot: `sRGB` for baseColor / emissive, `linear` for normal / occlusion / data channels. Massive VRAM savings in engines that transcode at load time (Bevy, three.js, Babylon). Requires [`toktx`](https://github.com/KhronosGroup/KTX-Software) on PATH — already bundled when running through Nix. |
| `--bevy-compat`        | **Non-spec workaround for Bevy 0.17.** Requires `--ktx2`. Drops the `KHR_texture_basisu` extension declaration and references KTX2 images via the standard `texture.source` field with `mimeType: "image/ktx2"`. Bevy's `bevy_image` (with `ktx2` + `basis-universal` features) decodes by MIME type, but only when the extension is absent. The output is **not valid glTF** — Blender, three.js and the Khronos validator will reject it. Do not use for anything other than a Bevy 0.17.x target. |
| `--max-tex-size <PX>`  | Cap each embedded texture so its largest dimension does not exceed `PX` pixels (aspect-preserving Lanczos3 resize). Applied before encoding in both the `--ktx2` and the raw-embed paths. Default `0` = no resize. Useful when the model sits far from camera and a 2K/4K source texture would just waste VRAM. In the raw-embed path, resized textures are re-encoded as PNG; without `--max-tex-size` source bytes are still passed through verbatim. |
| `--no-fx`              | Do not export effects. By default every particle system (`PAR_`), light (`LITE`) and projection (`PROJ`) becomes an empty node parented to the bone it rides on, carrying its parameters in that node's `extras` under the key `m3fx` — glTF has no particle systems of its own, so this is how the effect travels. See [docs/fx-extras.md](docs/fx-extras.md). |
| `-v`, `--verbose`      | Log level: `off`, `error`, `warn` (default), `info`, `debug`, `trace`. Same effect as `RUST_LOG=<level>`. |
| `--completions <SHELL>` | Print a completion script for `bash`, `fish`, `zsh`, `elvish` or `powershell` and exit. |

By default the converter prints a single one-line summary per file
on success (or the error on failure). Pass `-v info` for the
stage-by-stage trace, or `-q` for fully silent batch runs.

#### More examples

Doodad — geometry plus textures, no skeleton, no animations:

```bash
m3-to-glb Storm_Doodad_DS19_Buildings_11.m3 \
    -t /path/to/textures
```

Skinned hero with a companion animation file
(StarCraft II / older HotS, `MAT_` materials):

```bash
m3-to-glb Storm_Hero_Anduin_Base.m3 \
    -t /path/to/textures \
    -a Storm_Hero_Anduin_RequiredAnims.m3a
```

Newer HotS hero (`MADD` materials, no `MAT_`):

```bash
m3-to-glb Storm_Hero_Tracer_Base.m3 \
    -o tracer.glb \
    -t /path/to/textures \
    -a Storm_Hero_Tracer_RequiredAnims.m3a
```

Ability effect — no geometry at all, just emitters riding animated
bones:

```bash
m3-to-glb Storm_FX_Jaina_Base_RingofFrost.m3 \
    -t /path/to/textures
# → 4 particle systems, 1 light and 1 projection as `extras`-carrying nodes
```

Multiple animation files at once:

```bash
m3-to-glb hero.m3 \
    -a hero_RequiredAnims.m3a \
    -a hero_Combat.m3a \
    -a hero_Spell.m3a
```

Verbose tracing of the parse / convert / pack pipeline:

```bash
m3-to-glb model.m3 -t ./textures -v debug
# equivalently:
RUST_LOG=debug m3-to-glb model.m3 -t ./textures
```

Batch conversion — silent except on error:

```bash
for f in *.m3; do
    m3-to-glb "$f" -t ./textures -q -o "models/${f%.m3}.glb"
done
```

Bevy-friendly output — KTX2/UASTC textures stay GPU-compressed at runtime:

```bash
m3-to-glb hero.m3 -t ./textures --ktx2 -a hero_anims.m3a
```

A 2048×2048 base-colour map that costs 16 MiB of VRAM as raw RGBA8
drops to roughly 4 MiB as transcoded BC7 / ASTC. For HotS-sized scenes
the savings can be hundreds of MiB.

All textures go through UASTC + Zstd. Bevy 0.17's `bevy_image` accepts
only `None` and `Zstandard` KTX2 supercompression schemes; the more
compact ETC1S mode lives behind `BasisLZ` supercompression which Bevy
rejects, so UASTC is the one path that actually decodes there. UASTC
is also nearly lossless per channel, which is what tangent-space normal
maps need anyway.

The OETF tag is the one knob that varies by material slot:

  - `baseColor` / `emissive` → `sRGB` — the GPU sampler gamma-decodes
    at sample time, which is what color textures want.
  - `normal` / `occlusion` / data channels → `linear` — without this
    the sampler would gamma-decode the data and produce subtly wrong
    tangent-space lighting.

Pair it with `--max-tex-size` to cap source-texture dimensions before
encoding — useful for top-down views where a 2K source ends up covering
~100 px on screen:

```bash
m3-to-glb hero.m3 -t ./textures --ktx2 --max-tex-size 512
```

Bevy 0.17 itself ships without `KHR_texture_basisu` support
(see [`bevy_gltf` loader](https://github.com/bevyengine/bevy/blob/v0.17.3/crates/bevy_gltf/src/loader/mod.rs)).
For that engine specifically, add `--bevy-compat`:

```bash
m3-to-glb hero.m3 -t ./textures --ktx2 --bevy-compat -a hero_anims.m3a
```

This emits a non-canonical glTF: KTX2 bytes still ride in the buffer,
but the texture references them through the standard `source`/`mimeType`
path so Bevy's `bevy_image` picks them up. The file will fail any
spec-compliant validator (Blender, three.js, glTF-Validator) — only use
this flag when the consumer is Bevy 0.17.x.

#### MADD texture naming

`MADD` materials (mat_type 12, used by newer HotS heroes such as
Tracer) carry a flat list of texture paths with no slot tags. Slots
are routed by **filename suffix**:

| Suffix                        | Slot                  |
| ----------------------------- | --------------------- |
| `_diff`                       | `baseColorTexture`    |
| `_norm`                       | `normalTexture`       |
| `_emis` / `_emis1` / `_emis2` | `emissiveTexture`     |
| `_ao`                         | `occlusionTexture`    |
| `_spec`                       | ignored (no PBR slot) |

This matches Blizzard's HotS naming convention. `MAT_` materials use
their explicit slot fields and ignore filename suffixes.

## How it is tested

Blizzard's models cannot be committed to a public repository, so the test suite
**writes its own M3 files**. [`tests/common`](tests/common/mod.rs) describes a
model in plain terms — vertices, regions, bones, materials, animation tracks,
effects — and lays it out as a real M3: tag table, aligned payloads,
cross-references, in every tag version the reader supports.

Every conversion in the suite is held to the same **oracle**: correct GLB
framing, a JSON chunk that parses, acceptance by the independent
[`gltf`](https://crates.io/crates/gltf) crate (Bevy's loader), and the checks
that crate does not make — every accessor inside its buffer view, every index
inside its primitive's vertices, strictly increasing animation time, and a node
hierarchy that is an acyclic forest.

**Structure-aware fuzzing.** A raw-bytes fuzzer almost never gets past the tag
table: 20 seconds of it cover 223 code edges and stop there. The
[`fuzz_whole_pipeline`](tests/fuzz_pipeline.rs) target instead decodes the
fuzzer's input into a whole model description, builds a valid M3 from it,
corrupts it byte by byte, converts it and runs the oracle — 7,700 edges in five
minutes, reaching every stage of the pipeline. It runs for two minutes on every
push and for twenty every night in CI.

**Real files.** `M3_CORPUS=/path/to/extracted/game cargo test --release --test
corpus -- --ignored` runs every model under a directory through the converter
and the oracle. `gltf-transform validate out.glb` or a viewer such as
[gltf-viewer.donmccurdy.com](https://gltf-viewer.donmccurdy.com) is the manual
counterpart.

Bugs this found in code that had already been validated by hand on real models:

| Bug | Found by |
|---|---|
| A corrupt region count was passed to `Vec::with_capacity` — one input asked for 285 GB and aborted the process | fuzzer |
| Names were written with Rust's `{:?}`; a control character in a bone name produced invalid JSON and an unreadable file | oracle |
| `NaN` in a vertex or bone transform reached the JSON as `NaN` | oracle |
| A bone parented to itself or to a later bone made the node hierarchy a cycle | oracle |
| Triangles indexing outside their region, and keyframes stepping back in time, were passed through | oracle |
| A model with nothing visible got `"nodes": []` and a scene pointing at a node that does not exist — 6 of 358 sampled SC2 models | corpus diff |
| Three `unsafe` unaligned reads of `#[repr(C)]` structs out of byte buffers — undefined behaviour | `forbid(unsafe_code)` audit |
| Two attachment points on one bone: the last won, though the code meant the first | unit test |

```bash
cargo test --all-features                                        # everything, incl. a fuzz replay
cargo bolero test --profile fuzz fuzz_whole_pipeline -T 10min    # real libFuzzer (needs cargo-bolero)
CARGO_PROFILE_DEV_CODEGEN_BACKEND=llvm cargo llvm-cov --all-features   # coverage
```

## Limitations

Stated plainly, so nobody has to find them the hard way:

- **Effects are data, not pictures.** glTF has no particle systems; emitters,
  lights and decals travel as `extras` for an engine to spawn. A generic viewer
  shows empty nodes. ([Bevy runtime](integration/bevy/m3fx.rs).)
- **DDS / TGA textures are embedded as they are.** Core glTF allows only PNG
  and JPEG. Engines that read DDS by MIME type (Bevy, three.js with a loader)
  are fine; for strict validators use `--ktx2`, or convert the texture folder
  to PNG first.
- **One material per region.** M3 can bind several batches to one region; the
  first one's material is used, the rest are visibility metadata.
- **Composite materials** (`CMP_`) collapse to their first section; volume,
  displacement and terrain materials are skipped.
- **Rest-pose visibility only.** Geometry toggled by animations is resolved for
  the idle pose; glTF cannot animate visibility.
- **Not exported:** layer UV tiling and UV animation, material colour
  animation, ribbons, physics shapes, cameras.
- **`--bevy-compat` output is not valid glTF**, by design — see the [CLI reference](#cli-reference).

## Credits

This project would not exist without the people who reverse-engineered
the M3 format and published their findings.

- [**Solstice245/m3studio**](https://github.com/Solstice245/m3studio) —
  Blender add-on used as the behavioural reference. Vertex
  description, material slot layout, animation lookup logic and the
  `key_fcurves` filtering rules all trace back to this codebase.
- [**SC2Mapster/m3addon**](https://github.com/SC2Mapster/m3addon) —
  original `structures.xml`, the canonical description of every M3
  tag and field used by this converter.

The `structures.xml` file in turn credits a long line of contributors
who reverse-engineered the format over the years:

> *Florian Köberle — who created the first version of this file by
> using existing descriptions of the m3 file format · NiNtoxicated —
> who made an m3 Exporter and Importer for 3ds max · Leruster — who
> helped improving the structure.xml file · Witchsong (libm3) — who
> made an M3 library and helped NiNtoxicated on sequence data · Teal
> — PHP M3 parser · Blue Isle Studios · Volcore — who helped figure
> out vertex flags · Sixen (sc2mapster.com) · der_Ton — MD5 work that
> M3 is similar to · MrMoonKr · Skizot · Phygit · ufoZ — original M2
> reverse engineer · the SC2Mapster community · CaptainD001
> (M3_Import) · Talv · TangorCraft (M3 Editor) · Solstice245 · Renee.*

If you reuse the XML or any code generated from it, please carry
those credits forward.

## License

**GPL-2.0-only** — see [`LICENSE`](LICENSE).

This matches the licence of `m3studio` and `m3addon`, the projects
that supplied the format knowledge this converter is built on.
