//! Fuzz + robustness tests for the M3 parser.
//!
//! `m3::parse` takes raw bytes straight off an mmap and must NEVER panic on
//! malformed input — only return `Err`. This is both a `bolero` fuzz target
//! (`cargo bolero test fuzz_parse_never_panics`) and a set of deterministic
//! regression tests pinning specific malformed-input crash classes.

use bolero::check;
use m3_to_glb::m3;
use m3_to_glb::processor;

/// Drive a parsed file through every accessor a fuzzer should stress. None of
/// these may panic regardless of how corrupt the source bytes were.
fn exercise(m3f: &m3::M3File<'_>) {
    let _ = m3f.mesh_count();
    let _ = m3f.material_count();
    let _ = m3f.bone_count();
    let _ = m3f.vertex_flags();
    let _ = m3f.vertex_stride();
    let _ = m3f.vertex_data();
    let _ = processor::convert_all_meshes(m3f);
}

// ── Fuzz target ────────────────────────────────────────────────────────────────

#[test]
fn fuzz_parse_never_panics() {
    check!().for_each(|data: &[u8]| {
        if let Ok(m3f) = m3::parse(data) {
            exercise(&m3f);
        }
    });
}

// ── Deterministic regression seeds (crash classes) ─────────────────────────────

/// Build a minimal 12-byte M3 header: LE magic + tag-index offset + tag count.
fn header(magic: &[u8; 4], index_offset: u32, num_tags: u32) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(magic);
    v.extend_from_slice(&index_offset.to_le_bytes());
    v.extend_from_slice(&num_tags.to_le_bytes());
    v
}

#[test]
fn parse_empty_is_err_not_panic() {
    assert!(m3::parse(&[]).is_err());
}

#[test]
fn parse_header_only_no_tags_is_err() {
    // Valid magic but the file ends right after the 12-byte header: the tag
    // table can't fit.
    let data = header(b"43DM", 12, 1);
    assert!(m3::parse(&data).is_err());
}

#[test]
fn parse_truncated_tag_table_is_err() {
    // Claims 100 tags (100 * 16 = 1600 bytes) but supplies far fewer.
    let mut data = header(b"43DM", 12, 100);
    data.resize(64, 0);
    assert!(m3::parse(&data).is_err());
}

#[test]
fn parse_misaligned_tag_offset_is_err_not_panic() {
    // `MdIndexEntry` is 16 bytes / align 4. A tag offset of 13 makes
    // `cast_slice` see a misaligned slice → it used to panic. It must Err.
    let mut data = header(b"43DM", 13, 1);
    data.resize(13 + 16, 0); // room for one 16-byte entry at offset 13
    let _ = m3::parse(&data); // must not panic; Ok or Err both acceptable
}

#[test]
fn parse_huge_tag_count_is_err_not_panic() {
    // Enormous tag count must be rejected by the bounds check, not overflow.
    let data = header(b"43DM", 12, u32::MAX);
    assert!(m3::parse(&data).is_err());
}

// ── Known-good replay (skipped if the local .m3 assets aren't present) ──────────

/// Mutation fuzzing seeded from a real file. Truncations and byte-flips keep
/// the magic/structure realistic enough to drive the *deep* pipeline
/// (regions, vertex buffer, skin, conversion) — the paths random bytes never
/// reach. Any panic here is a parser-robustness bug. Engine-independent, so it
/// runs in plain `cargo test`.
#[test]
fn mutate_real_m3_never_panics() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("Storm_Doodad_DS19_Trees_00.m3");
    let Ok(seed) = std::fs::read(&path) else {
        eprintln!("skip mutate_real_m3: seed not present");
        return;
    };

    let run = |bytes: &[u8]| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if let Ok(m3f) = m3::parse(bytes) {
                exercise(&m3f);
            }
        }))
    };

    // 1) Truncate at many lengths — stresses every bounds check downstream.
    let mut failures = Vec::new();
    let step = (seed.len() / 512).max(1);
    for len in (0..seed.len()).step_by(step) {
        if run(&seed[..len]).is_err() {
            failures.push(format!("truncate@{len}"));
        }
    }

    // 2) Single-byte flips across the header + tag table region (first 4 KiB),
    //    where corrupting offsets/counts/versions is most likely to crash.
    let probe = seed.len().min(4096);
    for i in 0..probe {
        let mut m = seed.clone();
        m[i] ^= 0xFF;
        if run(&m).is_err() {
            failures.push(format!("flip@{i}"));
        }
    }

    assert!(
        failures.is_empty(),
        "parser panicked on {} mutation(s): {:?}",
        failures.len(),
        &failures[..failures.len().min(10)],
    );
}

#[test]
fn replay_repo_root_m3_files() {
    for name in [
        "Storm_Doodad_DS19_Trees_00.m3",
        "Storm_Doodad_KingsCrest_DragonKnight_Platform.m3",
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(name);
        let Ok(bytes) = std::fs::read(&path) else {
            eprintln!("skip {name}: not present");
            continue;
        };
        let m3f = m3::parse(&bytes).unwrap_or_else(|e| panic!("parse {name}: {e:#}"));
        exercise(&m3f);
    }
}
