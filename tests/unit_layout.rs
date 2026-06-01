//! Deterministic + property tests for the pure, panic-free parts of the
//! parser: magic detection, vertex stride, and vertex component offsets.
//!
//! The property tests use `bolero`, so they run both under plain `cargo test`
//! (bounded random driver) and as real fuzzing via `cargo bolero test <name>`.

use bolero::check;
use m3_to_glb::m3::{detect_version, stride_from_flags, M3Version};
use m3_to_glb::processor::VertexOffsets;

// ── detect_version ───────────────────────────────────────────────────────────

#[test]
fn detect_version_le_magics() {
    assert_eq!(detect_version(b"43DM....").unwrap(), M3Version::Md34);
    assert_eq!(detect_version(b"33DM....").unwrap(), M3Version::Md33);
    assert_eq!(detect_version(b"23DM....").unwrap(), M3Version::Md32);
}

#[test]
fn detect_version_natural_magics() {
    assert_eq!(detect_version(b"MD34....").unwrap(), M3Version::Md34);
    assert_eq!(detect_version(b"MD33....").unwrap(), M3Version::Md33);
}

#[test]
fn detect_version_rejects_garbage_and_short() {
    assert!(detect_version(b"XXXX").is_err());
    assert!(detect_version(b"").is_err());
    assert!(detect_version(b"43D").is_err()); // < 4 bytes
}

#[test]
fn detect_version_never_panics_on_any_bytes() {
    // Any 0..=8 byte prefix must return Ok/Err, never panic.
    check!().with_type::<Vec<u8>>().for_each(|bytes: &Vec<u8>| {
        let _ = detect_version(bytes);
    });
}

// ── stride_from_flags ──────────────────────────────────────────────────────────

#[test]
fn stride_pos_only() {
    assert_eq!(stride_from_flags(0x1), 12);
    assert_eq!(stride_from_flags(0x0), 12); // pos is implied regardless
}

#[test]
fn stride_anduin_reference() {
    // Storm_Hero_Anduin_Base.m3 — the Phase-1 regression anchor.
    // 0x01820061 = pos | skin0 | skin1 | uv0 | normal | tangent.
    // 12 + 4 + 4 + 4 + 4 + 4 = 32.
    assert_eq!(stride_from_flags(0x01820061), 32);
}

// ── VertexOffsets::from_flags ──────────────────────────────────────────────────

#[test]
fn offsets_anduin_reference() {
    let o = VertexOffsets::from_flags(0x01820061);
    let skin = o.skin.expect("Anduin has skin");
    assert_eq!(skin.pairs, 4);
    assert_eq!(skin.weights_offset, 12);
    assert_eq!(skin.lookups_offset, 16);
    assert_eq!(o.normal, Some(20));
    assert_eq!(o.uv0, Some(24));
    assert_eq!(o.uv1, None);
    assert_eq!(o.tangent, Some(28));
}

// ── Consistency property: offsets must never exceed the stride ─────────────────
//
// `VertexOffsets::from_flags` and `stride_from_flags` walk the same layout from
// independent code. For ANY flag bitset, every component the offsets table
// reports must fit entirely inside the stride. A drift between the two (the
// exact class of bug that produced garbage UVs in Phase 1) breaks this.

fn component_size(_flags: u32) -> usize {
    // normal/uv/tangent are all 4 bytes; skin is `pairs` bytes per array.
    4
}

#[test]
fn offsets_fit_within_stride() {
    check!().with_type::<u32>().for_each(|&flags: &u32| {
        let stride = stride_from_flags(flags);
        let o = VertexOffsets::from_flags(flags);

        for off in [o.normal, o.uv0, o.uv1, o.tangent].into_iter().flatten() {
            assert!(
                off + component_size(flags) <= stride,
                "flags={flags:#010x}: component at {off} (+{}) exceeds stride {stride}",
                component_size(flags),
            );
        }
        if let Some(s) = o.skin {
            assert!(s.weights_offset + s.pairs <= s.lookups_offset);
            assert!(
                s.lookups_offset + s.pairs <= stride,
                "flags={flags:#010x}: skin lookups at {} (+{}) exceeds stride {stride}",
                s.lookups_offset, s.pairs,
            );
        }
    });
}
