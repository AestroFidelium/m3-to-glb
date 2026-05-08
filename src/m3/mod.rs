//! Парсер формата M3 (StarCraft II / Heroes of the Storm).
//!
//! # Архитектура
//!
//! Используем **lazy zero-copy** подход:
//!  - Файл открыт через `memmap2` в `main.rs`, сюда передаётся `&[u8]`
//!  - Структуры (`Md32Header`, `TagEntry`, ...) — это `#[repr(C)]` + `bytemuck::Pod`
//!  - Мы создаём ссылки `&[StructType]` прямо в mmap-буфере через `bytemuck::cast_slice`
//!  - Никаких `Vec::clone()`, никаких `String::from()` для имён — только `&str` или `&[u8]`
//!
//! # Формат M3 (вкратце)
//!
//! ```text
//! ┌─────────────────────────────┐
//! │  Header (MD34 / MD33)       │  magic + number_of_tags + tag_index_offset
//! ├─────────────────────────────┤
//! │  Tag Index  [TagEntry; N]   │  каждый: id, type, offset, count
//! ├─────────────────────────────┤
//! │  Tag Data   (variable)      │  данные каждого тега — меши, кости, ...
//! └─────────────────────────────┘
//! ```

pub mod structures;
pub mod reader;

pub use reader::M3File;

use anyhow::{bail, Result};

/// Разбирает M3-файл из mmap-буфера. Zero-copy — никаких аллокаций.
pub fn parse(data: &[u8]) -> Result<M3File<'_>> {
    reader::M3File::from_bytes(data)
}

// ─── Константы магических байтов ──────────────────────────────────────────────
// Magic в файле хранится как u32 little-endian, поэтому байты перевёрнуты:
//   "MD34" как строка → в файле: b"43DM"
pub const MAGIC_MD34: [u8; 4] = *b"43DM";
pub const MAGIC_MD33: [u8; 4] = *b"33DM";
pub const MAGIC_MD32: [u8; 4] = *b"23DM";

/// Проверяем magic в начале файла.
/// M3 хранит тег как u32 LE — байты в файле идут задом наперёд относительно ASCII.
pub fn detect_version(data: &[u8]) -> Result<M3Version> {
    if data.len() < 4 {
        bail!("Файл слишком мал для M3 (< 4 байт)");
    }
    match &data[..4] {
        b"43DM" => Ok(M3Version::Md34), // "MD34" LE — самый распространённый
        b"33DM" => Ok(M3Version::Md33), // "MD33" LE
        b"23DM" => Ok(M3Version::Md32), // "MD32" LE
        // На всякий случай принимаем и прямой порядок
        b"MD34" => Ok(M3Version::Md34),
        b"MD33" => Ok(M3Version::Md33),
        b"MD32" => Ok(M3Version::Md32),
        other => bail!(
            "Неизвестный magic: {:?} (ожидаем MD34/MD33/MD32 в LE)",
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
