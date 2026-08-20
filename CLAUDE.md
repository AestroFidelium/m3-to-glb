# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
cargo build                  # debug build
cargo build --release        # optimized build (LTO, single CGU, mimalloc)
cargo run -- <input.m3>      # convert with auto-derived output path
cargo run -- <input.m3> -o out.glb -t ./textures -v debug
cargo clippy
```

Requires nightly toolchain (defined in `rust-toolchain.toml`).

Enable verbose tracing via CLI flag `-v debug` or env var `RUST_LOG=debug`.

## Architecture

Five-stage pipeline in `main.rs::run_conversion()`:

1. **mmap** — `memmap2::MmapOptions::new().map(&file)` — zero-copy file access
2. **M3 parse** — `m3::parse(&mmap)` → `M3File<'data>` whose lifetime is tied to the mmap buffer
3. **Texture index** — `assets::TextureCache::build(dir)` — walks directory, hashes `stem.to_lowercase()` with xxh3
4. **Geometry convert** — `processor::convert_all_meshes(&m3)` — rayon parallel per Division, AoS vertex buffer → `MeshDataSoA` SoA layout, SIMD via `multiversion` (AVX2 / SSE4.1 / scalar)
5. **GLB pack** — `glb::pack_and_write(meshes, textures, m3, path)` — writes glTF 2.0 binary (JSON chunk + BIN chunk)

### Module map

| Path | Responsibility |
|---|---|
| `src/main.rs` | Pipeline orchestration, CLI wiring |
| `src/cli.rs` | `Cli` struct (clap) |
| `src/m3/mod.rs` | `parse()`, version detection (MD32/33/34), magic bytes |
| `src/m3/reader.rs` | `M3File<'data>` — tag navigation, geometry/material/layer accessors |
| `src/m3/structures.rs` | `#[repr(C)] + Pod` structs: `M3Header`, `TagEntry`, `Division`, `Region`, `Batch`, `Bone`, `Layer`, `Reference` |
| `src/processor/mod.rs` | `convert_all_meshes()`, `VertexOffsets::from_flags()` |
| `src/processor/soa.rs` | `MeshDataSoA` — SoA vertex arrays, `as_bytes*` serialisers |
| `src/processor/transform.rs` | SIMD position/normal/UV/tangent extraction |
| `src/fx/mod.rs` | `collect()` — `PAR_`/`LITE`/`PROJ` → effect nodes + `extras` JSON (see `docs/fx-extras.md`) |
| `src/fx/curves.rs` | `FxCurves` — emitter tracks resolved out of `STC_` (rate, burst, speed, …) |
| `src/glb/mod.rs` | Binary GLB assembler, material alpha/double-sided logic |
| `src/glb/json_builder.rs` | glTF JSON manifest builder |
| `src/assets/mod.rs` | `TextureCache` — xxh3-hashed filename index, path normalisation |

### Critical non-obvious details

**M3 tag names are stored little-endian (byte-reversed):**
`"DIV_"` → `b"_VID"`, `"BONE"` → `b"ENOB"`, `"U8__"` → `b"__8U"`, `"U16_"` → `b"_61U"`, etc.
All tag searches in `reader.rs` use these reversed byte literals.

**Vertex layout is dynamic** — `vertex_flags` field at offset 96 in the MODL tag determines which components are present and their sizes. `VertexOffsets::from_flags()` (`processor/mod.rs`) computes per-component byte offsets by walking the flags in field order. The stride comes from `M3File::vertex_stride()`.

**Version-dependent struct sizes** — several structs have different sizes depending on the tag version field:
- `MAT_` (tag `_TAM`): versions 15 / 16–18 / 19 / 20 → sizes 268 / 280 / 340 / 352 bytes; layer offsets within MAT_ also vary
- `LAYR`: versions 20–22 / 23 / 24–26 → different `uv_tiling` offset
- `REGN` (`Region`): versions 0–2 / 3–4 / 5+ → sizes 32 / 40 / 48 bytes

**Effects are not glTF anything** — glTF has no particle systems, so `PAR_` /
`LITE` / `PROJ` are exported as *empty nodes* parented to their bone, with the
parameters in the node's `extras` under `m3fx`. Engines read them from there
(Bevy surfaces them as a `GltfExtras` component). Two consequences worth
knowing: an effect-only model has **no geometry at all** — `convert_all_meshes`
returns empty and bones/animations are emitted anyway, because that is what
moves the emitters — and emitter values are mostly *animated*, so
`fx::curves` resolves the rate/burst/speed tracks out of `STC_` rather than
trusting the static defaults (which are usually zero).

`M3File` never casts `ModelHeader` directly — the actual data layout doesn't match; tags are navigated by searching `tags[]` for LE names.
