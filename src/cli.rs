/// CLI definition and styling for `--help`.
use clap::builder::styling::{AnsiColor, Effects, Styles};
use clap::{ColorChoice, Parser};

/// Custom style: yellow headers, cyan flags, green placeholders.
pub fn cli_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Yellow.on_default() | Effects::BOLD)
        .usage(AnsiColor::Yellow.on_default() | Effects::BOLD)
        .literal(AnsiColor::Cyan.on_default() | Effects::BOLD)
        .placeholder(AnsiColor::Green.on_default())
        .error(AnsiColor::Red.on_default() | Effects::BOLD)
        .valid(AnsiColor::Green.on_default() | Effects::BOLD)
        .invalid(AnsiColor::Red.on_default() | Effects::BOLD)
}

#[derive(Parser, Debug)]
#[command(
    name    = "m3-to-glb",
    version,
    about   = "High-performance M3 → GLB converter",
    long_about = "Converts Blizzard M3 files (StarCraft II / Heroes of the Storm) \
                  into binary glTF 2.0 (GLB). Uses SIMD, rayon and zero-copy IO \
                  for maximum throughput.",
    styles  = cli_styles(),
    color   = ColorChoice::Auto,
)]
pub struct Cli {
    /// Path to the input `.m3` file
    #[arg(value_name = "INPUT")]
    pub input: String,

    /// Output `.glb` path. Defaults to the input path with a `.glb` extension.
    #[arg(short, long, value_name = "OUTPUT")]
    pub output: Option<String>,

    /// Texture directory (`.png` / `.dds` / `.tga`).
    /// Walked recursively, indexed by xxh3 of the lowercase stem.
    #[arg(short, long, value_name = "DIR")]
    pub textures: Option<String>,

    /// Companion animation files (`.m3a`) — repeatable.
    /// Bones come from the base `.m3`, keyframes from these.
    #[arg(short = 'a', long = "anims", value_name = "M3A")]
    pub anims: Vec<String>,

    /// Quiet mode — suppress everything except errors (exit code only).
    #[arg(short, long, conflicts_with = "verbose")]
    pub quiet: bool,

    /// Transcode every embedded texture to KTX2/UASTC + Zstd with mipmaps.
    /// Output uses the `KHR_texture_basisu` glTF extension. Drastically
    /// reduces VRAM in engines that transcode at load time (Bevy, three.js).
    /// Requires `toktx` (KTX-Software) on PATH.
    #[arg(long)]
    pub ktx2: bool,

    /// Non-spec workaround for Bevy 0.17: when used with `--ktx2`, emit KTX2
    /// images via standard `texture.source` + `mimeType:"image/ktx2"` instead
    /// of the `KHR_texture_basisu` extension. Bevy's `bevy_image` decodes the
    /// KTX2 by MIME type, but only when the extension is absent. The output
    /// is NOT a valid glTF — Blender, three.js and the Khronos validator
    /// will reject it. Use only when targeting Bevy 0.17.x.
    #[arg(long, requires = "ktx2")]
    pub bevy_compat: bool,

    /// Log level (off, error, warn, info, debug, trace). Default: warn.
    #[arg(short, long, default_value = "warn", value_name = "LEVEL")]
    pub verbose: String,
}
