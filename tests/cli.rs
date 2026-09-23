//! The `m3-to-glb` binary, driven as a user would drive it.
#![cfg(feature = "cli")]

mod common;

use common::*;
use std::path::Path;
use std::process::{Command, Output};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_m3-to-glb"))
}

fn run(args: &[&str], cwd: &Path) -> Output {
    bin().args(args).current_dir(cwd).env_remove("RUST_LOG").output().expect("spawn m3-to-glb")
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn write_quad(dir: &Path, name: &str) {
    std::fs::write(dir.join(name), ModelSpec::quad().build()).unwrap();
}

#[test]
fn help_and_version() {
    let tmp = tempfile::tempdir().unwrap();
    let out = run(&["--help"], tmp.path());
    assert!(out.status.success());
    assert!(text(&out.stdout).contains("--textures"));
    let out = run(&["--version"], tmp.path());
    assert!(text(&out.stdout).contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn completions_need_no_input() {
    let tmp = tempfile::tempdir().unwrap();
    let out = run(&["--completions", "zsh"], tmp.path());
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(text(&out.stdout).starts_with("#compdef m3-to-glb"));
    // …but INPUT is required otherwise.
    assert!(!run(&[], tmp.path()).status.success());
}

#[test]
fn converts_next_to_the_input_by_default() {
    let tmp = tempfile::tempdir().unwrap();
    write_quad(tmp.path(), "quad.m3");
    let out = run(&["quad.m3"], tmp.path());
    assert!(out.status.success(), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(stdout.contains("quad.glb") && stdout.contains("1 mesh"), "{stdout}");
    check_glb(&std::fs::read(tmp.path().join("quad.glb")).unwrap()).unwrap();
}

#[test]
fn picks_up_textures_beside_the_model() {
    let tmp = tempfile::tempdir().unwrap();
    write_quad(tmp.path(), "quad.m3");
    image::RgbaImage::new(2, 2).save(tmp.path().join("quad_diff.png")).unwrap();
    let out = run(&["quad.m3", "-o", "out.glb", "-v", "info"], tmp.path());
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(text(&out.stderr).contains("using model directory"));
    let g = check_glb(&std::fs::read(tmp.path().join("out.glb")).unwrap()).unwrap();
    assert_eq!(g.count("images"), 1);
}

#[test]
fn every_flag_together() {
    let tmp = tempfile::tempdir().unwrap();
    write_quad(tmp.path(), "quad.m3");
    std::fs::write(tmp.path().join("anims.m3a"), ModelSpec::default().build()).unwrap();
    std::fs::create_dir(tmp.path().join("tex")).unwrap();
    image::RgbaImage::new(4, 4).save(tmp.path().join("tex").join("quad_diff.png")).unwrap();
    let out = run(
        &[
            "quad.m3", "-o", "q.glb", "-t", "tex", "-a", "anims.m3a", "--no-fx", "--ktx2", "--bevy-compat",
            "--max-tex-size", "2", "-q",
        ],
        tmp.path(),
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(out.stdout.is_empty(), "-q must print nothing: {}", text(&out.stdout));
    assert!(tmp.path().join("q.glb").exists());
}

#[test]
fn failures_exit_non_zero_with_a_message() {
    let tmp = tempfile::tempdir().unwrap();
    for args in [
        &["missing.m3"][..],
        &["bad.m3"],
        &["quad.m3", "-t", "no-such-dir"],
        &["quad.m3", "-a", "missing.m3a"],
        &["quad.m3", "-o", "no/such/dir/x.glb"],
    ] {
        std::fs::write(tmp.path().join("bad.m3"), b"not an m3").unwrap();
        write_quad(tmp.path(), "quad.m3");
        let out = run(args, tmp.path());
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(text(&out.stderr).contains("error"), "{args:?}: {}", text(&out.stderr));
    }
    // --bevy-compat without --ktx2 is a usage error (clap exits 2).
    assert_eq!(run(&["quad.m3", "--bevy-compat"], tmp.path()).status.code(), Some(2));
}
