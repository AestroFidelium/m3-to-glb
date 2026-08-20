//! Animated emitter values — `STC_` sequence data resolved into plain curves.
//!
//! Emitter fields in M3 are [`FloatAnimationReference`]-shaped: a static
//! default plus an `anim_id` that the sequence data may drive. For particle
//! systems the default alone is usually *not* what the effect looks like — a
//! HotS ability effect commonly ships `emit_rate` default `0` and animates it
//! to a burst inside one sequence. Exporting only defaults would hand the
//! engine an emitter that never spawns anything.
//!
//! So the fields whose curves decide whether an effect is visible at all are
//! resolved here, per sequence, and written alongside the defaults. The lookup
//! is the same one [`crate::processor::anim`] performs for bone TRS —
//! `anim_id` → `STC.anim_refs` → `(type, index)` → the typed SD block — only
//! for float and vec3 tracks instead of translation/rotation/scale.
//!
//! [`FloatAnimationReference`]: crate::m3::structures::FloatAnimationReference

use crate::m3::reader::M3File;
use crate::m3::structures::Reference;

/// One track's samples, borrowed from the sequence that drives it, paired with
/// that sequence's name.
pub type Track<'a, T> = Vec<(&'a str, &'a [(f32, T)])>;

/// `anim_type` values inside a packed `anim_ref`, per m3studio.
const ANIM_TYPE_VEC3: u32 = 2;
const ANIM_TYPE_REAL: u32 = 5;
const ANIM_TYPE_INT16: u32 = 7;
const ANIM_TYPE_UINT16: u32 = 8;

/// Longest curve written into `extras`. Emitter tracks are short — a burst is
/// three or four keys — but a hand-authored one can be long, and the JSON chunk
/// is parsed on every load. Longer curves are decimated, endpoints kept.
const MAX_KEYS: usize = 48;

/// Every sequence's curves, in the order the sequences appear in the glTF.
#[derive(Default)]
pub struct FxCurves {
    seqs: Vec<SeqCurves>,
}

struct SeqCurves {
    /// Sequence name — the same string the glTF animation carries, so a runtime
    /// can pair a curve with the animation it belongs to.
    name: String,
    /// `anim_id` → packed `(anim_type, index)`.
    lookup: ahash::AHashMap<u32, (u32, u32)>,
    /// Float tracks, already decoded: `(seconds, value)`.
    real: Vec<Vec<(f32, f32)>>,
    /// Vec3 tracks, already decoded.
    vec3: Vec<Vec<(f32, [f32; 3])>>,
    /// 16-bit integer tracks, widened to float. A burst emitter's particle
    /// count lives here rather than in a float track.
    int16: Vec<Vec<(f32, f32)>>,
    uint16: Vec<Vec<(f32, f32)>>,
}

impl FxCurves {
    /// Resolve every sequence in the model. Cheap enough to do unconditionally:
    /// the SD blocks are already in the mmap and each is a pair of slices.
    pub fn build(m3: &M3File<'_>) -> Self {
        let mut seqs = Vec::new();
        for stc in m3.sequence_collections().unwrap_or_default() {
            let name = m3.read_char(&stc.name).unwrap_or("").to_owned();
            if name.is_empty() {
                continue;
            }
            let anim_ids = m3.read_ref_u32(&stc.anim_ids).unwrap_or_default();
            let anim_refs = m3.read_ref_u32(&stc.anim_refs).unwrap_or_default();
            if anim_ids.len() != anim_refs.len() || anim_ids.is_empty() {
                continue;
            }
            let lookup = anim_ids
                .iter()
                .zip(anim_refs.iter())
                .map(|(id, r)| (*id, ((r >> 16) & 0xFFFF, r & 0xFFFF)))
                .collect();

            let real = m3
                .read_sdr3(&stc.sdr3)
                .unwrap_or_default()
                .iter()
                .map(|b| decode_real(m3, &b.frames, &b.keys))
                .collect();
            let vec3 = m3
                .read_sd3v(&stc.sd3v)
                .unwrap_or_default()
                .iter()
                .map(|b| decode_vec3(m3, &b.frames, &b.keys))
                .collect();

            let int16 = m3
                .read_sds6(&stc.sds6)
                .unwrap_or_default()
                .iter()
                .map(|b| decode_int(m3, &b.frames, &b.keys, false))
                .collect();
            let uint16 = m3
                .read_sdu6(&stc.sdu6)
                .unwrap_or_default()
                .iter()
                .map(|b| decode_int(m3, &b.frames, &b.keys, true))
                .collect();

            seqs.push(SeqCurves { name, lookup, real, vec3, int16, uint16 });
        }
        Self { seqs }
    }

    /// Whether anything at all was resolved.
    pub fn is_empty(&self) -> bool {
        self.seqs.is_empty()
    }

    /// The float curve for `anim_id` in each sequence that drives it.
    pub fn real(&self, anim_id: u32) -> Track<'_, f32> {
        if anim_id == 0 {
            return Vec::new();
        }
        self.seqs
            .iter()
            .filter_map(|s| match s.lookup.get(&anim_id) {
                Some(&(ANIM_TYPE_REAL, idx)) => {
                    let c = s.real.get(idx as usize)?;
                    (!c.is_empty()).then_some((s.name.as_str(), c.as_slice()))
                }
                _ => None,
            })
            .collect()
    }

    /// The integer curve for `anim_id` in each sequence that drives it,
    /// widened to float. Used for counts, which M3 stores as 16-bit.
    pub fn int(&self, anim_id: u32) -> Track<'_, f32> {
        if anim_id == 0 {
            return Vec::new();
        }
        self.seqs
            .iter()
            .filter_map(|s| {
                let c = match s.lookup.get(&anim_id) {
                    Some(&(ANIM_TYPE_INT16, idx)) => s.int16.get(idx as usize)?,
                    Some(&(ANIM_TYPE_UINT16, idx)) => s.uint16.get(idx as usize)?,
                    _ => return None,
                };
                (!c.is_empty()).then_some((s.name.as_str(), c.as_slice()))
            })
            .collect()
    }

    /// The vec3 curve for `anim_id` in each sequence that drives it.
    pub fn vec3(&self, anim_id: u32) -> Track<'_, [f32; 3]> {
        if anim_id == 0 {
            return Vec::new();
        }
        self.seqs
            .iter()
            .filter_map(|s| match s.lookup.get(&anim_id) {
                Some(&(ANIM_TYPE_VEC3, idx)) => {
                    let c = s.vec3.get(idx as usize)?;
                    (!c.is_empty()).then_some((s.name.as_str(), c.as_slice()))
                }
                _ => None,
            })
            .collect()
    }
}

/// The peak of a float track across every sequence, and the window it is
/// non-zero for in the sequence where it peaks.
///
/// This is what turns an animated `emit_rate` back into something a spawner can
/// use without replaying the curve: `(peak, delay, duration)` describes the
/// burst — how fast, how long after the sequence starts, and for how long.
#[must_use]
pub fn peak_window(curves: &[(&str, &[(f32, f32)])]) -> Option<(f32, f32, f32)> {
    let mut best: Option<(f32, f32, f32)> = None;
    for (_, c) in curves {
        let peak = c.iter().fold(0.0_f32, |m, &(_, v)| m.max(v));
        if peak <= 0.0 {
            continue;
        }
        // The active window is from the first non-zero key to the last one. A
        // key that is exactly zero is the emitter being switched off, which is
        // how these curves are authored.
        let first = c.iter().position(|&(_, v)| v > 0.0);
        let last = c.iter().rposition(|&(_, v)| v > 0.0);
        let (Some(first), Some(last)) = (first, last) else { continue };
        // A burst's last non-zero key is not the moment it stops — the following
        // zero key is. Extend to it when there is one.
        let end = c.get(last + 1).map_or(c[last].0, |&(t, _)| t);
        let start = if first == 0 { c[0].0 } else { c[first - 1].0 };
        if best.is_none_or(|(p, _, _)| peak > p) {
            best = Some((peak, start, (end - start).max(0.0)));
        }
    }
    best
}

/// `[[t,v],…]`, decimated to [`MAX_KEYS`].
#[must_use]
pub fn real_json(curve: &[(f32, f32)]) -> String {
    let mut out = String::from("[");
    for (i, (t, v)) in decimate(curve).into_iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!("[{},{}]", super::num(t), super::num(v)));
    }
    out.push(']');
    out
}

/// `[[t,[x,y,z]],…]`, decimated to [`MAX_KEYS`].
#[must_use]
pub fn vec3_json(curve: &[(f32, [f32; 3])]) -> String {
    let mut out = String::from("[");
    for (i, (t, v)) in decimate(curve).into_iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "[{},[{},{},{}]]",
            super::num(t),
            super::num(v[0]),
            super::num(v[1]),
            super::num(v[2])
        ));
    }
    out.push(']');
    out
}

/// Keep at most [`MAX_KEYS`] evenly spaced samples, always including the last
/// one — dropping the end of a curve would change where the effect stops.
fn decimate<T: Copy>(curve: &[T]) -> Vec<T> {
    if curve.len() <= MAX_KEYS {
        return curve.to_vec();
    }
    let step = curve.len().div_ceil(MAX_KEYS);
    let mut out: Vec<T> = curve.iter().copied().step_by(step).collect();
    if let Some(&last) = curve.last() {
        out.push(last);
    }
    out
}

// ─── SD block decoding ───────────────────────────────────────────────────────

/// Frames are milliseconds (`I32_`); values are a parallel array. A block whose
/// two arrays disagree in length is truncated to the shorter one rather than
/// dropped — a partially readable curve still beats a static zero.
fn decode_real(m3: &M3File<'_>, frames: &Reference, keys: &Reference) -> Vec<(f32, f32)> {
    let f = m3.read_ref_i32(frames).unwrap_or_default();
    let v = m3.read_ref_f32(keys).unwrap_or_default();
    f.iter()
        .zip(v.iter())
        .map(|(&ms, &val)| (ms as f32 / 1000.0, if val.is_finite() { val } else { 0.0 }))
        .collect()
}

/// SDS6 / SDU6 keys, widened to float so counts share the float curve shape.
fn decode_int(m3: &M3File<'_>, frames: &Reference, keys: &Reference, unsigned: bool) -> Vec<(f32, f32)> {
    let f = m3.read_ref_i32(frames).unwrap_or_default();
    let v: Vec<f32> = if unsigned {
        m3.read_ref_u16(keys).unwrap_or_default().into_iter().map(f32::from).collect()
    } else {
        m3.read_ref_i16(keys).unwrap_or_default().into_iter().map(f32::from).collect()
    };
    f.iter().zip(v.iter()).map(|(&ms, &val)| (ms as f32 / 1000.0, val)).collect()
}

fn decode_vec3(m3: &M3File<'_>, frames: &Reference, keys: &Reference) -> Vec<(f32, [f32; 3])> {
    let f = m3.read_ref_i32(frames).unwrap_or_default();
    let v = m3.read_ref_vec3(keys).unwrap_or_default();
    f.iter()
        .zip(v.iter())
        .map(|(&ms, p)| {
            let fin = |x: f32| if x.is_finite() { x } else { 0.0 };
            (ms as f32 / 1000.0, [fin(p.x), fin(p.y), fin(p.z)])
        })
        .collect()
}
