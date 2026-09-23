# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
cargo build                  # debug build
cargo build --release        # optimized build (LTO, single CGU, mimalloc)
cargo run -- <input.m3>      # convert with auto-derived output path
cargo run -- <input.m3> -o out.glb -t ./textures -v debug
cargo clippy --all-targets --all-features -- -D warnings   # CI gate: pedantic + restriction picks, must be clean
cargo test --all-features                                  # unit + integration + bolero replay
cargo bolero test --profile fuzz fuzz_whole_pipeline -T 5min   # real libFuzzer run
M3_CORPUS=/mnt/Projects/StarCraftExtracted/out cargo test --release --test corpus -- --ignored
```

Nightly (pinned in `rust-toolchain.toml`) is only for dev speed — cranelift +
mold in `.cargo/config.toml`. The crate builds on stable ≥ `rust-version`.

Coverage: `CARGO_PROFILE_DEV_CODEGEN_BACKEND=llvm cargo llvm-cov --all-features`
(instrumentation needs LLVM, not cranelift).

Enable verbose tracing via CLI flag `-v debug` or env var `RUST_LOG=debug`.

## Architecture

Library + thin binary. `lib.rs::Converter::convert(&[u8]) -> Glb` runs the
pipeline; `main.rs` only parses args, mmaps files (the one `unsafe` in the
repo — the library is `#![forbid(unsafe_code)]`) and prints stats. The `cli`
feature (default) gates clap, mimalloc, memmap2 and the terminal logger.

1. **M3 parse** — `m3::parse(bytes)` → `M3File<'data>` borrowing the input buffer
2. **Texture index** — `assets::TextureCache::build(dir)` — walks directory, hashes `stem.to_lowercase()` with xxh3
3. **Geometry convert** — `processor::convert_all_meshes(&m3)` — rayon parallel per Division, AoS vertex buffer → `MeshDataSoA` SoA layout, SIMD via `multiversion` (AVX2 / SSE4.1 / scalar)
4. **GLB pack** — `glb::pack(meshes, textures, m3, anims, opts) -> Vec<u8>` — glTF 2.0 binary (JSON chunk + BIN chunk)

### Module map

| Path | Responsibility |
|---|---|
| `src/lib.rs` | Public API: `Converter`, `Glb`, `Stats`, `Error` |
| `src/main.rs` | CLI front end (mmap, logging, stats print, `--completions`) |
| `src/cli.rs` | `Cli` struct (clap) — part of the binary, not the library |
| `src/json.rs` | JSON string escaping / finite numbers / `Obj` writer shared by manifest + extras |
| `src/m3/mod.rs` | `parse()`, version detection (MD32/33/34), magic bytes |
| `src/m3/reader.rs` | `M3File<'data>` — tag navigation, geometry/material/layer accessors |
| `src/m3/structures.rs` | `#[repr(C)] + Pod` structs: `M3Header`, `TagEntry`, `Division`, `Region`, `Batch`, `Bone`, `Layer`, `Reference` |
| `src/processor/mod.rs` | `convert_all_meshes()`, `VertexOffsets::from_flags()` |
| `src/processor/soa.rs` | `MeshDataSoA` — SoA vertex arrays, `as_bytes*` serialisers |
| `src/processor/transform.rs` | SIMD position/normal/UV/tangent extraction |
| `src/fx/mod.rs` | `collect()` — `PAR_`/`LITE`/`PROJ` → effect nodes + `extras` JSON (see `docs/fx-extras.md`) |
| `src/attach.rs` | `collect()` — `ATT_`/`ATVL` → `m3attach` extras on the bone node (see `docs/attachments.md`) |
| `src/fx/curves.rs` | `FxCurves` — emitter tracks resolved out of `STC_` (rate, burst, speed, …) |
| `src/glb/mod.rs` | `pack()` orchestration, mesh accessors, GLB framing |
| `src/glb/buffers.rs` | BIN buffer / views / accessors; `Images` (dedup cache, KTX2 fallback) |
| `src/glb/materials.rs` | MAT_ / MADD → glTF materials, effect material summaries, MADD suffix slots |
| `src/glb/scene.rs` | bone nodes + attachments, effect nodes, skin/IBM math, mesh nodes, armature |
| `src/glb/animation.rs` | clips → samplers/channels |
| `src/glb/json_builder.rs` | glTF JSON manifest (`Document` → one writer per section) |
| `src/assets/mod.rs` | `TextureCache` — xxh3-hashed filename index, path normalisation |
| `tests/common/mod.rs` | `ModelSpec` synthetic M3 writer + `check_glb` oracle (gltf crate + range/forest checks) |
| `tests/fuzz_pipeline.rs` | structure-aware bolero target: `FuzzModel` → M3 bytes → corrupt → convert → oracle |

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

**Attachment names are not bone names** — an attachment point is a bone plus a
name in `ATT_`, and the two usually match, but not always: `Ref_Target` rides a
bone called `Vol_Target`. Matching `Ref_*` node names instead of reading the
table silently loses exactly the volume attachments (`src/attach.rs`).

`M3File` never casts `ModelHeader` directly — the actual data layout doesn't match; tags are navigated by searching `tags[]` for LE names.

**Everything written to the JSON chunk comes from an untrusted file.** Strings
go through `json::string` (never `{:?}` — Rust's Debug escapes are not JSON),
numbers through `json::num` (NaN/inf → 0). Any count read from the file is
bounded by the bytes that remain before it reaches `Vec::with_capacity`.
