//! `m3-to-glb` command-line front end. All conversion logic lives in the
//! library; this file only parses arguments, opens files and reports.

// mimalloc: noticeably faster than the system allocator for the many short
// per-region Vecs the converter builds.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

mod cli;

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser};
use cli::Cli;
use color_print::cformat;
use m3_to_glb::{Converter, PackOptions, TextureCache};
use std::path::Path;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

fn init_tracing(verbosity: &str) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(verbosity));
    tracing_subscriber::registry()
        .with(fmt::layer().with_writer(std::io::stderr).with_target(false).compact())
        .with(filter)
        .init();
}

/// Map a file read-only.
///
/// Zero-copy: the parser borrows straight out of the page cache, so a model is
/// never copied before conversion.
fn map(path: &str) -> Result<memmap2::Mmap> {
    let file = std::fs::File::open(path).with_context(|| format!("cannot open {path}"))?;
    // SAFETY: the map is read-only and lives only for this process's
    // conversion. The one hazard of `Mmap` — another process truncating or
    // rewriting the file while it is mapped — cannot make the parser unsound:
    // it treats every byte as untrusted and bounds-checks every access.
    let map = unsafe { memmap2::Mmap::map(&file) };
    map.with_context(|| format!("cannot map {path}"))
}

/// Use the model's own directory as the texture root when `-t` is not given and
/// it holds any image files.
fn auto_texture_dir(input: &str) -> Option<String> {
    let dir = Path::new(input).parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let has_textures = std::fs::read_dir(dir).ok()?.flatten().any(|e| {
        e.path()
            .extension()
            .and_then(|x| x.to_str())
            .is_some_and(|x| matches!(x.to_ascii_lowercase().as_str(), "png" | "dds" | "tga"))
    });
    has_textures.then(|| dir.to_string_lossy().into_owned())
}

fn run(cli: &Cli, input: &str) -> Result<()> {
    let output = cli.output.clone().unwrap_or_else(|| {
        Path::new(input).with_extension("glb").to_string_lossy().into_owned()
    });

    let texture_dir = cli.textures.clone().or_else(|| {
        let dir = auto_texture_dir(input)?;
        info!("--textures not provided; using model directory: {dir}");
        Some(dir)
    });
    let textures = match &texture_dir {
        Some(dir) => TextureCache::build(dir).context("texture indexing failed")?,
        None => TextureCache::empty(),
    };

    let model = map(input)?;
    let anims = cli.anims.iter().map(|p| map(p)).collect::<Result<Vec<_>>>()?;

    let mut converter = Converter::new().textures(&textures).options(PackOptions {
        ktx2:         cli.ktx2,
        bevy_compat:  cli.bevy_compat,
        max_tex_size: cli.max_tex_size,
        fx:           !cli.no_fx,
    });
    for a in &anims {
        converter = converter.animations(a);
    }

    let glb = converter.convert(&model)?;
    std::fs::write(&output, &glb.bytes).with_context(|| format!("cannot write {output}"))?;

    if !cli.quiet {
        let s = glb.stats;
        let plural = |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        anstream::println!(
            "{}",
            cformat!(
                "<green><bold>✓</bold></green> {} → <cyan>{}</cyan> <dim>({}, {}, {}, {})</dim>",
                input,
                output,
                plural(s.meshes, "mesh", "meshes"),
                plural(s.bones, "bone", "bones"),
                plural(s.animations, "animation", "animations"),
                plural(s.textures, "texture", "textures"),
            )
        );
    }
    Ok(())
}

fn main() {
    let cli = Cli::parse();

    if let Some(shell) = cli.completions {
        clap_complete::generate(shell, &mut Cli::command(), "m3-to-glb", &mut std::io::stdout());
        return;
    }

    init_tracing(if cli.quiet { "error" } else { cli.verbose.as_str() });

    // clap guarantees INPUT whenever --completions is absent.
    let input = cli.input.as_deref().unwrap_or_default();
    if let Err(e) = run(&cli, input) {
        anstream::eprintln!("{}", cformat!("<red><bold>error:</bold></red> {:#}", e));
        std::process::exit(1);
    }
}
