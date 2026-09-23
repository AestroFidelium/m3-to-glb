# Changelog

All notable changes. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versions follow [SemVer](https://semver.org/).

## [0.2.0] — 2026-09-23

The converter is now a library with a thin CLI on top, and every conversion is
checked against an independent glTF loader in the test suite.

### Added
- Library API: `Converter` (builder over model bytes, companion `.m3a` files,
  a texture index and packing options) → `Glb { bytes, stats }`, with a typed
  `Error`. `glb::pack` assembles a GLB in memory.
- `cli` cargo feature (default). `default-features = false` drops clap,
  mimalloc, memmap2 and the terminal logger.
- `--completions <SHELL>` (bash, fish, zsh, elvish, powershell).
- Test suite: synthetic M3 writer, GLB oracle, structure-aware libFuzzer target
  over the whole pipeline, CLI tests, opt-in real-corpus test (`M3_CORPUS`).
- CI: clippy, tests, stable + MSRV builds, coverage, fuzzing (per push and
  nightly); tagged releases build Linux / Windows / macOS binaries.

### Fixed
- Names from the file are escaped as JSON; a control character used to make the
  whole GLB unreadable.
- Non-finite floats never reach the JSON chunk.
- A model with nothing visible no longer declares an empty `nodes` array and a
  scene pointing at a missing node.
- A corrupt record count can no longer trigger a huge allocation.
- Bones whose parent does not precede them become roots instead of forming a
  cycle.
- Out-of-range triangles and backwards keyframes are dropped instead of
  emitted.
- Regions without valid faces no longer produce zero-length accessors.
- The first of two attachment points on one bone wins, as intended.
- Every mesh division gets a scene node, not only the first.
- Undefined behaviour: three unaligned reads of `#[repr(C)]` records; the
  library is now `#![forbid(unsafe_code)]`.

### Changed
- MSRV 1.88; the crate builds on stable (nightly remains the dev toolchain).
- `build.rs` removed — it wrote shell completions into the source tree.
- JSON numbers are written without a trailing `.0`.

## [0.1.0]

First version: geometry, `MAT_` / `MADD` materials, skeleton and skin,
animations, effects and attachment points as node `extras`, KTX2 textures.
