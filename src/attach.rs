//! M3 attachment points (`ATT_`, `ATVL`) → glTF node `extras`.
//!
//! An attachment point is where a game hangs something on a model: a weapon in
//! `Ref_Weapon Right`, a status effect over `Ref_Overhead`, a projectile spawn
//! at `Ref_Hand Left`. In M3 it is a **bone plus a name**, and the bone is
//! already exported as a glTF node — animated, parented, in the right place. So
//! unlike effects (see [`crate::fx`]) nothing new has to be emitted here: the
//! attachment's name is written into the `extras` of the bone node that carries
//! it, under the key `m3attach`.
//!
//! ## Why the ATT_ table is read at all
//!
//! Because the bone's own name is *not* reliably the attachment's name. On
//! `Storm_Hero_Chromie_Spring19` 28 of the 30 attachment points sit on a bone
//! named exactly like them, but `Ref_Target` rides a bone called `Vol_Target`
//! and `Ref_Shield` rides `Vol_Shield`. An engine matching on `Ref_*` node
//! names silently loses those two — which are precisely the ones a game needs,
//! since they are the targeting and shield-impact points.
//!
//! ## Volumes
//!
//! `ATVL` binds a shape (sphere / cuboid / cylinder) to the same bone: the hit
//! or target volume for that attachment. When one matches, it rides along in
//! the same object under `volume`.
//!
//! ```text
//! Bone_Head                       extras.m3attach = {"name":"Ref_Head"}
//! Vol_Target                      extras.m3attach = {"name":"Ref_Target",
//!                                                    "volume":{"shape":"sphere",…}}
//! ```
//!
//! Not exported: `attachment_points_addon` — the parallel `U16_` list that
//! numbers same-named attachments (`Ref_Weapon` ×3). Each attachment already
//! has its own node, so the disambiguation an engine needs is the node itself.

use crate::fx::Obj;
use crate::m3::reader::M3File;
use crate::m3::structures::Atvl;
use tracing::debug;

/// `Atvl.shape`, per structures.xml.
const SHAPES: [&str; 3] = ["cuboid", "sphere", "cylinder"];

/// A volume bound to the same bone as an attachment point.
#[derive(Debug, Clone, Copy)]
pub struct Volume {
    /// Shape name, or `"unknown"` for a value structures.xml does not name.
    pub shape:  &'static str,
    /// Raw M3 size triple. Its meaning follows `shape`: radius for a sphere,
    /// extents for a cuboid, radius + height for a cylinder.
    pub size:   [f32; 3],
    /// Bone-local transform of the volume. Stored in glTF's own column-major
    /// order (translation at 12..15), so it feeds `Mat4::from_cols_array`
    /// unchanged.
    pub matrix: [f32; 16],
}

/// One attachment point, resolved against the bone array.
#[derive(Debug, Clone)]
pub struct Attachment {
    /// Attachment name as authored — `Ref_Head`, `Ref_Weapon Right`, …
    pub name:   String,
    /// Index into the bone array, which is also the glTF node index.
    pub bone:   usize,
    pub volume: Option<Volume>,
}

impl Attachment {
    /// The `extras` object for this attachment — `{"m3attach":{…}}`.
    #[must_use]
    pub fn extras_json(&self) -> String {
        let mut body = Obj::new();
        body.string("name", &self.name);
        if let Some(v) = self.volume {
            let mut vol = Obj::new();
            vol.string("shape", v.shape);
            vol.vec3("size", v.size);
            vol.raw("matrix", &mat_json(&v.matrix));
            body.raw("volume", &vol.finish());
        }

        let mut j = Obj::new();
        j.raw("m3attach", &body.finish());
        j.finish()
    }
}

/// Read every attachment point in the file, dropping the ones whose bone index
/// is out of range (`bone_count` is the caller's — the bone array it will
/// actually emit).
#[must_use]
pub fn collect(m3: &M3File<'_>, bone_count: usize) -> Vec<Attachment> {
    let atts = m3.attachment_points().unwrap_or_default();
    if atts.is_empty() {
        return Vec::new();
    }
    let volumes = m3.attachment_volumes().unwrap_or_default();

    let mut out = Vec::with_capacity(atts.len());
    for att in &atts {
        let bone = att.bone as usize;
        if bone >= bone_count {
            debug!("ATT_ bone {} out of range ({} bones) — skipped", bone, bone_count);
            continue;
        }
        let name = m3.read_char(&att.name).unwrap_or("").to_owned();
        if name.is_empty() {
            continue;
        }
        // ATVL stores the bone three times (bone0/bone1/bone2 are equal on
        // every file inspected); bone0 is what binds it to the attachment.
        let volume = volumes
            .iter()
            .find(|v| v.bone0 as usize == bone)
            .map(volume_of);
        out.push(Attachment { name, bone, volume });
    }
    out
}

fn volume_of(v: &Atvl) -> Volume {
    let m = v.matrix;
    Volume {
        shape:  SHAPES.get(v.shape as usize).copied().unwrap_or("unknown"),
        size:   [v.size0, v.size1, v.size2],
        matrix: [
            m.x.x, m.x.y, m.x.z, m.x.w,
            m.y.x, m.y.y, m.y.z, m.y.w,
            m.z.x, m.z.y, m.z.z, m.z.w,
            m.w.x, m.w.y, m.w.z, m.w.w,
        ],
    }
}

fn mat_json(m: &[f32; 16]) -> String {
    let mut s = String::from("[");
    for (i, v) in m.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&crate::fx::num(*v));
    }
    s.push(']');
    s
}
