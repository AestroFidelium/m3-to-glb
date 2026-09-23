//! The BIN chunk and the JSON arrays that index into it — buffer views,
//! accessors and embedded images.

use super::json_builder::{Accessor, BufferView, GltfImage};
use super::ktx2::{self, TextureRole};
use super::PackOptions;
use crate::assets::TextureCache;
use tracing::{debug, warn};

/// The binary buffer plus the views and accessors describing it.
#[derive(Default)]
pub(super) struct Buffers {
    pub bin:       Vec<u8>,
    pub views:     Vec<BufferView>,
    pub accessors: Vec<Accessor>,
}

impl Buffers {
    /// Append `data` 4-byte aligned and describe it with a new buffer view.
    pub fn view(&mut self, data: &[u8], target: Option<u32>) -> usize {
        let offset = self.bin.len().next_multiple_of(4);
        self.bin.resize(offset, 0);
        self.bin.extend_from_slice(data);
        self.views.push(BufferView { offset, length: data.len(), target });
        self.views.len() - 1
    }

    /// Add an accessor and return its index.
    pub fn accessor(&mut self, accessor: Accessor) -> usize {
        self.accessors.push(accessor);
        self.accessors.len() - 1
    }

    /// A tightly packed view of `data` with one accessor over all of it.
    pub fn attribute(
        &mut self,
        data: &[u8],
        target: Option<u32>,
        component_type: u32,
        count: usize,
        element_type: &'static str,
    ) -> usize {
        let buffer_view = self.view(data, target);
        self.accessor(Accessor {
            buffer_view,
            byte_offset: 0,
            component_type,
            count,
            element_type,
            normalized: false,
            min: None,
            max: None,
        })
    }
}

/// Embedded images, deduplicated by path and role.
///
/// Effect materials routinely share a texture with each other and with mesh
/// materials (one frost atlas across six emitters); without the cache every
/// reference would embed — and, under `--ktx2`, re-encode — its own copy.
pub(super) struct Images<'t> {
    textures: &'t TextureCache,
    ktx2:     bool,
    max_dim:  u32,
    pub json: Vec<GltfImage>,
    cache:    ahash::AHashMap<(String, TextureRole), Option<usize>>,
}

impl<'t> Images<'t> {
    pub fn new(textures: &'t TextureCache, options: PackOptions) -> Self {
        Self {
            textures,
            ktx2: options.ktx2,
            max_dim: options.max_tex_size,
            json: Vec::new(),
            cache: ahash::AHashMap::new(),
        }
    }

    /// Embed the texture an M3 path names and return its image index, or
    /// `None` when it is not in the index or cannot be read.
    pub fn load(&mut self, bufs: &mut Buffers, path: &str, role: TextureRole) -> Option<usize> {
        if path.is_empty() || self.textures.is_empty() {
            return None;
        }
        let key = (path.to_ascii_lowercase(), role);
        if let Some(&hit) = self.cache.get(&key) {
            return hit;
        }
        let image = self.encode(path, role).map(|(bytes, mime_type)| {
            let buffer_view = bufs.view(&bytes, None);
            self.json.push(GltfImage { buffer_view, mime_type });
            self.json.len() - 1
        });
        self.cache.insert(key, image);
        image
    }

    /// The bytes and MIME type to embed. KTX2 is tried first when requested;
    /// on failure (no `toktx`, unsupported source) the source image is used,
    /// still honouring the size cap.
    fn encode(&self, path: &str, role: TextureRole) -> Option<(Vec<u8>, String)> {
        let (file, mime) = self.textures.find_with_mime(path)?;
        if self.ktx2 {
            let opts = ktx2::EncodeOptions { role, max_dim: self.max_dim };
            match ktx2::transcode(file, opts) {
                Ok(b) => {
                    debug!("transcoded {} → KTX2 ({:?}, {} bytes)", file.display(), role, b.len());
                    return Some((b, "image/ktx2".to_owned()));
                }
                Err(e) => warn!("KTX2 transcode failed for {}: {} — embedding original", file.display(), e),
            }
        }
        match ktx2::read_with_optional_downscale(file, role, self.max_dim, mime) {
            Ok(t) => {
                debug!("loading texture {} ({} bytes, mime={})", file.display(), t.0.len(), t.1);
                Some(t)
            }
            Err(e) => {
                debug!("failed to read texture {}: {}", file.display(), e);
                None
            }
        }
    }
}
