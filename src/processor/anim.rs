//! Анимации: SEQS / STG_ / STC_ → glTF animations.
//!
//! ## Маппинг M3 → glTF
//!
//! Каждый STC (sub-animation) соответствует одной glTF animation. Имя берём
//! из `STC.name` (CHAR ссылки). Группа SEQS даёт нам границы времени
//! (`anim_ms_start..anim_ms_end`) и общее имя группы — мы его не используем
//! для именования (glTF плоский), но через STG.stc_indices мы узнаём, какие
//! STC принадлежат данной SEQS, и этого достаточно для пары
//! `(group, action) → STC name`.
//!
//! `STC.anim_refs[i]` — это упакованный `(anim_type << 16) | anim_index`:
//!   anim_type индексирует массив SDxx внутри STC (см. m3studio
//!   io_m3_import.py:701-704), a anim_index — это позиция в этом массиве.
//!
//! Per-bone TRS привязка идёт через `bone.location.header.id`,
//! `bone.rotation.header.id`, `bone.scale.header.id` → ищем в `STC.anim_ids`,
//! берём соответствующий `anim_refs[]`, расшифровываем `(type, index)`,
//! читаем SD3V (для T/S) или SD4Q (для R).
//!
//! **Корневые кости** получают тот же поворот Z-up→Y-up, что и в TRS rest pose
//! (см. `glb/mod.rs::build_glb_content`). Это:
//!   - translation: T' = R · T (поворот вектора)
//!   - rotation:    Q' = R_quat ⊗ Q (композиция кватернионов)
//!   - scale:       без изменений
//! Если этого не сделать, корневые кости анимируются "мимо" повёрнутого меша.

use crate::m3::reader::M3File;
use crate::m3::structures::{Bone, Sd3v, Sd4q};
use anyhow::Result;
use tracing::debug;

/// Z-up → Y-up rotation (rotate -90° around X), применяется к корневым костям.
const ZY_QUAT: [f32; 4] = [
    -std::f32::consts::FRAC_1_SQRT_2,
    0.0,
    0.0,
    std::f32::consts::FRAC_1_SQRT_2,
];

#[derive(Debug, Clone, Copy)]
pub enum Path {
    Translation,
    Rotation,
    Scale,
}

#[derive(Debug, Clone)]
pub enum SamplerData {
    Vec3(Vec<[f32; 3]>),
    Quat(Vec<[f32; 4]>),
}

#[derive(Debug, Clone)]
pub struct Sampler {
    /// Время кадров в секундах (ms→s, делим на 1000).
    pub times_sec: Vec<f32>,
    pub data:      SamplerData,
    /// 0 = STEP (constant), 1 = LINEAR. По умолчанию LINEAR.
    pub linear:    bool,
}

#[derive(Debug, Clone)]
pub struct Channel {
    pub sampler:     usize,
    pub target_node: usize, // нодовый индекс кости
    pub path:        Path,
}

#[derive(Debug, Clone)]
pub struct Animation {
    pub name:     String,
    pub samplers: Vec<Sampler>,
    pub channels: Vec<Channel>,
}

/// anim_type внутри anim_ref:
///  0=sdev, 1=sd2v, 2=sd3v, 3=sd4q, 4=sdcc, 5=sdr3, 6=sdu8,
///  7=sds6, 8=sdu6, 9=sds3, 10=sdu3, 11=sdfg, 12=sdmb
const ANIM_TYPE_VEC3: u32 = 2;
const ANIM_TYPE_QUAT: u32 = 3;

/// Карта `anim_id → (anim_type, anim_index)` для одного STC.
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

/// Вычитает повторяющиеся таймштампы (m3studio делает то же — io_m3_import.py:714-720).
/// Возвращает индексы, которые нужно сохранить.
fn dedupe_frames(frames_ms: &[i32]) -> (Vec<f32>, Vec<usize>) {
    let mut times = Vec::with_capacity(frames_ms.len());
    let mut keep = Vec::with_capacity(frames_ms.len());
    let mut prev: Option<i32> = None;
    for (i, &ms) in frames_ms.iter().enumerate() {
        if Some(ms) == prev {
            // m3studio оставляет последнюю занимающую тот же кадр запись —
            // мы делаем то же: переписываем последний keep на текущий index
            if let Some(last) = keep.last_mut() {
                *last = i;
            }
        } else {
            times.push(ms as f32 / 1000.0);
            keep.push(i);
            prev = Some(ms);
        }
    }
    (times, keep)
}

#[inline]
fn quat_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let (ax, ay, az, aw) = (a[0], a[1], a[2], a[3]);
    let (bx, by, bz, bw) = (b[0], b[1], b[2], b[3]);
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

#[inline]
fn rotate_vec_by_quat(v: [f32; 3], q: [f32; 4]) -> [f32; 3] {
    let (qx, qy, qz, qw) = (q[0], q[1], q[2], q[3]);
    let tx = 2.0 * (qy * v[2] - qz * v[1]);
    let ty = 2.0 * (qz * v[0] - qx * v[2]);
    let tz = 2.0 * (qx * v[1] - qy * v[0]);
    [
        v[0] + qw * tx + (qy * tz - qz * ty),
        v[1] + qw * ty + (qz * tx - qx * tz),
        v[2] + qw * tz + (qx * ty - qy * tx),
    ]
}

/// Главная точка входа. Парсит SEQS/STG/STC из каждого источника и собирает
/// список glTF-совместимых анимаций.
///
/// `base` — главный .m3 файл, из которого берутся кости и их `anim_id`.
/// `anim_sources` — все файлы (включая `base`, если у него есть свои SEQS,
/// и/или внешние `.m3a`), из которых читаются SEQS/STG/STC и SDxx данные.
/// `bone_node_base` — индекс ноды первой кости в массиве glTF nodes (= 0
/// в текущей раскладке glb/mod.rs).
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
                "anim source #{}: SEQS={} STC={} — пропускаю",
                src_idx, seqs.len(), stcs.len()
            );
            continue;
        }

        debug!(
            "anim source #{}: SEQS={} STG={} STC={}",
            src_idx, seqs.len(), stgs.len(), stcs.len()
        );

        let mut emitted = vec![false; stcs.len()];

        // Пара SEQS↔STG идёт по index (m3studio io_m3_import.py:680 zip).
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
                )? {
                    anims.push(anim);
                }
            }
        }

        // STC, не попавшие ни в одну STG.
        for (stc_idx, stc) in stcs.iter().enumerate() {
            if emitted[stc_idx] { continue; }
            if let Some(anim) = build_one_animation(src, stc, &bones, "", bone_node_base)? {
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
) -> Result<Option<Animation>> {
    let stc_name = m3.read_char(&stc.name).unwrap_or("").to_owned();
    if stc_name.is_empty() {
        debug!("STC: пустое имя, пропускаю");
        return Ok(None);
    }

    // m3studio делает name.replace(group_name, '')[1:] чтобы получить чистое
    // имя экшна, но для glTF удобнее оставлять полное имя — оно гарантированно
    // уникальное и подходит для отображения в редакторах.
    let _ = group_name;
    let anim_name = stc_name;

    let anim_ids = m3.read_ref_u32(&stc.anim_ids).unwrap_or_default();
    let anim_refs = m3.read_ref_u32(&stc.anim_refs).unwrap_or_default();
    if anim_ids.is_empty() || anim_refs.is_empty() {
        return Ok(None);
    }
    if anim_ids.len() != anim_refs.len() {
        debug!(
            "STC '{}': anim_ids({}) != anim_refs({}); пропускаю",
            anim_name, anim_ids.len(), anim_refs.len()
        );
        return Ok(None);
    }

    let lookup = StcLookup::build(&anim_ids, &anim_refs);
    let sd3v_arr: Vec<Sd3v> = m3.read_sd3v(&stc.sd3v).unwrap_or_default();
    let sd4q_arr: Vec<Sd4q> = m3.read_sd4q(&stc.sd4q).unwrap_or_default();

    let mut samplers: Vec<Sampler> = Vec::new();
    let mut channels: Vec<Channel> = Vec::new();

    for (bi, bone) in bones.iter().enumerate() {
        let target_node = bone_node_base + bi;
        let is_root = bone.parent < 0;

        // Translation. m3studio (key_fcurves в io_m3_import.py) фильтрует
        // только по факту наличия anim_id в STC, не по header.flags — флаги
        // в .m3 часто 0 даже для реально анимируемых костей.
        let loc_id = bone.location.header.id;
        if loc_id != 0 {
            if let Some((kind, idx)) = lookup.lookup(loc_id) {
                if kind == ANIM_TYPE_VEC3 {
                    if let Some(block) = sd3v_arr.get(idx as usize) {
                        if let Some(samp) = build_vec3_sampler(m3, block, bone.location.header.interpolation, is_root, false)? {
                            let s_idx = samplers.len();
                            samplers.push(samp);
                            channels.push(Channel { sampler: s_idx, target_node, path: Path::Translation });
                        }
                    }
                }
            }
        }

        // Rotation
        let rot_id = bone.rotation.header.id;
        if rot_id != 0 {
            if let Some((kind, idx)) = lookup.lookup(rot_id) {
                if kind == ANIM_TYPE_QUAT {
                    if let Some(block) = sd4q_arr.get(idx as usize) {
                        if let Some(samp) = build_quat_sampler(m3, block, bone.rotation.header.interpolation, is_root)? {
                            let s_idx = samplers.len();
                            samplers.push(samp);
                            channels.push(Channel { sampler: s_idx, target_node, path: Path::Rotation });
                        }
                    }
                }
            }
        }

        // Scale (всегда пропускаем поворот корня — scale корня не получает
        // вращательной коррекции, только translation/rotation).
        let scl_id = bone.scale.header.id;
        if scl_id != 0 {
            if let Some((kind, idx)) = lookup.lookup(scl_id) {
                if kind == ANIM_TYPE_VEC3 {
                    if let Some(block) = sd3v_arr.get(idx as usize) {
                        if let Some(samp) = build_vec3_sampler(m3, block, bone.scale.header.interpolation, false, true)? {
                            let s_idx = samplers.len();
                            samplers.push(samp);
                            channels.push(Channel { sampler: s_idx, target_node, path: Path::Scale });
                        }
                    }
                }
            }
        }
    }

    if channels.is_empty() {
        debug!(
            "STC '{}': channels пуст (нет совпадений anim_id костей с anim_ids STC)",
            anim_name
        );
        return Ok(None);
    }

    debug!(
        "anim '{}': {} samplers, {} channels",
        anim_name, samplers.len(), channels.len()
    );

    Ok(Some(Animation { name: anim_name, samplers, channels }))
}

fn build_vec3_sampler(
    m3:           &M3File<'_>,
    block:        &Sd3v,
    interpolation: u16,
    apply_zy:     bool,
    is_scale:     bool,
) -> Result<Option<Sampler>> {
    let frames_ms = m3.read_ref_i32(&block.frames).unwrap_or_default();
    let values    = m3.read_ref_vec3(&block.keys).unwrap_or_default();
    if frames_ms.is_empty() || values.is_empty() { return Ok(None); }
    let n = frames_ms.len().min(values.len());
    let frames_ms = &frames_ms[..n];
    let values = &values[..n];

    let (times_sec, keep) = dedupe_frames(frames_ms);
    let mut data: Vec<[f32; 3]> = Vec::with_capacity(keep.len());
    for &i in &keep {
        let v = values[i];
        let arr = if apply_zy && !is_scale {
            rotate_vec_by_quat([v.x, v.y, v.z], ZY_QUAT)
        } else {
            [v.x, v.y, v.z]
        };
        data.push(arr);
    }

    Ok(Some(Sampler {
        times_sec,
        data: SamplerData::Vec3(data),
        linear: interpolation != 0,
    }))
}

fn build_quat_sampler(
    m3:            &M3File<'_>,
    block:         &Sd4q,
    interpolation: u16,
    apply_zy:      bool,
) -> Result<Option<Sampler>> {
    let frames_ms = m3.read_ref_i32(&block.frames).unwrap_or_default();
    let values    = m3.read_ref_quat(&block.keys).unwrap_or_default();
    if frames_ms.is_empty() || values.is_empty() { return Ok(None); }
    let n = frames_ms.len().min(values.len());
    let frames_ms = &frames_ms[..n];
    let values = &values[..n];

    let (times_sec, keep) = dedupe_frames(frames_ms);
    let mut data: Vec<[f32; 4]> = Vec::with_capacity(keep.len());
    for &i in &keep {
        let q = values[i];
        let arr = if apply_zy {
            quat_mul(ZY_QUAT, [q.x, q.y, q.z, q.w])
        } else {
            [q.x, q.y, q.z, q.w]
        };
        data.push(arr);
    }

    Ok(Some(Sampler {
        times_sec,
        data: SamplerData::Quat(data),
        linear: interpolation != 0,
    }))
}
