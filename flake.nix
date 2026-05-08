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

        # Stable с нужными компонентами
        rustStable = pkgs.rust-bin.stable.latest.default.override {
          extensions = [
            "rust-src" # нужен для rust-analyzer
            "llvm-tools-preview" # нужен для cargo-llvm-lines, cargo-show-asm
          ];
        };

        # Nightly — для cranelift, polonius, std::simd, optimize_attribute
        rustNightly = pkgs.rust-bin.nightly.latest.default.override {
          extensions = [
            "rust-src"
            "llvm-tools-preview"
            "rustc-codegen-cranelift-preview" # быстрая компиляция в dev
          ];
        };

      in
      {
        devShells.default = pkgs.mkShell {
          nativeBuildInputs = [
            rustNightly # или rustStable если не нужен nightly

            # линковщик — ускоряет cargo build в dev в 5-10x
            pkgs.mold
            pkgs.clang # нужен как frontend для mold

            # cargo-инструменты профилирования и разработки
            pkgs.cargo-flamegraph # CPU профиль
            pkgs.cargo-show-asm # cargo asm — смотреть что сгенерировал компилятор
            pkgs.cargo-llvm-lines # что раздувает бинарник
            pkgs.cargo-nextest # быстрый test runner (до 3x быстрее cargo test)
            pkgs.cargo-watch # watcher для скриптов и CI
            pkgs.bacon # умный watcher с TUI
            pkgs.cargo-tarpaulin # покрытие кода (только Linux)

            # системные инструменты профилирования
            pkgs.linuxPackages.perf # perf stat, perf record
            pkgs.heaptrack # heap профилировщик с flamegraph по памяти

            # LLVM инструменты
            pkgs.llvmPackages.llvm # llvm-profdata, llvm-bolt и др.

            # системные зависимости
            pkgs.pkg-config
            pkgs.openssl.dev
          ];

          shellHook = ''
            # mold линковщик через clang
            export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="${pkgs.clang}/bin/clang"
            export RUSTFLAGS="-C link-arg=-fuse-ld=${pkgs.mold}/bin/mold"

            echo "🦀 Rust dev env ready (nightly + mold)"
          '';
        };
      }
    );
}
