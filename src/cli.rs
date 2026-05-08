/// Определение CLI и кастомных стилей для --help.
use clap::builder::styling::{AnsiColor, Effects, Styles};
use clap::{ColorChoice, Parser};

/// Кастомный стиль: жёлтые заголовки, циановые флаги, зелёные плейсхолдеры
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
    long_about = "Конвертирует файлы формата M3 (StarCraft II / Heroes of the Storm) \
                  в бинарный glTF 2.0 (GLB). Использует SIMD, rayon и zero-copy IO \
                  для максимальной скорости.",
    styles  = cli_styles(),
    color   = ColorChoice::Auto,
)]
pub struct Cli {
    /// Путь к входному файлу (.m3)
    #[arg(value_name = "INPUT")]
    pub input: String,

    /// Путь к выходному файлу (.glb)
    /// Если не указан — используется имя входного файла с расширением .glb
    #[arg(short, long, value_name = "OUTPUT")]
    pub output: Option<String>,

    /// Папка с текстурами (.png / .dds)
    /// Будет индексирована рекурсивно с xxh3-хэшированием имён
    #[arg(short, long, value_name = "DIR")]
    pub textures: Option<String>,

    /// Файлы анимаций (.m3a) — могут быть указаны несколько раз
    /// Кости берутся из основного .m3, ключевые кадры из этих .m3a
    #[arg(short = 'a', long = "anims", value_name = "M3A")]
    pub anims: Vec<String>,

    /// Уровень детализации логов (off, error, warn, info, debug, trace)
    #[arg(short, long, default_value = "info", value_name = "LEVEL")]
    pub verbose: String,
}
