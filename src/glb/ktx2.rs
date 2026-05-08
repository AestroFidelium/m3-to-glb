//! KTX2/UASTC texture transcoder.
//!
//! Shells out to `toktx` (KTX-Software) to encode an existing PNG/DDS/TGA/JPEG
//! into KTX2 with UASTC + Zstd supercompression and a generated mipmap chain.
//! The output is suitable for the `KHR_texture_basisu` glTF extension; engines
//! such as Bevy or three.js transcode UASTC into the platform-native format
//! (BC7 / ASTC / ETC2) at load time, keeping the texture compressed in VRAM.

use anyhow::{Context, Result, anyhow, bail};
use std::path::Path;
use std::process::Command;

/// Transcode an arbitrary supported texture file into a KTX2/UASTC byte
/// blob, ready to embed in the GLB buffer.
///
/// Workflow:
///   1. Decode the input via the `image` crate — PNG/DDS/TGA/JPEG are
///      supported. Anything else returns an error and the caller falls
///      back to embedding the original file unchanged.
///   2. Re-encode as PNG into a temp file (`toktx` only ingests PNG/JPEG/EXR).
///   3. Run `toktx --t2 --uastc --uastc_quality 2 --zcmp 18 --genmipmap …`.
///   4. Read the resulting KTX2 bytes.
pub fn transcode(input_path: &Path) -> Result<Vec<u8>> {
    let tmp = tempfile::tempdir().context("creating tempdir for KTX2 transcode")?;
    let png_path = tmp.path().join("input.png");
    let ktx2_path = tmp.path().join("output.ktx2");

    let img = image::open(input_path)
        .with_context(|| format!("decoding source texture {:?}", input_path))?;
    img.save_with_format(&png_path, image::ImageFormat::Png)
        .with_context(|| format!("writing temp PNG {:?}", png_path))?;

    let status = Command::new("toktx")
        .args([
            "--t2",
            "--uastc",
            "--uastc_quality",
            "2",
            "--zcmp",
            "18",
            "--genmipmap",
        ])
        .arg(&ktx2_path)
        .arg(&png_path)
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
