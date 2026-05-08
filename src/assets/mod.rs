//! Поиск и индексирование текстур через xxh3-хэширование.
//!
//! # Стратегия
//!
//! 1. Рекурсивно обходим папку и собираем все `.png` / `.dds` / `.tga`
//! 2. Для каждого имени файла вычисляем `xxh3_64(имя_без_расширения.to_lowercase())`
//! 3. Храним `AHashMap<u64, PathBuf>` — O(1) поиск по хэшу
//! 4. `aho-corasick` для нормализации путей M3 (замена `\` на `/`, удаление префиксов)
//!
//! # Почему xxh3 а не ahash
//!
//! `ahash` недетерминирован между запусками.
//! `xxh3` детерминирован — хэши можно кэшировать на диск.

use ahash::AHashMap;
use aho_corasick::AhoCorasick;
use anyhow::Result;
use std::path::{Path, PathBuf};
use tracing::{debug, warn};
use xxhash_rust::xxh3::xxh3_64;

/// Кэш текстур: хэш имени → путь к файлу.
pub struct TextureCache {
    /// Основная хэш-карта: xxh3(lowercased_stem) → полный путь
    map: AHashMap<u64, PathBuf>,
    /// Паттерны для нормализации M3 путей (замена известных префиксов)
    normalizer: Option<AhoCorasick>,
}

impl TextureCache {
    /// Пустой кэш (если папка текстур не указана).
    pub fn empty() -> Self {
        Self {
            map: AHashMap::new(),
            normalizer: None,
        }
    }

    /// Индексирует все текстуры в папке рекурсивно.
    ///
    /// Параллельно с rayon для быстрого обхода больших папок.
    pub fn build(dir: &str) -> Result<Self> {
        use rayon::prelude::*;

        let base_path = Path::new(dir);
        if !base_path.is_dir() {
            anyhow::bail!("Папка текстур не найдена: {}", dir);
        }

        // Собираем все файлы рекурсивно (однопоточно — fs обход)
        let texture_files = collect_texture_files(base_path)?;
        debug!("Найдено файлов текстур: {}", texture_files.len());

        // Хэшируем имена параллельно через rayon
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
            // Коллизии — оставляем первый найденный файл, предупреждаем
            if let Some(existing) = map.get(&hash) {
                // warn!(
                //     "Коллизия xxh3 хэша для {:?} и {:?}, оставляю первый",
                //     existing, path
                // );
            } else {
                map.insert(hash, path);
            }
        }

        // aho-corasick: паттерны для нормализации M3 путей
        // M3 может хранить пути как "Assets\Textures\unit.dds" или "textures/unit.dds"
        let patterns = ["\\", "assets\\", "assets/", "textures\\", "textures/"];
        let normalizer = AhoCorasick::builder()
            .ascii_case_insensitive(true)
            .build(&patterns)
            .ok();

        Ok(Self { map, normalizer })
    }

    /// Ищет текстуру по пути из M3 файла.
    /// Нормализует путь и ищет по xxh3 хэшу имени.
    pub fn find(&self, m3_path: &str) -> Option<&PathBuf> {
        // Нормализуем путь из M3
        let normalized = self.normalize_m3_path(m3_path);

        // Берём только имя файла без расширения
        let stem = Path::new(normalized.as_ref())
            .file_stem()?
            .to_str()?
            .to_lowercase();

        let hash = xxh3_64(stem.as_bytes());
        self.map.get(&hash)
    }

    /// Количество проиндексированных текстур.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Ищет текстуру по пути из M3 и возвращает путь + MIME-type.
    pub fn find_with_mime(&self, m3_path: &str) -> Option<(&PathBuf, &'static str)> {
        let normalized = self.normalize_m3_path(m3_path);
        let stem = Path::new(normalized.as_ref())
            .file_stem()?
            .to_str()?
            .to_lowercase();
        let hash = xxh3_64(stem.as_bytes());
        let path = self.map.get(&hash)?;
        let mime = mime_type_for_path(path);
        Some((path, mime))
    }

    /// Нормализует путь из M3: убирает известные префиксы, меняет `\` на `/`.
    fn normalize_m3_path<'a>(&self, path: &'a str) -> std::borrow::Cow<'a, str> {
        match &self.normalizer {
            Some(ac) => {
                let replacements = ["/", "", "", "", ""];
                let result = ac.replace_all(path, &replacements);
                std::borrow::Cow::Owned(result)
            }
            none => std::borrow::Cow::Borrowed(path),
        }
    }
}

/// Рекурсивный обход папки, возвращает пути ко всем файлам текстур.
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
        } else if is_texture_file(&path) {
            out.push(path);
        }
    }
    Ok(())
}

#[inline]
fn is_texture_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("png" | "dds" | "tga" | "jpg" | "jpeg" | "bmp" | "PNG" | "DDS" | "TGA")
    )
}

/// Определяет MIME-type по расширению файла.
#[inline]
fn mime_type_for_path(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("jpg") | Some("jpeg") | Some("JPG") | Some("JPEG") => "image/jpeg",
        Some("png") | Some("PNG") => "image/png",
        Some("dds") | Some("DDS") => "image/vnd-ms.dds",
        Some("tga") | Some("TGA") => "image/x-tga",
        Some("bmp") | Some("BMP") => "image/bmp",
        _ => "image/png",
    }
}
