//! Paths that only run with verbose logging on, or with external tools
//! present.
//!
//! One test per binary on purpose: it installs a *global* tracing subscriber
//! (rayon worker threads log too, so a thread-local one would miss them) and
//! rewrites `PATH` to put a stand-in `toktx` on it — both process-wide.

mod common;

use common::*;
use m3_to_glb::{Converter, TextureCache};
use std::fmt::Debug;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

/// Enables every level and formats every field, so the arguments of each
/// `debug!` are evaluated exactly as they would be under `-v trace`.
struct Everything;

struct Format;
impl Visit for Format {
    fn record_debug(&mut self, _: &Field, value: &dyn Debug) {
        let _ = format!("{value:?}");
    }
}

impl Subscriber for Everything {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, span: &Attributes<'_>) -> Id {
        span.record(&mut Format);
        Id::from_u64(1)
    }
    fn record(&self, _: &Id, values: &Record<'_>) {
        values.record(&mut Format);
    }
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, event: &Event<'_>) {
        event.record(&mut Format);
    }
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}

/// A `toktx` stand-in: `mode` decides whether it succeeds (copying its input
/// to the requested output), fails, or succeeds without writing anything.
fn fake_toktx(dir: &std::path::Path, mode: &str) {
    let body = match mode {
        "ok" => "for a; do out=$prev; prev=$a; done; cp \"$prev\" \"$out\"",
        "fail" => "exit 3",
        "noexec" => "exit 0",
        _ => "exit 0",
    };
    let path = dir.join("toktx");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if mode == "noexec" { 0o644 } else { 0o755 };
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
    }
}

#[test]
fn verbose_logging_and_external_tools() {
    tracing::subscriber::set_global_default(Everything).unwrap();

    // Every stage, with logging on.
    for spec in [ModelSpec::quad(), animated(), with_effects(), {
        let mut s = ModelSpec::quad();
        s.mat_version = 42; // unknown MAT_ version → v20 layout, logged
        s.layr_version = 99;
        s.regn_version = 2;
        s
    }] {
        let glb = Converter::new().convert(&spec.build()).unwrap();
        check_glb(&glb.bytes).unwrap();
        let m3 = spec.build();
        m3_to_glb::parse(&m3).unwrap().dump_tags();
    }

    // KTX2 through a stand-in `toktx`.
    #[cfg(unix)]
    {
        let tex = tempfile::tempdir().unwrap();
        for name in ["quad_diff.png", "quad_norm.png", "quad_ao.png"] {
            image::RgbaImage::from_pixel(8, 8, image::Rgba([10, 200, 30, 90])).save(tex.path().join(name)).unwrap();
        }
        let cache = TextureCache::build(tex.path().to_str().unwrap()).unwrap();
        let mut spec = ModelSpec::quad();
        let layer = |p: &str| LayerSpec { texture: p.into(), ..LayerSpec::default() };
        spec.materials[0].layers = vec![("diff", layer("quad_diff.dds")), ("norm", layer("quad_norm.dds")), ("ao", layer("quad_ao.dds"))];

        let tools = tempfile::tempdir().unwrap();
        let old_path = std::env::var_os("PATH").unwrap_or_default();
        let mut path = std::ffi::OsString::from(tools.path());
        path.push(":");
        path.push(&old_path);
        // SAFETY: this is the only test in this binary, so nothing else reads
        // the environment concurrently.
        unsafe { std::env::set_var("PATH", &path) };

        let mimes = |bevy: bool, max: u32| {
            let c = Converter::new().textures(&cache).ktx2(true).max_texture_size(max).options(m3_to_glb::PackOptions {
                ktx2: true,
                bevy_compat: bevy,
                max_tex_size: max,
                fx: true,
            });
            let glb = c.convert(&spec.build()).unwrap();
            let g = check_glb(&glb.bytes).unwrap();
            let v: Vec<String> = g.json["images"].as_array().unwrap().iter().map(|i| i["mimeType"].as_str().unwrap().to_owned()).collect();
            (v, g.json.get("extensionsRequired").is_some())
        };

        fake_toktx(tools.path(), "ok");
        let (m, required) = mimes(false, 4);
        assert!(m.iter().all(|m| m == "image/ktx2"), "{m:?}");
        assert!(required);
        let (_, required) = mimes(true, 64);
        assert!(!required, "--bevy-compat must not declare KHR_texture_basisu");

        fake_toktx(tools.path(), "fail");
        assert!(mimes(false, 0).0.iter().all(|m| m == "image/png"));

        fake_toktx(tools.path(), "silent");
        assert!(mimes(false, 0).0.iter().all(|m| m == "image/png"));

        // Present but not executable: a spawn error other than "not found".
        fake_toktx(tools.path(), "noexec");
        assert!(mimes(false, 0).0.iter().all(|m| m == "image/png"));

        // The fallback itself failing drops the texture.
        std::fs::write(tex.path().join("quad_diff.png"), b"not an image").unwrap();
        let (m, _) = mimes(false, 4);
        assert_eq!(m.len(), 2, "{m:?}");

        // SAFETY: as above — single-test binary.
        unsafe { std::env::set_var("PATH", old_path) };
    }
}
