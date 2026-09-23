//! Texture lookup and indexing via xxh3 hashing.
//!
//! # Strategy
//!
//! 1. Walk the directory recursively and collect every `.png` / `.dds` / `.tga`.
//! 2. For each file compute `xxh3_64(stem.to_lowercase())`.
//! 3. Store an `AHashMap<u64, PathBuf>` — O(1) lookup by hash.
//! 4. `aho-corasick` normalises M3 paths (replace `\` with `/`, strip known prefixes).
//!
//! # Why xxh3 instead of ahash
//!
//! `ahash` is non-deterministic across runs. `xxh3` is deterministic, so
//! the hashes can be cached on disk.

use ahash::AHashMap;
use aho_corasick::AhoCorasick;
use anyhow::Result;
use std::path::{Path, PathBuf};
use tracing::debug;
use xxhash_rust::xxh3::xxh3_64;

/// Texture cache: hash of filename stem → path.
pub struct TextureCache {
    /// Primary table: `xxh3(lowercased_stem)` → full path.
    map: AHashMap<u64, PathBuf>,
    /// Patterns used to normalise M3 paths (strip well-known prefixes).
    normalizer: Option<AhoCorasick>,
}

impl std::fmt::Debug for TextureCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextureCache").field("textures", &self.map.len()).finish_non_exhaustive()
    }
}

impl TextureCache {
    /// Empty cache (no texture directory provided).
    #[must_use]
    pub fn empty() -> Self {
        Self {
            map: AHashMap::new(),
            normalizer: None,
        }
    }

    /// Index every texture under `dir`, recursively.
    ///
    /// Hashing is parallelised via rayon for large texture directories.
    ///
    /// # Errors
    ///
    /// `dir` is not a directory, or a directory under it cannot be read.
    pub fn build(dir: &str) -> Result<Self> {
        use rayon::prelude::*;

        let base_path = Path::new(dir);
        if !base_path.is_dir() {
            anyhow::bail!("texture directory not found: {dir}");
        }

        // Collect every file recursively (single-threaded fs walk).
        let texture_files = collect_texture_files(base_path)?;
        debug!("texture files found: {}", texture_files.len());

        // Hash filenames in parallel.
        let entries: Vec<(u64, PathBuf)> = texture_files
            .par_iter()
            .filter_map(|path| {
                let stem = path.file_stem()?.to_str()?.to_lowercase();
                let hash = xxh3_64(stem.as_bytes());
                Some((hash, path.clone()))
            })
            .collect();

        let mut map = AHashMap::with_capacity(entries.len());
        for (hash, path) in entries {
            // On collision keep the first match.
            if map.get(&hash).is_none() {
                map.insert(hash, path);
            }
        }

        // aho-corasick patterns for M3-path normalisation.
        // M3 may store paths as "Assets\Textures\unit.dds" or "textures/unit.dds".
        let patterns = ["\\", "assets\\", "assets/", "textures\\", "textures/"];
        let normalizer = AhoCorasick::builder()
            .ascii_case_insensitive(true)
            .build(patterns)
            .ok();

        Ok(Self { map, normalizer })
    }

    /// Look up a texture by an M3 path.
    /// Normalises the path, then queries by xxh3 of the stem.
    #[must_use]
    pub fn find(&self, m3_path: &str) -> Option<&PathBuf> {
        let normalized = self.normalize_m3_path(m3_path);

        // Use only the file stem (no extension).
        let stem = Path::new(normalized.as_ref())
            .file_stem()?
            .to_str()?
            .to_lowercase();

        let hash = xxh3_64(stem.as_bytes());
        self.map.get(&hash)
    }

    /// Number of indexed textures.
    #[must_use]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Whether no texture was indexed (always true for [`Self::empty`]).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Look up a texture by M3 path, also returning its MIME type.
    #[must_use]
    pub fn find_with_mime(&self, m3_path: &str) -> Option<(&PathBuf, &'static str)> {
        let normalized = self.normalize_m3_path(m3_path);
        let stem = Path::new(normalized.as_ref())
            .file_stem()?
            .to_str()?
            .to_lowercase();
        let hash = xxh3_64(stem.as_bytes());
        let path = self.map.get(&hash)?;
        // Only files `texture_mime` accepted were indexed.
        Some((path, texture_mime(path).unwrap_or("image/png")))
    }

    /// Normalise an M3 path: drop known prefixes, swap `\` for `/`.
    fn normalize_m3_path<'a>(&self, path: &'a str) -> std::borrow::Cow<'a, str> {
        match &self.normalizer {
            Some(ac) => {
                let replacements = ["/", "", "", "", ""];
                let result = ac.replace_all(path, &replacements);
                std::borrow::Cow::Owned(result)
            }
            None => std::borrow::Cow::Borrowed(path),
        }
    }
}

/// Recursive directory walk that returns every texture file's path.
fn collect_texture_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut result = Vec::new();
    collect_recursive(dir, &mut result)?;
    Ok(result)
}

fn collect_recursive(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            collect_recursive(&path, out)?;
        } else if texture_mime(&path).is_some() {
            out.push(path);
        }
    }
    Ok(())
}

/// MIME type of a texture file, by extension (any case), or `None` for a file
/// that is not an image the converter can embed.
fn texture_mime(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "dds" => "image/vnd-ms.dds",
        "tga" => "image/x-tga",
        "bmp" => "image/bmp",
        _ => return None,
    })
}
