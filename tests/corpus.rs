//! Convert every `.m3` under `$M3_CORPUS` and hold each result to the GLB
//! oracle. Blizzard's files cannot ship with the repository, so this runs only
//! when pointed at a local extraction:
//!
//! ```text
//! M3_CORPUS=/path/to/extracted cargo test --release --test corpus -- --ignored --nocapture
//! ```

mod common;

use rayon::prelude::*;
use std::path::{Path, PathBuf};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("m3")) {
            out.push(p);
        }
    }
}

#[test]
#[ignore = "needs M3_CORPUS pointing at extracted game files"]
fn real_models_convert_to_valid_glb() {
    let Some(root) = std::env::var_os("M3_CORPUS") else {
        eprintln!("M3_CORPUS not set — nothing to do");
        return;
    };
    let mut files = Vec::new();
    walk(Path::new(&root), &mut files);
    files.sort();

    let start = std::time::Instant::now();
    let rejected = std::sync::atomic::AtomicUsize::new(0);
    let results: Vec<(PathBuf, Result<(), String>)> = files
        .par_iter()
        .map(|p| {
            let r = std::fs::read(p).map_err(|e| e.to_string()).and_then(|bytes| {
                match m3_to_glb::Converter::new().convert(&bytes) {
                    Ok(glb) => common::check_glb(&glb.bytes).map(drop).map_err(|e| format!("INVALID: {e}")),
                    // Rejecting a file is allowed; emitting a broken one is not.
                    Err(_) => {
                        rejected.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        Ok(())
                    }
                }
            });
            (p.clone(), r)
        })
        .collect();
    let invalid: Vec<_> = results.iter().filter(|(_, r)| r.is_err()).collect();
    eprintln!(
        "{} models in {:.1?}: {} rejected as malformed, {} invalid output",
        files.len(),
        start.elapsed(),
        rejected.into_inner(),
        invalid.len()
    );
    for (p, e) in invalid.iter().take(20) {
        eprintln!("  {}: {}", p.display(), e.as_ref().unwrap_err());
    }
    assert!(invalid.is_empty());
}
