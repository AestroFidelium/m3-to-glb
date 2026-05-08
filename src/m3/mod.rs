//! M3 format parser (StarCraft II / Heroes of the Storm).
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

pub use reader::M3File;

use anyhow::{bail, Result};

/// Parse an M3 file from an mmap buffer. Zero-copy — no allocations.
pub fn parse(data: &[u8]) -> Result<M3File<'_>> {
    reader::M3File::from_bytes(data)
}

// ─── Magic-byte constants ────────────────────────────────────────────────────
// Magic is stored as little-endian u32, so the bytes are reversed:
//   "MD34" as a string → on disk: b"43DM"
pub const MAGIC_MD34: [u8; 4] = *b"43DM";
pub const MAGIC_MD33: [u8; 4] = *b"33DM";
pub const MAGIC_MD32: [u8; 4] = *b"23DM";

/// Inspect the magic bytes at the start of the file.
/// M3 stores tag IDs as little-endian u32 — the bytes are byte-reversed
/// relative to the ASCII spelling.
pub fn detect_version(data: &[u8]) -> Result<M3Version> {
    if data.len() < 4 {
        bail!("file too small to be M3 (< 4 bytes)");
    }
    match &data[..4] {
        b"43DM" => Ok(M3Version::Md34), // "MD34" LE — most common
        b"33DM" => Ok(M3Version::Md33), // "MD33" LE
        b"23DM" => Ok(M3Version::Md32), // "MD32" LE
        // Accept the natural byte order as well, just in case.
        b"MD34" => Ok(M3Version::Md34),
        b"MD33" => Ok(M3Version::Md33),
        b"MD32" => Ok(M3Version::Md32),
        other => bail!(
            "unknown magic: {:?} (expected MD34/MD33/MD32 in LE)",
            std::str::from_utf8(other).unwrap_or("?")
        ),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum M3Version {
    Md32,
    Md33,
    Md34,
}
