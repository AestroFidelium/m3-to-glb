//! m3-to-glb — high-performance M3 → glTF Binary (GLB) converter.
//!
//! Architectural principles:
//!  - Data-Oriented Design: SoA geometry, hot/cold data separated
//!  - Zero-copy: memmap2 + references into the mmap buffer, minimal Clone/ToOwned
//!  - SIMD: wide (stable) + multiversion runtime dispatch (AVX2 / SSE4.1)
//!  - Parallelism: rayon over independent meshes and textures
//!  - Memory: mimalloc globally + bumpalo for short-lived parser allocations

// ─── Global allocator ────────────────────────────────────────────────────────
// IMPORTANT: without this the mimalloc crate is linked but never used.
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
use tracing::{error, info};
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

    // -q forces tracing down to errors only and silences the success print.
    let tracing_level = if cli.quiet { "error" } else { cli.verbose.as_str() };
    init_tracing(tracing_level);

    // Output path: explicit `--output` or fall back to `<input>.glb`.
    let output_path = cli.output.clone().unwrap_or_else(|| {
        let p = std::path::Path::new(&cli.input);
        p.with_extension("glb")
            .to_string_lossy()
            .into_owned()
    });

    let texture_dir_owned: Option<String> = if cli.textures.is_some() {
        None
    } else {
        // Auto-discover textures next to the model if a PNG/DDS/TGA is present.
        let model_dir = std::path::Path::new(&cli.input)
            .parent()
            .unwrap_or(std::path::Path::new("."));
        let candidate = model_dir.to_string_lossy().into_owned();
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
            info!("--textures not provided; using model directory: {}", candidate);
            Some(candidate)
        } else {
            None
        }
    };

    let texture_dir: Option<&str> = cli.textures.as_deref()
        .or(texture_dir_owned.as_deref());

    let anim_paths: Vec<&str> = cli.anims.iter().map(|s| s.as_str()).collect();

    let pack_options = glb::PackOptions {
        ktx2:        cli.ktx2,
        bevy_compat: cli.bevy_compat,
    };

    match run_conversion(
        &cli.input, &output_path, texture_dir, &anim_paths, &pack_options,
    ) {
        Ok(stats) => {
            if !cli.quiet {
                anstream::println!(
                    "{}",
                    cformat!(
                        "<green><bold>✓</bold></green> {} → <cyan>{}</cyan> <dim>({} mesh{}, {} texture{})</dim>",
                        cli.input,
                        output_path,
                        stats.mesh_count,
                        if stats.mesh_count == 1 { "" } else { "es" },
                        stats.texture_count,
                        if stats.texture_count == 1 { "" } else { "s" },
                    )
                );
            }
            Ok(())
        }
        Err(e) => {
            error!("conversion failed: {:#}", e);
            std::process::exit(1);
        }
    }
}

/// Stats returned by a successful conversion.
struct ConversionStats {
    mesh_count:    usize,
    texture_count: usize,
}

fn run_conversion(
    input:        &str,
    output:       &str,
    texture_dir:  Option<&str>,
    anim_paths:   &[&str],
    pack_options: &glb::PackOptions,
) -> Result<ConversionStats> {
    // ── Stage 1: open input via mmap (zero-copy) ─────────────────────────────
    info!("opening {} via mmap", input);
    let file = std::fs::File::open(input)
        .with_context(|| format!("cannot open {input}"))?;
    let mmap = unsafe { memmap2::MmapOptions::new().map(&file) }
        .with_context(|| "mmap failed")?;

    // mmaps for the .m3a animation files — kept alive for the entire conversion
    // so M3File can borrow into them.
    let anim_mmaps: Vec<memmap2::Mmap> = anim_paths
        .iter()
        .map(|p| {
            info!("opening animation {} via mmap", p);
            let f = std::fs::File::open(p)
                .with_context(|| format!("cannot open .m3a: {p}"))?;
            let mm = unsafe { memmap2::MmapOptions::new().map(&f) }
                .with_context(|| format!("mmap failed for {p}"))?;
            Ok::<_, anyhow::Error>(mm)
        })
        .collect::<Result<_>>()?;

    // ── Stage 2: parse M3 ────────────────────────────────────────────────────
    info!("parsing M3 headers");
    let m3_file = m3::parse(&mmap)
        .with_context(|| "M3 parse failed")?;

    let anim_files: Vec<m3::M3File<'_>> = anim_mmaps
        .iter()
        .zip(anim_paths.iter())
        .map(|(mm, p)| {
            m3::parse(mm).with_context(|| format!(".m3a parse failed: {p}"))
        })
        .collect::<Result<_>>()?;

    // Full tag dump for diagnostics (only at -v debug).
    m3_file.dump_tags();

    info!(
        "M3: {} meshes, {} materials, {} bones, {} anim file(s)",
        m3_file.mesh_count(),
        m3_file.material_count(),
        m3_file.bone_count(),
        anim_files.len(),
    );

    // ── Stage 3: index textures ──────────────────────────────────────────────
    let texture_map = if let Some(dir) = texture_dir {
        info!("indexing textures from {}", dir);
        assets::TextureCache::build(dir)
            .with_context(|| "texture indexing failed")?
    } else {
        assets::TextureCache::empty()
    };

    let texture_count = texture_map.len();
    info!("{} texture(s) indexed", texture_count);

    // ── Stage 4: convert geometry (SoA + SIMD + rayon) ──────────────────────
    info!("converting geometry");
    let mesh_data = processor::convert_all_meshes(&m3_file)
        .with_context(|| "geometry conversion failed")?;

    let mesh_count = mesh_data.len();

    // ── Stage 5: pack GLB ────────────────────────────────────────────────────
    info!("packing GLB");
    let anim_refs: Vec<&m3::M3File<'_>> = anim_files.iter().collect();
    glb::pack_and_write(
        &mesh_data, &texture_map, &m3_file, &anim_refs, output, pack_options,
    )
    .with_context(|| "GLB packing failed")?;

    Ok(ConversionStats { mesh_count, texture_count })
}
