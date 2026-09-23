//! M3 format parser (`StarCraft` II / Heroes of the Storm).
//!
//! # Architecture
//!
//! Lazy zero-copy:
//!  - The file is mmap'd in `main.rs`; `&[u8]` is passed in from there.
//!  - The structures (`Md32Header`, `TagEntry`, ...) are `#[repr(C)]` + `bytemuck::Pod`.
//!  - We materialise `&[StructType]` views directly into the mmap buffer via `bytemuck::cast_slice`.
//!  - No `Vec::clone()`, no `String::from()` for tag names — everything stays as `&str` or `&[u8]`.
//!
//! # M3 layout (in brief)
//!
//! ```text
//! ┌─────────────────────────────┐
//! │  Header (MD34 / MD33)       │  magic + number_of_tags + tag_index_offset
//! ├─────────────────────────────┤
//! │  Tag Index  [TagEntry; N]   │  each: id, type, offset, count
//! ├─────────────────────────────┤
//! │  Tag Data   (variable)      │  per-tag payload — meshes, bones, ...
//! └─────────────────────────────┘
//! ```

pub mod structures;
pub mod reader;

pub use reader::{M3File, layr_record_size, layr_uv_tiling_offset, mat_record_size, stride_from_flags};

use anyhow::{bail, Result};

/// Parse an M3 file from an mmap buffer. Zero-copy — no allocations.
///
/// # Errors
///
/// The magic is not MD32/MD33/MD34, or the tag table does not fit in (or is misaligned within) `data`.
pub fn parse(data: &[u8]) -> Result<M3File<'_>> {
    reader::M3File::from_bytes(data)
}

// ─── Magic-byte constants ────────────────────────────────────────────────────
// Magic is stored as little-endian u32, so the bytes are reversed:
//   "MD34" as a string → on disk: b"43DM"
/// On-disk magic of an MD34 file (`StarCraft` II, Heroes of the Storm).
pub const MAGIC_MD34: [u8; 4] = *b"43DM";
/// On-disk magic of an MD33 file (early `StarCraft` II betas).
pub const MAGIC_MD33: [u8; 4] = *b"33DM";
/// On-disk magic of an MD32 file.
pub const MAGIC_MD32: [u8; 4] = *b"23DM";

/// Inspect the magic bytes at the start of the file.
/// M3 stores tag IDs as little-endian u32 — the bytes are byte-reversed
/// relative to the ASCII spelling.
///
/// # Errors
///
/// `data` is shorter than four bytes or does not start with an M3 magic.
pub fn detect_version(data: &[u8]) -> Result<M3Version> {
    if data.len() < 4 {
        bail!("file too small to be M3 (< 4 bytes)");
    }
    match &data[..4] {
        // On disk the magic is byte-reversed ("MD34" → "43DM"); the natural
        // order is accepted as well, just in case.
        b"43DM" | b"MD34" => Ok(M3Version::Md34),
        b"33DM" | b"MD33" => Ok(M3Version::Md33),
        b"23DM" | b"MD32" => Ok(M3Version::Md32),
        other => bail!(
            "unknown magic: {:?} (expected MD34/MD33/MD32 in LE)",
            std::str::from_utf8(other).unwrap_or("?")
        ),
    }
}

/// M3 header variant, from the file's magic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum M3Version {
    /// `MD32`.
    Md32,
    /// `MD33`.
    Md33,
    /// `MD34` — every shipping `StarCraft` II / Heroes of the Storm model.
    Md34,
}
