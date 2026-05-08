//! Геометрический движок: конвертация M3 AoS → SoA + SIMD трансформации.

pub mod anim;
mod soa;
mod transform;

pub use soa::MeshDataSoA;

use crate::m3::reader::M3File;
use anyhow::Result;
use rayon::prelude::*;
use tracing::debug;

/// Конвертирует все меши M3 в SoA формат параллельно через rayon.
pub fn convert_all_meshes(m3: &M3File<'_>) -> Result<Vec<MeshDataSoA>> {
    let divisions = m3.divisions()?;
    if divisions.is_empty() { return Ok(Vec::new()); }

    let vertex_data  = m3.vertex_data()?.to_vec();
    let vertex_flags = m3.vertex_flags();
    let stride       = m3.vertex_stride();
    let offsets      = VertexOffsets::from_flags(vertex_flags);

    debug!("vertex_flags=0x{:08X} stride={} offsets={:?}", vertex_flags, stride, offsets);

    // Читаем material references и bone_lookup один раз
    let mat_refs    = m3.material_references().unwrap_or_default();
    let bone_lookup = m3.bone_lookup().unwrap_or_default();

    // Собираем регионы/индексы/batches последовательно (M3File не Sync),
    // потом параллельная конвертация через rayon.
    type DivPayload = (Vec<crate::m3::structures::Regn>, u32, Vec<u16>, Vec<crate::m3::structures::Bat>);
    let div_data: Vec<DivPayload> = divisions
        .iter()
        .map(|div| {
            let (regions, regn_version) = m3.regions(div)?;
            let indices = m3.face_indices(div)?;
            let batches = m3.batches(div).unwrap_or_default();
            Ok((regions, regn_version, indices, batches))
        })
        .collect::<Result<_>>()?;

    div_data
        .into_par_iter()
        .enumerate()
        .map(|(div_idx, (regions, regn_version, indices, batches))| {
            debug!(
                "Конвертирую Division #{} ({} регионов v{}, {} batches)",
                div_idx, regions.len(), regn_version, batches.len()
            );
            let mut soa = convert_division(
                &vertex_data, stride, &offsets,
                &regions, regn_version, &indices, &batches, &mat_refs,
                &bone_lookup,
            )?;
            // M3 хранит модели в Z-up; glTF — Y-up. Запекаем поворот -90° вокруг X
            // прямо в позиции/нормали/тангенты/AABB. Для скиннинговых мешей в
            // glb/mod.rs тот же поворот применяется к корневым костям.
            soa.apply_zup_to_yup();
            Ok(soa)
        })
        .collect()
}

// ─── Смещения компонентов вершины ─────────────────────────────────────────────

/// Вычисленные смещения компонентов внутри вершины.
/// Основаны на vertex_flags из MODL, согласно m3studio io_m3.py:144 `get_vertex_description`.
#[derive(Debug, Clone)]
pub struct VertexOffsets {
    pub normal:  Option<usize>,
    pub uv0:     Option<usize>,
    pub uv1:     Option<usize>,
    pub tangent: Option<usize>,
    pub skin:    Option<SkinLayout>,
}

#[derive(Debug, Clone, Copy)]
pub struct SkinLayout {
    /// Смещение weights[0..pairs] (uint8, по байту) от начала вершины.
    pub weights_offset: usize,
    /// Смещение lookups[0..pairs] (uint8, по байту) от начала вершины.
    pub lookups_offset: usize,
    /// Количество пар weight/lookup на вершину: 2 (только skin0 или только skin1) или 4 (оба).
    pub pairs:          usize,
}

impl VertexOffsets {
    /// Вычисляет смещения компонентов из vertex_flags по правилам m3studio
    /// (io_m3.py:144 `get_vertex_description`).
    ///
    /// Layout (с типичным flags=0x01860261, stride=40):
    ///   +0:  pos       [f32;3]                    12B  (0x1)
    ///   +12: weights[N] uint8×pairs                NB  (skin0/skin1: pairs=2 или 4)
    ///   +12+N: lookups[N] uint8×pairs              NB
    ///   +20: normalf  [f32;3]                     12B  (0x80)
    ///   +20: normal+sign uint8×4                   4B  (0x800000)
    ///   +24: test100  uint32                       4B  (0x100)
    ///   +24: col      [u8;4]                       4B  (0x200)
    ///   +28: testNNN  uint32                       4B  (0x400/0x800/0x1000)
    ///   ...: fuvN     [f32;2]                      8B каждый (0x2000..0x10000)
    ///   ...: uvN      [i16;2]                      4B каждый (0x20000..0x100000)
    ///   ...: normal_v3 / tangent_v3 [f32;3]       12B каждый (0x200000/0x400000)
    ///   ...: tangent  [u8;4]                       4B  (0x1000000)
    pub fn from_flags(flags: u32) -> Self {
        let mut off: usize = 12; // pos always (0x1)

        // Skin: weights then lookups, each uint8 per slot.
        let pairs = match (flags & 0x20 != 0, flags & 0x40 != 0) {
            (true, true)   => 4,
            (true, false) | (false, true) => 2,
            (false, false) => 0,
        };
        let skin = if pairs > 0 {
            let weights_off = off;
            let lookups_off = off + pairs;
            off += pairs * 2;
            Some(SkinLayout { weights_offset: weights_off, lookups_offset: lookups_off, pairs })
        } else {
            None
        };

        if flags & 0x000080 != 0 { off += 12; }  // normalf (uncompressed normal)

        // Compressed normal (Vector3As3uint8 + sign byte) — RIGHT AFTER skin/normalf,
        // BEFORE color and UVs (m3studio io_m3.py:172-175).
        let normal = if flags & 0x800000 != 0 {
            let o = off; off += 4; Some(o)
        } else { None };

        if flags & 0x000100 != 0 { off += 4; }   // test100 (before col)
        if flags & 0x000200 != 0 { off += 4; }   // col
        if flags & 0x000400 != 0 { off += 4; }   // test400
        if flags & 0x000800 != 0 { off += 4; }   // test800
        if flags & 0x001000 != 0 { off += 4; }   // test1000
        if flags & 0x002000 != 0 { off += 8; }   // fuv0 (Vec2 float)
        if flags & 0x004000 != 0 { off += 8; }   // fuv1
        if flags & 0x008000 != 0 { off += 8; }   // fuv2
        if flags & 0x010000 != 0 { off += 8; }   // fuv3

        // Compressed UVs (Vector2As2int16 — 4 bytes each).
        let uv0 = if flags & 0x020000 != 0 {
            let o = off; off += 4; Some(o)
        } else { None };
        let uv1 = if flags & 0x040000 != 0 {
            let o = off; off += 4; Some(o)
        } else { None };
        if flags & 0x080000 != 0 { off += 4; }   // uv2
        if flags & 0x100000 != 0 { off += 4; }   // uv3

        if flags & 0x200000 != 0 { off += 12; }  // normalf2 (uncompressed normal #2)
        if flags & 0x400000 != 0 { off += 12; }  // tanf (uncompressed tangent)

        // Compressed tangent (Vector3As3uint8 + unused byte).
        let tangent = if flags & 0x1000000 != 0 {
            Some(off)
        } else { None };

        Self { normal, uv0, uv1, tangent, skin }
    }
}

// ─── Конвертация одного Division ─────────────────────────────────────────────

fn convert_division(
    vertex_data:  &[u8],
    stride:       usize,
    offsets:      &VertexOffsets,
    regions:      &[crate::m3::structures::Regn],
    regn_version: u32,
    indices:      &[u16],
    batches:      &[crate::m3::structures::Bat],
    mat_refs:     &[crate::m3::structures::Matm],
    bone_lookup:  &[u16],
) -> Result<MeshDataSoA> {
    let mut soa = MeshDataSoA::new();

    // Привязка material→region. m3studio (io_m3_import.py:1056) собирает ВСЕ
    // batches с batch.region_index == ri и выводит их как material slots на
    // одном меше; в glTF на выходе остаётся ОДИН primitive с первым материалом
    // (остальные batches — метаданные toggling по костям, glTF таких сцен не
    // описывает). Поэтому берём ПЕРВЫЙ batch региона.
    //
    // Здесь хранится ИНДЕКС matm-записи (а не mat_ или madd напрямую). glb/mod.rs
    // потом читает matm[idx].mat_type и диспатчит на MAT_ (тип 1) или MADD (тип
    // 12). Принимаем оба типа — для остальных (DIS_/CMP_/...) оставляем None.
    let mut region_to_mat: Vec<Option<usize>> = vec![None; regions.len()];
    for batch in batches {
        let ridx = batch.region_index as usize;
        if ridx >= region_to_mat.len() || region_to_mat[ridx].is_some() {
            continue;
        }
        let mref_idx = batch.material_reference_index as usize;
        if let Some(mref) = mat_refs.get(mref_idx) {
            if mref.mat_type == 1 || mref.mat_type == 12 {
                region_to_mat[ridx] = Some(mref_idx);
            }
        }
    }

    for (ri, region) in regions.iter().enumerate() {
        let first = region.first_vertex_index as usize;
        let count = region.vertex_count as usize;
        if count == 0 { continue; }

        debug!(
            "  Region[{}]: first_vtx={} count={} stride={} first_face={} num_faces={} mat={:?}",
            ri, first, count, stride,
            region.first_face_index, region.face_count,
            region_to_mat.get(ri).copied().flatten()
        );

        // Позиции (offset=0 всегда)
        transform::extract_positions_to_soa(vertex_data, first, count, stride, &mut soa)?;

        // Скиннинг — JOINTS_0/WEIGHTS_0. Окно bone_lookup для региона:
        //   region_lookup = bone_lookup[first_bone_lookup_index..+bone_lookup_count]
        // Внутри per-vertex lookup байт индексирует это окно; значение даёт
        // глобальный индекс кости (= индекс в skin.joints[]).
        if let Some(layout) = offsets.skin {
            let lk_start = region.first_bone_lookup_idx as usize;
            let lk_count = region.bone_lookup_count as usize;
            let lk_end   = (lk_start + lk_count).min(bone_lookup.len());
            let region_lookup = &bone_lookup[lk_start.min(bone_lookup.len())..lk_end];
            transform::decode_skin(
                vertex_data, first, count, stride, layout, region_lookup, &mut soa,
            )?;
        }

        // Нормали
        if let Some(normal_off) = offsets.normal {
            transform::decode_normals_simd(vertex_data, first, count, stride, normal_off, &mut soa)?;
        } else {
            for _ in 0..count {
                soa.normals_x.push(0.0);
                soa.normals_y.push(1.0);
                soa.normals_z.push(0.0);
            }
        }

        // Тангенты (VEC4: xyz + знак w)
        if let Some(tangent_off) = offsets.tangent {
            transform::decode_tangents(vertex_data, first, count, stride, tangent_off, &mut soa)?;
        } else {
            for _ in 0..count {
                soa.tangents_x.push(1.0);
                soa.tangents_y.push(0.0);
                soa.tangents_z.push(0.0);
                soa.tangents_w.push(1.0);
            }
        }

        // UV — m3studio всегда использует uv0 (если присутствует во vertex_flags).
        // Формула: raw * uv_multiply / 32768 + uv_offset, V flipped.
        {
            let uv_multiply = if region.uv_multiply == 0.0 { 16.0 } else { region.uv_multiply };
            let uv_offset = region.uv_offset;

            if let Some(uv_off) = offsets.uv0 {
                transform::decode_uvs(
                    vertex_data, first, count, stride, uv_off,
                    uv_multiply, uv_offset, &mut soa,
                )?;
            } else {
                for _ in 0..count {
                    soa.uvs_u.push(0.0);
                    soa.uvs_v.push(0.0);
                }
            }
        }

        // Индексы. Для REGN v≤2 индексы абсолютные (от начала vertex buffer),
        // нужно вычесть first_vertex_index чтобы сделать их региональными
        // (см. m3studio io_m3_import.py:1066-1068).
        let fi = region.first_face_index as usize;
        let ni = region.face_count as usize;
        let index_start = soa.indices.len();
        if fi + ni <= indices.len() {
            let base = soa.base_vertex_for_region() as u32;
            let abs_to_local: u32 = if regn_version <= 2 { region.first_vertex_index } else { 0 };
            soa.indices.extend(
                indices[fi..fi + ni]
                    .iter()
                    .map(|&i| (i as u32).saturating_sub(abs_to_local) + base),
            );
        }
        let index_count = soa.indices.len() - index_start;
        soa.commit_region();

        soa.region_primitives.push(crate::processor::soa::RegionPrimitiveInfo {
            index_start,
            index_count,
            material_index: region_to_mat.get(ri).copied().flatten(),
        });
    }

    Ok(soa)
}
