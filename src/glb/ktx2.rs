//! KTX2 texture transcoder.
//!
//! Shells out to `toktx` (KTX-Software) to encode an existing PNG/DDS/TGA/JPEG
//! into KTX2. The encoder mode is chosen per texture role:
//!
//!   - [`TextureRole::Color`] → ETC1S/BasisLZ. Compact (≈0.5 bpp), lossy on
//!     hue/chroma but visually fine for sRGB color (baseColor, emissive).
//!   - [`TextureRole::Data`]  → UASTC + Zstd, OETF tagged `linear`.
//!     Per-channel precision is preserved (required for normal maps), and
//!     the linear tag prevents engines from applying gamma at sample time.
//!
//! A mip chain is generated for both modes. Output is suitable for the
//! `KHR_texture_basisu` glTF extension; engines such as Bevy or three.js
//! transcode the basis-encoded data into the platform-native format
//! (BC7/BC5/ASTC/ETC2) at load time, keeping the texture compressed in VRAM.

use anyhow::{Context, Result, anyhow, bail};
use image::DynamicImage;
use std::path::Path;
use std::process::Command;

/// Role a texture plays in the material — drives encoder selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureRole {
    /// sRGB color data (baseColor, emissive). Encoded with ETC1S.
    Color,
    /// Linear data channel (normal, occlusion, metallicRoughness). Encoded
    /// with UASTC + Zstd; OETF forced to linear.
    Data,
}

#[derive(Debug, Clone, Copy)]
pub struct EncodeOptions {
    pub role: TextureRole,
    /// If non-zero, downscale (aspect-preserving Lanczos3) so that
    /// `max(width, height) <= max_dim` before encoding.
    pub max_dim: u32,
}

/// Transcode a source texture into a KTX2 byte blob ready to embed in the
/// GLB buffer.
pub fn transcode(input_path: &Path, opts: &EncodeOptions) -> Result<Vec<u8>> {
    let tmp = tempfile::tempdir().context("creating tempdir for KTX2 transcode")?;
    let png_path  = tmp.path().join("input.png");
    let ktx2_path = tmp.path().join("output.ktx2");

    // Decode → optional resize → re-encode as PNG (toktx ingests PNG/JPEG/EXR).
    let img = image::open(input_path)
        .with_context(|| format!("decoding source texture {:?}", input_path))?;
    let img = maybe_downscale(img, opts.max_dim);
    img.save_with_format(&png_path, image::ImageFormat::Png)
        .with_context(|| format!("writing temp PNG {:?}", png_path))?;

    let mut cmd = Command::new("toktx");
    cmd.arg("--t2").arg("--genmipmap");
    match opts.role {
        TextureRole::Color => {
            // ETC1S/BasisLZ has its own internal supercompression — passing
            // `--zcmp` on top is both unnecessary and rejected by toktx.
            // OETF stays at toktx's default (sRGB for PNG input), which is
            // exactly what color textures want.
            cmd.args(["--encode", "etc1s"]);
        }
        TextureRole::Data => {
            cmd.args(["--encode", "uastc"]);
            cmd.args(["--uastc_quality", "2"]);
            cmd.args(["--zcmp", "18"]);
            // Without this, toktx tags PNG inputs as sRGB; the GPU sampler
            // would then apply gamma decode on normal-map samples and
            // produce subtly wrong tangent-space lighting.
            cmd.args(["--assign_oetf", "linear"]);
        }
    }
    cmd.arg(&ktx2_path).arg(&png_path);

    let status = cmd
        .status()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                anyhow!(
                    "`toktx` not found in PATH. Install KTX-Software \
                     (e.g. `pkgs.ktx-tools` in Nix, or https://github.com/KhronosGroup/KTX-Software) \
                     or omit --ktx2."
                )
            } else {
                anyhow!("running toktx: {e}")
            }
        })?;

    if !status.success() {
        bail!("toktx exited with status {status}");
    }

    std::fs::read(&ktx2_path)
        .with_context(|| format!("reading toktx output {:?}", ktx2_path))
}

/// Read a texture file, optionally downscaling it to `max_dim`.
/// Used by the non-KTX2 embedding path (raw PNG/DDS/TGA in the GLB buffer).
///
/// When no resize is needed (or `max_dim == 0`), the file is returned
/// byte-for-byte and the original MIME is preserved. When a resize is
/// applied, the image is decoded, resized (Lanczos3) and re-encoded as
/// PNG; the returned MIME switches to `image/png`.
pub fn read_with_optional_downscale(
    input_path: &Path,
    max_dim:    u32,
    src_mime:   &str,
) -> Result<(Vec<u8>, String)> {
    if max_dim == 0 {
        let bytes = std::fs::read(input_path)
            .with_context(|| format!("reading texture {:?}", input_path))?;
        return Ok((bytes, src_mime.to_owned()));
    }

    let img = image::open(input_path)
        .with_context(|| format!("decoding source texture {:?}", input_path))?;
    if img.width().max(img.height()) <= max_dim {
        let bytes = std::fs::read(input_path)
            .with_context(|| format!("reading texture {:?}", input_path))?;
        return Ok((bytes, src_mime.to_owned()));
    }

    let img = maybe_downscale(img, max_dim);
    let mut buf: Vec<u8> = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .with_context(|| "encoding resized texture as PNG")?;
    Ok((buf, "image/png".to_owned()))
}

fn maybe_downscale(img: DynamicImage, max_dim: u32) -> DynamicImage {
    if max_dim == 0 {
        return img;
    }
    let (w, h) = (img.width(), img.height());
    if w.max(h) <= max_dim {
        return img;
    }
    let scale = max_dim as f32 / w.max(h) as f32;
    let new_w = ((w as f32 * scale).round() as u32).max(1);
    let new_h = ((h as f32 * scale).round() as u32).max(1);
    img.resize(new_w, new_h, image::imageops::FilterType::Lanczos3)
}
