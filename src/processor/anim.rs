//! Animations: SEQS / STG_ / STC_ → glTF animations.
//!
//! ## M3 → glTF mapping
//!
//! Each STC (sub-animation) becomes one glTF animation. The name comes from
//! `STC.name` (a CHAR reference). A SEQS group gives us the time bounds
//! (`anim_ms_start..anim_ms_end`) and a group name — we don't use the group
//! for naming (glTF is flat), but `STG.stc_indices` tells us which STCs
//! belong to a SEQS, which is enough for `(group, action) → STC name`.
//!
//! `STC.anim_refs[i]` is the packed `(anim_type << 16) | anim_index`:
//!   `anim_type` indexes the `SDxx` array inside the STC (see m3studio
//!   io_m3_import.py:701-704); `anim_index` is the slot inside that array.
//!
//! Per-bone TRS lookup goes through `bone.location.header.id`,
//! `bone.rotation.header.id`, `bone.scale.header.id` → search them in
//! `STC.anim_ids`, take the matching `anim_refs[]`, decode `(type, index)`,
//! read SD3V (T/S) or SD4Q (R).
//!
//! **Root bones** receive the same Z-up→Y-up rotation as the rest pose
//! (see `glb/mod.rs::build_glb_content`). Specifically:
//!   - translation: `T' = R · T` (rotate the vector)
//!   - rotation:    `Q' = R_quat ⊗ Q` (compose quaternions)
//!   - scale:       unchanged
//!
//! Without this the root bone animates "past" the rotated mesh.

use crate::m3::reader::M3File;
use crate::m3::structures::{Bone, Sd3v, Sd4q};
use anyhow::Result;
use tracing::debug;

/// The node property an animation channel drives.
#[derive(Debug, Clone, Copy)]
pub enum Path {
    /// `translation` (VEC3).
    Translation,
    /// `rotation` (VEC4 quaternion, xyzw).
    Rotation,
    /// `scale` (VEC3).
    Scale,
}

/// A sampler's output values, one per input time.
#[derive(Debug, Clone)]
pub enum SamplerData {
    /// Translations or scales.
    Vec3(Vec<[f32; 3]>),
    /// Unit quaternions, hemisphere-aligned for linear interpolation.
    Quat(Vec<[f32; 4]>),
}

/// One keyframe track.
#[derive(Debug, Clone)]
pub struct Sampler {
    /// Frame timestamps in seconds (ms→s, divide by 1000).
    pub times_sec: Vec<f32>,
    /// Values at `times_sec`.
    pub data:      SamplerData,
    /// LINEAR vs STEP. For bone TRS we always use LINEAR — see
    /// `build_vec3_sampler` / `build_quat_sampler` for the reasoning.
    pub linear:    bool,
}

/// Binds a sampler to a bone node property.
#[derive(Debug, Clone)]
pub struct Channel {
    /// Index into [`Animation::samplers`].
    pub sampler:     usize,
    /// glTF node index of the bone.
    pub target_node: usize,
    /// Which property of that node.
    pub path:        Path,
}

/// One clip — an M3 `STC_` — in glTF terms.
#[derive(Debug, Clone)]
pub struct Animation {
    /// Clip name as authored (`Stand_full`, `Walk A_full`, …).
    pub name:     String,
    /// Keyframe tracks.
    pub samplers: Vec<Sampler>,
    /// Which bone property each track drives.
    pub channels: Vec<Channel>,
}

/// `anim_type` inside `anim_ref`:
///  0=sdev, 1=sd2v, 2=sd3v, 3=sd4q, 4=sdcc, 5=sdr3, 6=sdu8,
///  7=sds6, 8=sdu6, 9=sds3, 10=sdu3, 11=sdfg, 12=sdmb
const ANIM_TYPE_VEC3: u32 = 2;
const ANIM_TYPE_QUAT: u32 = 3;

/// `anim_id → (anim_type, anim_index)` map for a single STC.
struct StcLookup {
    map: ahash::AHashMap<u32, (u32, u32)>,
}

impl StcLookup {
    fn build(anim_ids: &[u32], anim_refs: &[u32]) -> Self {
        let mut map = ahash::AHashMap::with_capacity(anim_ids.len());
        for (id, r) in anim_ids.iter().zip(anim_refs.iter()) {
            let kind = (r >> 16) & 0xFFFF;
            let idx = r & 0xFFFF;
            map.insert(*id, (kind, idx));
        }
        Self { map }
    }

    fn lookup(&self, anim_id: u32) -> Option<(u32, u32)> {
        self.map.get(&anim_id).copied()
    }
}

/// Drop duplicate timestamps (m3studio does the same — io_m3_import.py:714-720).
/// Returns the times in seconds and the key indices to keep.
///
/// glTF requires sampler input to be *strictly* increasing. Real files only
/// ever repeat a timestamp, but a corrupt one can step backwards; such a key is
/// dropped rather than emitted as an invalid sampler.
pub(crate) fn dedupe_frames(frames_ms: &[i32]) -> (Vec<f32>, Vec<usize>) {
    let mut times: Vec<f32> = Vec::with_capacity(frames_ms.len());
    let mut keep = Vec::with_capacity(frames_ms.len());
    for (i, &ms) in frames_ms.iter().enumerate() {
        // Compared as the f32 that is written: two distinct i32 milliseconds
        // past ~4.6 hours round to the same f32.
        let t = ms as f32 / 1000.0;
        match times.last().map(|prev| t.total_cmp(prev)) {
            Some(std::cmp::Ordering::Less) => {}
            // m3studio keeps the *last* sample sharing the same frame.
            Some(std::cmp::Ordering::Equal) => {
                if let Some(last) = keep.last_mut() {
                    *last = i;
                }
            }
            _ => {
                times.push(t);
                keep.push(i);
            }
        }
    }
    (times, keep)
}

/// Main entry point. Parses SEQS/STG/STC out of every source and produces
/// the list of glTF-compatible animations.
///
/// `base` — main `.m3` file; bones and their `anim_id`s come from here.
/// `anim_sources` — every file (including `base` if it has its own SEQS,
/// and/or external `.m3a` files) that contributes SEQS/STG/STC and `SDxx` data.
/// `bone_node_base` — node index of the first bone in glTF nodes (= 0 in
/// the current glb/mod.rs layout).
///
/// # Errors
///
/// The base model's bone table cannot be read.
pub fn build_animations(
    base:           &M3File<'_>,
    anim_sources:   &[&M3File<'_>],
    bone_node_base: usize,
) -> Result<Vec<Animation>> {
    let bones = base.bones()?;
    if bones.is_empty() { return Ok(Vec::new()); }

    let mut anims: Vec<Animation> = Vec::new();

    for (src_idx, src) in anim_sources.iter().enumerate() {
        let seqs = src.sequences().unwrap_or_default();
        let stgs = src.sequence_groups().unwrap_or_default();
        let stcs = src.sequence_collections().unwrap_or_default();

        if seqs.is_empty() || stcs.is_empty() {
            debug!(
                "anim source #{}: SEQS={} STC={} — skipping",
                src_idx, seqs.len(), stcs.len()
            );
            continue;
        }

        debug!(
            "anim source #{}: SEQS={} STG={} STC={}",
            src_idx, seqs.len(), stgs.len(), stcs.len()
        );

        let mut emitted = vec![false; stcs.len()];

        // SEQS↔STG pair up by index (m3studio io_m3_import.py:680 zip).
        for (seq, stg) in seqs.iter().zip(stgs.iter()) {
            let group_name = src.read_char(&seq.name).unwrap_or("").to_owned();
            let stc_indices = src.read_ref_u32(&stg.stc_indices).unwrap_or_default();

            for stc_idx_u32 in stc_indices {
                let stc_idx = stc_idx_u32 as usize;
                if stc_idx >= stcs.len() { continue; }
                if emitted[stc_idx] { continue; }
                emitted[stc_idx] = true;

                if let Some(anim) = build_one_animation(
                    src, &stcs[stc_idx], &bones, &group_name, bone_node_base,
                ) {
                    anims.push(anim);
                }
            }
        }

        // STCs not bound to any STG.
        for (stc_idx, stc) in stcs.iter().enumerate() {
            if emitted[stc_idx] { continue; }
            if let Some(anim) = build_one_animation(src, stc, &bones, "", bone_node_base) {
                anims.push(anim);
            }
        }
    }

    Ok(anims)
}

fn build_one_animation(
    m3:             &M3File<'_>,
    stc:            &crate::m3::structures::Stc,
    bones:          &[Bone],
    group_name:     &str,
    bone_node_base: usize,
) -> Option<Animation> {
    let stc_name = m3.read_char(&stc.name).unwrap_or("").to_owned();
    if stc_name.is_empty() {
        debug!("STC: empty name, skipping");
        return None;
    }

    // m3studio computes `name.replace(group_name, '')[1:]` to derive a clean
    // action name, but for glTF it's better to keep the full name — it's
    // guaranteed unique and readable in editors.
    let _ = group_name;
    let anim_name = stc_name;

    let anim_ids = m3.read_ref_u32(&stc.anim_ids).unwrap_or_default();
    let anim_refs = m3.read_ref_u32(&stc.anim_refs).unwrap_or_default();
    if anim_ids.is_empty() || anim_refs.is_empty() {
        return None;
    }
    if anim_ids.len() != anim_refs.len() {
        debug!(
            "STC '{}': anim_ids({}) != anim_refs({}); skipping",
            anim_name, anim_ids.len(), anim_refs.len()
        );
        return None;
    }

    let lookup = StcLookup::build(&anim_ids, &anim_refs);
    let sd3v_arr: Vec<Sd3v> = m3.read_sd3v(&stc.sd3v).unwrap_or_default();
    let sd4q_arr: Vec<Sd4q> = m3.read_sd4q(&stc.sd4q).unwrap_or_default();

    let mut samplers: Vec<Sampler> = Vec::new();
    let mut channels: Vec<Channel> = Vec::new();

    for (bi, bone) in bones.iter().enumerate() {
        let target_node = bone_node_base + bi;
        let is_root = bone.parent_index(bi).is_none();

        // m3studio (`key_fcurves` in io_m3_import.py) filters only on the
        // presence of the anim_id in STC, not on header.flags — flags are
        // often 0 in the .m3 even for bones that *are* animated.
        let tracks = [
            (bone.location.header.id, ANIM_TYPE_VEC3, Path::Translation),
            (bone.rotation.header.id, ANIM_TYPE_QUAT, Path::Rotation),
            (bone.scale.header.id, ANIM_TYPE_VEC3, Path::Scale),
        ];
        for (anim_id, want_kind, path) in tracks {
            if anim_id == 0 {
                continue;
            }
            let Some((kind, idx)) = lookup.lookup(anim_id) else { continue };
            if kind != want_kind {
                continue;
            }
            let idx = idx as usize;
            // Only the root's translation and rotation carry the Z-up → Y-up
            // bake; scale is axis-independent of it.
            let sampler = match path {
                Path::Translation => sd3v_arr.get(idx).and_then(|b| build_vec3_sampler(m3, b, is_root)),
                Path::Rotation => sd4q_arr.get(idx).and_then(|b| build_quat_sampler(m3, b, is_root)),
                Path::Scale => sd3v_arr.get(idx).and_then(|b| build_vec3_sampler(m3, b, false)),
            };
            if let Some(samp) = sampler {
                channels.push(Channel { sampler: samplers.len(), target_node, path });
                samplers.push(samp);
            }
        }
    }

    if channels.is_empty() {
        debug!(
            "STC '{}': empty channels (no bone anim_id matched STC.anim_ids)",
            anim_name
        );
        return None;
    }

    debug!(
        "anim '{}': {} samplers, {} channels",
        anim_name, samplers.len(), channels.len()
    );

    Some(Animation { name: anim_name, samplers, channels })
}

fn build_vec3_sampler(m3: &M3File<'_>, block: &Sd3v, apply_zy: bool) -> Option<Sampler> {
    // m3studio (`io_m3_import.py:577`/`603`) hardcodes LINEAR for bone
    // translation/scale FCurves regardless of `header.interpolation`.
    // The M3 field reads 0 ("constant") for essentially every bone
    // channel — it's a vestigial engine flag, not a real instruction.
    // Honoring it would emit STEP everywhere and produce frame-by-frame
    // pop instead of smooth motion.
    let frames_ms = m3.read_ref_i32(&block.frames).unwrap_or_default();
    let values    = m3.read_ref_vec3(&block.keys).unwrap_or_default();
    if frames_ms.is_empty() || values.is_empty() { return None; }
    let n = frames_ms.len().min(values.len());
    let frames_ms = &frames_ms[..n];
    let values = &values[..n];

    let (times_sec, keep) = dedupe_frames(frames_ms);
    let mut data: Vec<[f32; 3]> = Vec::with_capacity(keep.len());
    for &i in &keep {
        let v = values[i];
        let arr = if apply_zy {
            crate::quat::rotate([v.x, v.y, v.z], crate::quat::Z_UP_TO_Y_UP)
        } else {
            [v.x, v.y, v.z]
        };
        data.push(arr);
    }

    Some(Sampler {
        times_sec,
        data: SamplerData::Vec3(data),
        linear: true,
    })
}

fn build_quat_sampler(m3: &M3File<'_>, block: &Sd4q, apply_zy: bool) -> Option<Sampler> {
    // See note in `build_vec3_sampler`: bone rotation always interpolates
    // LINEAR. m3studio does the same (`io_m3_import.py:590`).
    let frames_ms = m3.read_ref_i32(&block.frames).unwrap_or_default();
    let values    = m3.read_ref_quat(&block.keys).unwrap_or_default();
    if frames_ms.is_empty() || values.is_empty() { return None; }
    let n = frames_ms.len().min(values.len());
    let frames_ms = &frames_ms[..n];
    let values = &values[..n];

    let (times_sec, keep) = dedupe_frames(frames_ms);
    let mut data: Vec<[f32; 4]> = Vec::with_capacity(keep.len());
    for &i in &keep {
        let q = values[i];
        let raw = if apply_zy {
            crate::quat::mul(crate::quat::Z_UP_TO_Y_UP, [q.x, q.y, q.z, q.w])
        } else {
            [q.x, q.y, q.z, q.w]
        };
        data.push(crate::quat::normalize(raw));
    }

    // Quaternion sign correction: q and -q represent the same rotation, but
    // glTF LINEAR interpolation lerps component-wise. If two consecutive
    // samples land on opposite hemispheres (dot < 0), lerp passes through
    // (or near) identity and the bone snaps the long way around. Mirror the
    // hemisphere of every sample to its predecessor.
    for i in 1..data.len() {
        let prev = data[i - 1];
        let cur  = data[i];
        let dot = prev[0] * cur[0] + prev[1] * cur[1] + prev[2] * cur[2] + prev[3] * cur[3];
        if dot < 0.0 {
            data[i] = [-cur[0], -cur[1], -cur[2], -cur[3]];
        }
    }

    Some(Sampler {
        times_sec,
        data: SamplerData::Quat(data),
        linear: true,
    })
}

