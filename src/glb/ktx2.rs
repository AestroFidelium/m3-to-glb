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
//! Per-texture role drives the OETF tag, and for normal maps it also
//! triggers a channel unswizzle (HotS source files use a Blizzard
//! variant of DXT5nm with X in alpha, Y in green, R/B unused — see
//! [`unpack_blizzard_normal`]):
//!
//!   - [`TextureRole::Color`]     → UASTC + Zstd, OETF `sRGB`.
//!   - [`TextureRole::NormalMap`] → UASTC + Zstd, OETF `linear`,
//!     channels rewritten to standard glTF layout (R=X, G=Y, B=Z=√(1-X²-Y²)).
//!   - [`TextureRole::Data`]      → UASTC + Zstd, OETF `linear`,
//!     channels untouched (occlusion, metallicRoughness, etc.).
//!
//! Output is suitable for the `KHR_texture_basisu` glTF extension;
//! engines such as Bevy or three.js transcode the basis-encoded data
//! into the platform-native format (BC7/BC5/ASTC/ETC2) at load time,
//! keeping the texture compressed in VRAM.

use anyhow::{Context, Result, anyhow, bail};
use image::DynamicImage;
use std::path::Path;
use std::process::Command;

/// Role a texture plays in the material — drives the OETF tag and (for
/// normal maps) the channel unswizzle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureRole {
    /// sRGB color data (baseColor, emissive). UASTC + Zstd, OETF tagged
    /// sRGB so the GPU sampler gamma-decodes at sample time.
    Color,
    /// Tangent-space normal map. UASTC + Zstd, OETF tagged linear, and
    /// channels are unswizzled from Blizzard's DXT5nm variant
    /// (R=unused, G=Y, B=unused, A=X) into standard glTF layout
    /// (R=X, G=Y, B=Z reconstructed) before encoding.
    NormalMap,
    /// Other linear data channels (occlusion, metallicRoughness, …).
    /// UASTC + Zstd, OETF tagged linear, channels untouched.
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

    // Decode → optional Blizzard normal unswizzle → optional resize →
    // re-encode as PNG (toktx ingests PNG/JPEG/EXR).
    let img = image::open(input_path)
        .with_context(|| format!("decoding source texture {:?}", input_path))?;
    let img = if opts.role == TextureRole::NormalMap {
        unpack_blizzard_normal(img)
    } else {
        img
    };
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
        TextureRole::NormalMap | TextureRole::Data => "linear",
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

/// Read a texture file for the non-KTX2 embedding path.
///
/// For `Color` and `Data` roles: when no resize is needed (or `max_dim
/// == 0`), the source bytes are returned byte-for-byte with the
/// original MIME preserved; otherwise decode → Lanczos3 → re-encode as
/// PNG (MIME switches to `image/png`).
///
/// For `NormalMap` role the source can never pass through unchanged —
/// HotS DDS/PNG normal maps store X in alpha and reserve R/B for other
/// data, so they must always be decoded and unswizzled into standard
/// glTF layout before being embedded.
pub fn read_with_optional_downscale(
    input_path: &Path,
    role:       TextureRole,
    max_dim:    u32,
    src_mime:   &str,
) -> Result<(Vec<u8>, String)> {
    let needs_decode = role == TextureRole::NormalMap;

    if !needs_decode && max_dim == 0 {
        let bytes = std::fs::read(input_path)
            .with_context(|| format!("reading texture {:?}", input_path))?;
        return Ok((bytes, src_mime.to_owned()));
    }

    let img = image::open(input_path)
        .with_context(|| format!("decoding source texture {:?}", input_path))?;

    if !needs_decode && img.width().max(img.height()) <= max_dim {
        let bytes = std::fs::read(input_path)
            .with_context(|| format!("reading texture {:?}", input_path))?;
        return Ok((bytes, src_mime.to_owned()));
    }

    let img = if role == TextureRole::NormalMap {
        unpack_blizzard_normal(img)
    } else {
        img
    };
    let img = maybe_downscale(img, max_dim);

    let mut buf: Vec<u8> = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .with_context(|| "encoding texture as PNG")?;
    Ok((buf, "image/png".to_owned()))
}

/// Convert a Blizzard-packed HotS/SC2 normal map into the standard glTF
/// layout (R=X, G=Y, B=Z, A=255).
///
/// The HotS DDS/PNG convention stores tangent-space normals as
/// `R=unused (~255), G=Y, B=unused (~0), A=X` — see `*_norm.dds` files.
/// Reading that as a glTF normal map gives a junk vector roughly
/// `(+1, ±0, -1)` per pixel, which produces near-zero `dot(L,N)` and a
/// matte / unlit-looking surface.
///
/// Reconstruction:
///
/// ```text
///   nx = A * 2 - 1                    (in [-1, 1])
///   ny = G * 2 - 1                    (in [-1, 1])
///   nz = sqrt(max(0, 1 - nx² - ny²))  (in [0, 1], outward)
///   out.R = A           ((nx+1)/2 → glTF X)
///   out.G = G           ((ny+1)/2 → glTF Y)
///   out.B = (nz+1)/2 * 255            (glTF Z, always in [127, 255])
///   out.A = 255
/// ```
fn unpack_blizzard_normal(img: DynamicImage) -> DynamicImage {
    let mut rgba = img.into_rgba8();
    for px in rgba.pixels_mut() {
        let a = px.0[3];
        let g = px.0[1];
        let nx = (a as f32) / 127.5 - 1.0;
        let ny = (g as f32) / 127.5 - 1.0;
        let nz = (1.0 - nx * nx - ny * ny).max(0.0).sqrt();
        // ((nz + 1) / 2) * 255 — encoded outward Z, always >= 127.
        let b = ((nz + 1.0) * 127.5).round().clamp(0.0, 255.0) as u8;
        px.0 = [a, g, b, 255];
    }
    DynamicImage::ImageRgba8(rgba)
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
