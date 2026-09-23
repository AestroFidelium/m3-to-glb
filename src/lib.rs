//! Convert Blizzard **M3** models — StarCraft II and Heroes of the Storm — into
//! **glTF 2.0 binary** (`.glb`).
//!
//! M3 is an undocumented, versioned, tag-indexed binary format. This crate reads
//! it zero-copy straight from the file's bytes and produces a self-contained GLB
//! with geometry, PBR materials and embedded textures, the skeleton and skin,
//! every animation clip, and the model's particle systems, lights, decals and
//! attachment points carried as node `extras`.
//!
//! # Quick start
//!
//! ```no_run
//! use m3_to_glb::{Converter, TextureCache};
//!
//! # fn main() -> Result<(), m3_to_glb::Error> {
//! let model = std::fs::read("Storm_Hero_Anduin_Base.m3")?;
//! let anims = std::fs::read("Storm_Hero_Anduin_RequiredAnims.m3a")?;
//! let textures = TextureCache::build("textures/")?;
//!
//! let glb = Converter::new()
//!     .animations(&anims)
//!     .textures(&textures)
//!     .convert(&model)?;
//!
//! std::fs::write("anduin.glb", &glb.bytes)?;
//! println!("{} meshes, {} animations", glb.stats.meshes, glb.stats.animations);
//! # Ok(()) }
//! ```
//!
//! Conversion never panics on malformed input — every M3 byte is untrusted, and
//! the parser and the whole pipeline are fuzzed with structure-aware inputs (see
//! `tests/fuzz_pipeline.rs`). A file that is too broken to convert is an
//! [`Error`]; one that is merely unusual converts with the unusual parts
//! skipped.
//!
//! # Pipeline
//!
//! | stage | module | |
//! |---|---|---|
//! | parse | [`m3`] | tag table, version-dependent struct layouts |
//! | geometry | [`processor`] | dynamic vertex layout → SoA, SIMD decode, rayon per division |
//! | animation | [`processor::anim`] | `SEQS`/`STG_`/`STC_` → glTF samplers |
//! | effects | [`fx`], [`attach`] | `PAR_`/`LITE`/`PROJ`/`ATT_` → node `extras` |
//! | textures | [`assets`] | directory index keyed by xxh3 of the file stem |
//! | packing | [`glb`] | JSON manifest + BIN chunk |
//!
//! # Features
//!
//! * `cli` *(default)* — the `m3-to-glb` binary. Library users can disable it
//!   with `default-features = false` to drop clap, the terminal logger and the
//!   global allocator.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]

pub mod assets;
pub mod attach;
pub mod fx;
pub mod glb;
mod json;
pub mod m3;
pub mod processor;
pub mod quat;

pub use assets::TextureCache;
pub use glb::PackOptions;
pub use m3::{M3File, parse};

use std::path::{Path, PathBuf};

/// Everything that can go wrong in a conversion.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The model file could not be read or the output file written.
    #[error("{}: {source}", path.display())]
    Io {
        /// File the operation was on.
        path:   PathBuf,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// An I/O error with no file attached to it.
    #[error(transparent)]
    IoOther(#[from] std::io::Error),
    /// The base model is not a readable M3 file.
    #[error("model: {0:#}")]
    Model(anyhow::Error),
    /// Companion animation file number `index` is not a readable M3 file.
    #[error("animation file #{index}: {source:#}")]
    Animation {
        /// Position of the file in the list given to [`Converter::animations`].
        index:  usize,
        /// What was wrong with it.
        source: anyhow::Error,
    },
    /// The texture directory could not be indexed.
    #[error("textures: {0:#}")]
    Textures(anyhow::Error),
    /// The parsed model could not be laid out as glTF.
    #[error("conversion: {0:#}")]
    Convert(anyhow::Error),
}

impl From<anyhow::Error> for Error {
    /// Texture indexing is the only public API that returns `anyhow` directly.
    fn from(e: anyhow::Error) -> Self {
        Self::Textures(e)
    }
}

/// Shorthand for `Result<T, m3_to_glb::Error>`.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// A converted model.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Glb {
    /// The complete `.glb` file.
    pub bytes: Vec<u8>,
    /// What went into it.
    pub stats: Stats,
}

/// Counts describing a conversion — what the CLI prints on success.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Stats {
    /// Mesh divisions converted (usually 1; 0 for an effect-only model).
    pub meshes:     usize,
    /// Triangles across all meshes.
    pub triangles:  usize,
    /// Bones in the skeleton.
    pub bones:      usize,
    /// Textures available in the index that was used.
    pub textures:   usize,
    /// Animation clips the model and its companion files declare.
    pub animations: usize,
}

/// Configures and runs a conversion.
///
/// Holds borrowed inputs only, so it is cheap to build per model. The defaults
/// match the CLI: effects on, textures embedded as-is, no animation files.
#[derive(Debug, Clone, Default)]
#[must_use]
pub struct Converter<'a> {
    animations: Vec<&'a [u8]>,
    textures:   Option<&'a TextureCache>,
    options:    PackOptions,
}

impl<'a> Converter<'a> {
    /// A converter with the default options.
    pub fn new() -> Self {
        Self { options: PackOptions { fx: true, ..PackOptions::default() }, ..Self::default() }
    }

    /// Add a companion `.m3a` animation file. Heroes of the Storm ships most
    /// clips outside the base model; call once per file.
    pub fn animations(mut self, m3a: &'a [u8]) -> Self {
        self.animations.push(m3a);
        self
    }

    /// Resolve and embed material textures from this index.
    pub fn textures(mut self, cache: &'a TextureCache) -> Self {
        self.textures = Some(cache);
        self
    }

    /// Export particle systems, lights and decals as `m3fx` node extras
    /// (default `true`).
    pub fn effects(mut self, on: bool) -> Self {
        self.options.fx = on;
        self
    }

    /// Downscale embedded textures so neither side exceeds `px`; `0` keeps
    /// them as they are (the default).
    pub fn max_texture_size(mut self, px: u32) -> Self {
        self.options.max_tex_size = px;
        self
    }

    /// Transcode textures to KTX2 (UASTC + Zstd) through `toktx` and declare
    /// `KHR_texture_basisu`. Falls back to the source image per texture when
    /// `toktx` is missing or fails.
    pub fn ktx2(mut self, on: bool) -> Self {
        self.options.ktx2 = on;
        self
    }

    /// Replace every option at once.
    pub fn options(mut self, options: PackOptions) -> Self {
        self.options = options;
        self
    }

    /// Convert an M3 model held in memory.
    ///
    /// # Errors
    ///
    /// [`Error::Model`] / [`Error::Animation`] when an input is not an M3 file
    /// at all, [`Error::Convert`] when it parses but cannot be laid out.
    pub fn convert(&self, model: &[u8]) -> Result<Glb> {
        let m3 = m3::parse(model).map_err(Error::Model)?;
        m3.dump_tags();
        let anims = self
            .animations
            .iter()
            .enumerate()
            .map(|(index, bytes)| {
                m3::parse(bytes).map_err(|source| Error::Animation { index, source })
            })
            .collect::<Result<Vec<_>>>()?;
        let anim_refs: Vec<&M3File<'_>> = anims.iter().collect();

        let empty = TextureCache::empty();
        let textures = self.textures.unwrap_or(&empty);

        let meshes = processor::convert_all_meshes(&m3).map_err(Error::Convert)?;
        let bytes = glb::pack(&meshes, textures, &m3, &anim_refs, &self.options)
            .map_err(Error::Convert)?;

        let animations = std::iter::once(&m3)
            .chain(anims.iter())
            .map(|f| f.sequence_collections().map_or(0, |s| s.len()))
            .sum();
        let stats = Stats {
            meshes:    meshes.len(),
            triangles: meshes.iter().map(processor::MeshDataSoA::triangle_count).sum(),
            bones:     m3.bone_count(),
            textures:  textures.len(),
            animations,
        };
        Ok(Glb { bytes, stats })
    }

    /// Read `input`, convert it, and write the GLB to `output`.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] for either file, otherwise as [`Converter::convert`].
    pub fn convert_file(&self, input: impl AsRef<Path>, output: impl AsRef<Path>) -> Result<Stats> {
        let (input, output) = (input.as_ref(), output.as_ref());
        let io = |path: &Path| {
            let path = path.to_path_buf();
            move |source| Error::Io { path, source }
        };
        let model = std::fs::read(input).map_err(io(input))?;
        let glb = self.convert(&model)?;
        std::fs::write(output, &glb.bytes).map_err(io(output))?;
        Ok(glb.stats)
    }
}
