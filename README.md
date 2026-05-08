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

## Usage

```bash
m3-to-glb INPUT [-o OUT.glb] [-t TEXTURE_DIR] [-a ANIM.m3a ...] [-v LEVEL]
```

| Flag                   | Meaning                                                                         |
| ---------------------- | ------------------------------------------------------------------------------- |
| `INPUT`                | Path to the `.m3` file. Required.                                               |
| `-o`, `--output`       | Output `.glb` path. Defaults to `INPUT` with a `.glb` extension.                |
| `-t`, `--textures`     | Directory holding `.png` / `.dds` textures. Walked recursively, indexed by xxh3 of the lowercase stem. |
| `-a`, `--anims`        | Companion `.m3a` animation file. Repeatable. HotS heroes ship animations separately from the base model. |
| `-v`, `--verbose`      | Log level: `off`, `error`, `warn`, `info` (default), `debug`, `trace`. Same effect as `RUST_LOG=<level>`. |

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

TBD.
