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

        # Минимальный nightly для воспроизводимой сборки пакета
        # (без cranelift / llvm-tools — они не нужны для release).
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

          # `.cargo/config.toml` требует clang + mold как линковщик.
          # `installShellFiles` — для установки zsh-подсказок из build.rs.
          nativeBuildInputs = [
            pkgs.clang
            pkgs.mold
            pkgs.installShellFiles
          ];

          # `build.rs` генерирует `completions/_m3-to-glb` рядом с исходниками.
          # Кладём в `$out/share/zsh/site-functions/` — стандартный fpath.
          postInstall = ''
            installShellCompletion --zsh completions/_m3-to-glb
          '';

          # `nix run` показывает имя через mainProgram.
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
