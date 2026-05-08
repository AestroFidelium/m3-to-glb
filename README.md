# m3-to-glb

A fast, pure-Rust converter from Blizzard's **M3** model format
(StarCraft II / Heroes of the Storm) to **glTF 2.0 Binary (GLB)**.

Goal: sub-100 ms per model, output that matches the Python+Blender
reference exactly — no heuristics, no guessing.

Features:

- Geometry — positions / normals / tangents / UVs (handles dynamic
  vertex layouts via `vertex_flags`)
- Materials — `MAT_` (PBR slots) and `MADD` (suffix-routed for newer
  HotS heroes)
- Skinning — bones, IBMs derived from the bone hierarchy, per-vertex
  joints / weights
- Animations — translation / rotation / scale tracks per bone, parsed
  from `SEQS` / `STG` / `STC` blocks; supports companion `.m3a` files
- Z-up → Y-up bake on rest pose, animations and AABB
- Zero-copy `mmap` parsing, SIMD geometry transforms (AVX2 / SSE4.1 /
  scalar), rayon-parallel mesh conversion

## Build

Requires the **nightly** Rust toolchain
(pinned by `rust-toolchain.toml`).

```bash
cargo build --release
```

Release profile uses fat LTO, single codegen unit, `mimalloc`,
`panic = "abort"` and stripped symbols.

## Run with Nix

The flake exposes both a runnable app and a buildable package, so you
can use the converter without cloning the repo or installing Rust at
all — flakes must be enabled in your Nix config (`experimental-features
= nix-command flakes`).

One-off run, fetched and built on demand:

```bash
nix run github:AestroFidelium/m3-to-glb -- model.m3 -t /path/to/textures
nix run github:AestroFidelium/m3-to-glb -- hero.m3 -a hero_anims.m3a -t ./textures
```

Build the binary into `./result/bin/m3-to-glb`:

```bash
nix build github:AestroFidelium/m3-to-glb
./result/bin/m3-to-glb model.m3 -t ./textures
```

Install into your user profile:

```bash
nix profile install github:AestroFidelium/m3-to-glb
m3-to-glb model.m3 -t ./textures
```

Drop into a development shell with nightly Rust, mold, cranelift and
the cargo profiling tools:

```bash
nix develop
```

## Usage

```bash
m3-to-glb INPUT [-o OUT.glb] [-t TEXTURE_DIR] [-a ANIM.m3a ...] [-q | -v LEVEL]
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
| `-v`, `--verbose`      | Log level: `off`, `error`, `warn` (default), `info`, `debug`, `trace`. Same effect as `RUST_LOG=<level>`. |

By default the converter prints a single one-line summary per file
on success (or the error on failure). Pass `-v info` for the
stage-by-stage trace, or `-q` for fully silent batch runs.

### Examples

Doodad — geometry plus textures, no skeleton, no animations:

```bash
cargo run --release -- Storm_Doodad_DS19_Buildings_11.m3 \
    -t /path/to/textures
```

Skinned hero with a companion animation file
(StarCraft II / older HotS, `MAT_` materials):

```bash
cargo run --release -- Storm_Hero_Anduin_Base.m3 \
    -t /path/to/textures \
    -a Storm_Hero_Anduin_RequiredAnims.m3a
```

Newer HotS hero (`MADD` materials, no `MAT_`):

```bash
cargo run --release -- Storm_Hero_Tracer_Base.m3 \
    -o tracer.glb \
    -t /path/to/textures \
    -a Storm_Hero_Tracer_RequiredAnims.m3a
```

Multiple animation files at once:

```bash
cargo run --release -- hero.m3 \
    -a hero_RequiredAnims.m3a \
    -a hero_Combat.m3a \
    -a hero_Spell.m3a
```

Verbose tracing of the parse / convert / pack pipeline:

```bash
cargo run --release -- model.m3 -t ./textures -v debug
# equivalently:
RUST_LOG=debug cargo run --release -- model.m3 -t ./textures
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

### MADD texture naming

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

## Validation

Validate the produced `.glb` with
[`gltf-transform`](https://gltf-transform.dev):

```bash
gltf-transform validate out.glb
```

Or inspect visually with
[gltf.report](https://gltf.report) or
[donmccurdy.com/gltf-viewer](https://gltf-viewer.donmccurdy.com).

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
