//! m3-to-glb — высокопроизводительный конвертер формата M3 → glTF Binary (GLB).
//!
//! Архитектурные принципы:
//!  - Data-Oriented Design: SoA для геометрии, горячие/холодные данные раздельно
//!  - Zero-copy: memmap2 + ссылки в mmap буфер, минимум Clone/ToOwned
//!  - SIMD: wide (stable) + multiversion runtime dispatch (AVX2 / SSE4.1)
//!  - Параллелизм: rayon для независимых мешей и текстур
//!  - Память: mimalloc глобально + bumpalo для временных аллокаций в парсере

// ─── Глобальный аллокатор ─────────────────────────────────────────────────────
// ВАЖНО: без этого крейт mimalloc подключён, но не используется!
use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;
// ─────────────────────────────────────────────────────────────────────────────

mod assets;
mod cli;
mod glb;
mod m3;
mod processor;

use anyhow::{Context, Result};
use cli::Cli;
use clap::Parser;
use color_print::cformat;
use std::io::stderr;
use tracing::{error, info, warn};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

fn init_tracing(verbosity: &str) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(verbosity));

    tracing_subscriber::registry()
        .with(
            fmt::layer()
                .with_writer(stderr)
                .with_ansi(true)
                .with_target(false)
                .compact(),
        )
        .with(filter)
        .init();
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    init_tracing(&cli.verbose);

    anstream::println!(
        "{}",
        cformat!(
            "<white><bold>[!]</bold></white> <magenta>Запуск конвертации:</magenta> <cyan>{}</cyan>",
            cli.input
        )
    );

    // Определяем выходной путь: либо явный, либо меняем расширение
    let output_path = cli.output.clone().unwrap_or_else(|| {
        let p = std::path::Path::new(&cli.input);
        p.with_extension("glb")
            .to_string_lossy()
            .into_owned()
    });

    if cli.output.is_none() {
        warn!(
            "Выходной файл не указан, использую: {}",
            output_path
        );
    }

    let texture_dir_owned: Option<String> = if cli.textures.is_some() {
        None // будем использовать cli.textures напрямую
    } else {
        // Автопоиск: ищем папку рядом с моделью (та же директория)
        let model_dir = std::path::Path::new(&cli.input)
            .parent()
            .unwrap_or(std::path::Path::new("."));
        let candidate = model_dir.to_string_lossy().into_owned();
        // Используем директорию только если там есть хоть один PNG/DDS/TGA
        let has_textures = std::fs::read_dir(&candidate)
            .ok()
            .and_then(|mut rd| {
                rd.find(|e| {
                    e.as_ref().ok().and_then(|e| {
                        let p = e.path();
                        p.extension()
                            .and_then(|ext| ext.to_str())
                            .map(|ext| matches!(ext.to_lowercase().as_str(), "png" | "dds" | "tga"))
                    }).unwrap_or(false)
                })
            })
            .is_some();
        if has_textures {
            warn!("--textures не указан, ищу текстуры рядом с моделью: {}", candidate);
            Some(candidate)
        } else {
            None
        }
    };

    let texture_dir: Option<&str> = cli.textures.as_deref()
        .or(texture_dir_owned.as_deref());

    let anim_paths: Vec<&str> = cli.anims.iter().map(|s| s.as_str()).collect();

    match run_conversion(&cli.input, &output_path, texture_dir, &anim_paths) {
        Ok(stats) => {
            anstream::println!(
                "{}",
                cformat!(
                    "<green><bold>✓ Готово!</bold></green> {} меш(ей), {} текстур → <cyan>{}</cyan>",
                    stats.mesh_count,
                    stats.texture_count,
                    output_path
                )
            );
            Ok(())
        }
        Err(e) => {
            error!("Ошибка конвертации: {:#}", e);
            std::process::exit(1);
        }
    }
}

/// Статистика завершённой конвертации
struct ConversionStats {
    mesh_count:    usize,
    texture_count: usize,
}

fn run_conversion(
    input:       &str,
    output:      &str,
    texture_dir: Option<&str>,
    anim_paths:  &[&str],
) -> Result<ConversionStats> {
    // ── Этап 1: Открываем файл через mmap (zero-copy) ────────────────────────
    info!("Открываю файл через mmap: {}", input);
    let file = std::fs::File::open(input)
        .with_context(|| format!("Не могу открыть файл: {input}"))?;
    let mmap = unsafe { memmap2::MmapOptions::new().map(&file) }
        .with_context(|| "Ошибка mmap")?;

    // mmap-ы для .m3a файлов с анимациями — храним до конца конвертации,
    // чтобы M3File мог ссылаться на их данные.
    let anim_mmaps: Vec<memmap2::Mmap> = anim_paths
        .iter()
        .map(|p| {
            info!("Открываю файл анимации через mmap: {}", p);
            let f = std::fs::File::open(p)
                .with_context(|| format!("Не могу открыть .m3a: {p}"))?;
            let mm = unsafe { memmap2::MmapOptions::new().map(&f) }
                .with_context(|| format!("Ошибка mmap для {p}"))?;
            Ok::<_, anyhow::Error>(mm)
        })
        .collect::<Result<_>>()?;

    // ── Этап 2: Парсинг M3 ───────────────────────────────────────────────────
    info!("Парсинг заголовков M3...");
    let m3_file = m3::parse(&mmap)
        .with_context(|| "Ошибка парсинга M3")?;

    let anim_files: Vec<m3::M3File<'_>> = anim_mmaps
        .iter()
        .zip(anim_paths.iter())
        .map(|(mm, p)| {
            m3::parse(mm).with_context(|| format!("Ошибка парсинга .m3a: {p}"))
        })
        .collect::<Result<_>>()?;

    // Полный дамп тегов для диагностики (только при -v debug)
    m3_file.dump_tags();

    info!(
        "M3: {} мешей, {} материалов, {} костей, {} файлов анимаций",
        m3_file.mesh_count(),
        m3_file.material_count(),
        m3_file.bone_count(),
        anim_files.len(),
    );

    // ── Этап 3: Поиск текстур ────────────────────────────────────────────────
    let texture_map = if let Some(dir) = texture_dir {
        info!("Индексирую текстуры из: {}", dir);
        assets::TextureCache::build(dir)
            .with_context(|| "Ошибка индексирования текстур")?
    } else {
        assets::TextureCache::empty()
    };

    let texture_count = texture_map.len();
    info!("Найдено {} текстур", texture_count);

    // ── Этап 4: Конвертация геометрии (SoA + SIMD + rayon) ──────────────────
    info!("Конвертирую геометрию...");
    let mesh_data = processor::convert_all_meshes(&m3_file)
        .with_context(|| "Ошибка конвертации геометрии")?;

    let mesh_count = mesh_data.len();

    // ── Этап 5: Сборка GLB ───────────────────────────────────────────────────
    info!("Собираю GLB...");
    let anim_refs: Vec<&m3::M3File<'_>> = anim_files.iter().collect();
    glb::pack_and_write(&mesh_data, &texture_map, &m3_file, &anim_refs, output)
        .with_context(|| "Ошибка сборки GLB")?;

    Ok(ConversionStats { mesh_count, texture_count })
}
