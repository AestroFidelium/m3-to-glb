//! KTX2 texture transcoder.
//!
//! Shells out to `toktx` (KTX-Software) to encode an existing PNG/DDS/TGA/JPEG
//! into KTX2/UASTC + Zstd with a generated mip chain. UASTC preserves
//! per-channel precision so it works for both color and data (normal /
//! occlusion / metallicRoughness) textures; the Bevy 0.17 KTX2 reader
//! (`bevy_image`) only supports `None` and `Zstandard` supercompression
//! schemes — `BasisLZ` (the native scheme for the more compact ETC1S
//! mode) is rejected — so UASTC + Zstd is the one path that actually
//! decodes there.
//!
//! Per-texture role only differentiates the OETF tag:
//!
//!   - [`TextureRole::Color`] → UASTC + Zstd, OETF `sRGB`. baseColor /
//!     emissive — color the GPU should gamma-decode at sample time.
//!   - [`TextureRole::Data`]  → UASTC + Zstd, OETF `linear`. Normal /
//!     occlusion / metallicRoughness — raw linear values; no gamma
//!     decode at sample time (which would otherwise corrupt the data).
//!
//! Output is suitable for the `KHR_texture_basisu` glTF extension;
//! engines such as Bevy or three.js transcode the basis-encoded data
//! into the platform-native format (BC7/BC5/ASTC/ETC2) at load time,
//! keeping the texture compressed in VRAM.

use anyhow::{Context, Result, anyhow, bail};
use image::DynamicImage;
use std::path::Path;
use std::process::Command;

/// Role a texture plays in the material — drives the OETF tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureRole {
    /// sRGB color data (baseColor, emissive). UASTC + Zstd, OETF tagged
    /// sRGB so the GPU sampler gamma-decodes at sample time.
    Color,
    /// Linear data channel (normal, occlusion, metallicRoughness).
    /// UASTC + Zstd, OETF tagged linear.
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
    cmd.arg("--t2");
    cmd.arg("--genmipmap");
    cmd.args(["--encode", "uastc"]);
    cmd.args(["--uastc_quality", "2"]);
    // Zstd supercompression on the mip levels. Bevy 0.17's bevy_image
    // accepts `None` and `Zstandard` only; BasisLZ (the native scheme
    // for ETC1S) is rejected — see bevy_image/src/ktx2.rs.
    cmd.args(["--zcmp", "18"]);
    // OETF is the one knob that differs by role. toktx defaults to sRGB
    // on PNG input; without forcing linear for normal / occlusion the
    // GPU sampler would gamma-decode the data and produce wrong values.
    let oetf = match opts.role {
        TextureRole::Color => "srgb",
        TextureRole::Data  => "linear",
    };
    cmd.args(["--assign_oetf", oetf]);
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
