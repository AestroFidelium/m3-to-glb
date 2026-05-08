{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      flake-utils,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };

        # Stable with the components we need.
        rustStable = pkgs.rust-bin.stable.latest.default.override {
          extensions = [
            "rust-src" # required by rust-analyzer
            "llvm-tools-preview" # required by cargo-llvm-lines, cargo-show-asm
          ];
        };

        # Nightly — for cranelift, polonius, std::simd, optimize_attribute.
        rustNightly = pkgs.rust-bin.nightly.latest.default.override {
          extensions = [
            "rust-src"
            "llvm-tools-preview"
            "rustc-codegen-cranelift-preview" # fast dev compiles
          ];
        };

        # Minimal nightly for reproducible package builds
        # (no cranelift / llvm-tools — release doesn't need them).
        rustBuild = pkgs.rust-bin.nightly.latest.default;

        rustPlatform = pkgs.makeRustPlatform {
          cargo = rustBuild;
          rustc = rustBuild;
        };

        m3-to-glb = rustPlatform.buildRustPackage {
          pname = "m3-to-glb";
          version = "0.1.0";
          src = pkgs.lib.cleanSource ./.;
          cargoLock.lockFile = ./Cargo.lock;

          # `.cargo/config.toml` pins clang + mold as the linker.
          # `installShellFiles` ships the zsh completions emitted by build.rs.
          nativeBuildInputs = [
            pkgs.clang
            pkgs.mold
            pkgs.installShellFiles
          ];

          # `build.rs` generates `completions/_m3-to-glb` next to the source.
          # Drop it into `$out/share/zsh/site-functions/` — the standard fpath.
          postInstall = ''
            installShellCompletion --zsh completions/_m3-to-glb
          '';

          # `nix run` shows the binary name via mainProgram.
          meta = {
            description = "Fast Rust converter from Blizzard M3 (StarCraft II / Heroes of the Storm) to glTF 2.0 Binary";
            homepage = "https://github.com/AestroFidelium/m3-to-glb";
            license = pkgs.lib.licenses.gpl2Only;
            mainProgram = "m3-to-glb";
            platforms = pkgs.lib.platforms.unix;
          };
        };

      in
      {
        packages.default = m3-to-glb;
        packages.m3-to-glb = m3-to-glb;

        apps.default = {
          type = "app";
          program = "${m3-to-glb}/bin/m3-to-glb";
        };
        apps.m3-to-glb = self.apps.${system}.default;

        devShells.default = pkgs.mkShell {
          nativeBuildInputs = [
            rustNightly # swap for rustStable if nightly isn't needed

            # Linker — speeds up dev `cargo build` by 5-10x.
            pkgs.mold
            pkgs.clang # used as the frontend for mold

            # cargo profiling / dev tooling
            pkgs.cargo-flamegraph # CPU profile
            pkgs.cargo-show-asm # `cargo asm` — inspect what the compiler emitted
            pkgs.cargo-llvm-lines # find what bloats the binary
            pkgs.cargo-nextest # fast test runner (up to 3x faster than cargo test)
            pkgs.cargo-watch # watcher for scripts and CI
            pkgs.bacon # smart watcher with a TUI
            pkgs.cargo-tarpaulin # code coverage (Linux only)

            # System profiling tools
            pkgs.linuxPackages.perf # perf stat, perf record
            pkgs.heaptrack # heap profiler with flamegraph view

            # LLVM tooling
            pkgs.llvmPackages.llvm # llvm-profdata, llvm-bolt, etc.

            # System dependencies
            pkgs.pkg-config
            pkgs.openssl.dev
          ];

          shellHook = ''
            # mold linker via clang
            export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="${pkgs.clang}/bin/clang"
            export RUSTFLAGS="-C link-arg=-fuse-ld=${pkgs.mold}/bin/mold"

            echo "Rust dev env ready (nightly + mold)"
          '';
        };
      }
    );
}
