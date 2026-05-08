//! Бинарные структуры формата M3 (StarCraft 2 / Blizzard).
//!
//! Сгенерировано из structures.xml (SC2Mapster/m3addon).
//!
//! Все структуры помечены `#[repr(C)]` + `bytemuck::Pod + Zeroable`.
//! Это позволяет делать zero-copy cast из `&[u8]` → `&[StructType]`
//! через `bytemuck::from_bytes` / `bytemuck::cast_slice`.
//!
//! # Соглашения по именованию
//! - `VN` суффикс означает конкретную версию структуры (например `BoneV1`).
//! - Поля с `since_version` / `till_version` закомментированы если они не входят
//!   в основную (наибольшую поддерживаемую) версию.
//! - Для версионных структур есть псевдонимы типа `pub type Bone = BoneV1;`.

#![allow(dead_code, non_snake_case)]

use bytemuck::{Pod, Zeroable};

// ═══════════════════════════════════════════════════════════════════════════════
//  БАЗОВЫЕ ПРИМИТИВЫ
// ═══════════════════════════════════════════════════════════════════════════════

/// CHAR — один байт символа. Массив всегда завершается нулём.
/// version 0, size 1
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Char {
    pub value: u8,
}

/// FLAG — uint32, используется для хранения нескольких флагов анимации.
/// version 0, size 4
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Flag {
    pub value: u32,
}

/// U8__ — беззнаковый целый 8 бит.
/// version 0, size 1
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct U8 {
    pub value: u8,
}

/// I16_ — знаковый целый 16 бит.
/// version 0, size 2
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct I16 {
    pub value: i16,
}

/// U16_ — беззнаковый целый 16 бит.
/// version 0, size 2
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct U16 {
    pub value: u16,
}

/// I32_ — знаковый целый 32 бит.
/// version 0, size 4
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct I32 {
    pub value: i32,
}

/// U32_ — беззнаковый целый 32 бит.
/// version 0, size 4
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct U32 {
    pub value: u32,
}

/// U64_ — беззнаковый целый 64 бит.
/// version 0, size 8
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct U64 {
    pub value: u64,
}

/// COL — цвет BGRA, по 1 байту на канал.
/// version 0, size 4
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Col {
    pub b: u8,
    pub g: u8,
    pub r: u8,
    pub a: u8,
}

/// REAL — float 32.
/// version 0, size 4
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Real {
    pub value: f32,
}

/// VEC2 — вектор 2 компонента.
/// version 0, size 8
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

/// VEC3 — вектор 3 компонента.
/// version 0, size 12
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// VEC4 — вектор 4 компонента.
/// version 0, size 16
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Vec4 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

/// QUAT — кватернион.
/// version 0, size 16
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Quat {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    /// default 1.0
    pub w: f32,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  ВСПОМОГАТЕЛЬНЫЕ СТРУКТУРЫ
// ═══════════════════════════════════════════════════════════════════════════════

/// Reference — ссылка на один или несколько элементов в файле.
/// version 0, size 12
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Reference {
    /// Количество элементов
    pub entries: u32,
    /// Индекс в таблице тегов
    pub index:   u32,
    /// Флаги (обычно 0 или 1)
    pub flags:   u32,
}

/// SmallReference — ссылка без поля flags (используется в MD33).
/// version 0, size 8
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct SmallReference {
    pub entries: u32,
    pub index:   u32,
}

/// Vector3As3uint8 — вектор из 3 байт; (i / 255.0) → float.
/// version 0, size 3
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Vector3As3uint8 {
    pub x: u8,
    pub y: u8,
    pub z: u8,
}

/// Vector2As2int16 — вектор из int16; (i / 2048.0) → float.
/// version 0, size 4
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Vector2As2int16 {
    pub x: i16,
    pub y: i16,
}

/// Matrix44 — матрица 4×4.
/// version 0, size 64
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Matrix44 {
    pub x: Vec4,
    pub y: Vec4,
    pub z: Vec4,
    pub w: Vec4,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  BNDS — ОГРАНИЧИВАЮЩИЙ ОБЪЁМ
// ═══════════════════════════════════════════════════════════════════════════════

/// BNDS — bounding box + sphere. Center = (min+max)/2.
/// version 0, size 28
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Bnds {
    pub min:    Vec3,
    pub max:    Vec3,
    pub radius: f32,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  EVNT — СОБЫТИЕ АНИМАЦИИ
// ═══════════════════════════════════════════════════════════════════════════════

/// EVNT — событие анимационной последовательности.
/// version 2, size 108
///
/// Layout:
///   +0   name           Reference   12
///   +12  id             i32          4   (default -1)
///   +16  bone           i16          2   (default -1)
///   +18  bone_fb        u16          2
///   +20  matrix         Matrix44    64
///   +84  flags          u32          4   (default 4)
///   +88  payload        Reference   12
///   +100 data_param0    u32          4   (since_version 1)
///   +104 data_param1    u32          4   (since_version 2)
///   = 108
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct EvntV2 {
    pub name:        Reference,
    pub id:          i32,
    pub bone:        i16,
    pub bone_fb:     u16,
    pub matrix:      Matrix44,
    pub flags:       u32,
    pub payload:     Reference,
    pub data_param0: u32,
    pub data_param1: u32,
}

pub type Evnt = EvntV2;

// ═══════════════════════════════════════════════════════════════════════════════
//  ANIMATION REFERENCE HEADER И ТИПЫ
// ═══════════════════════════════════════════════════════════════════════════════

/// AnimationReferenceHeader — заголовок ссылки на анимацию.
/// interpolation: 0=constant, 1=linear
/// flags: 6 = действительная ссылка, 0 = пустая
/// version 0, size 8
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct AnimationReferenceHeader {
    pub interpolation: u16,
    pub flags:         u16,
    pub id:            u32,
}

/// Vector3AnimationReference — анимируемый vec3.
/// version 0, size 36
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Vector3AnimationReference {
    pub header:  AnimationReferenceHeader,
    pub default: Vec3,
    pub null:    Vec3,
    pub unused:  i32,
}

/// Vector2AnimationReference — анимируемый vec2.
/// version 0, size 28
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Vector2AnimationReference {
    pub header:  AnimationReferenceHeader,
    pub default: Vec2,
    pub null:    Vec2,
    pub unused:  i32,
}

/// QuaternionAnimationReference — анимируемый кватернион.
/// version 0, size 44
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct QuaternionAnimationReference {
    pub header:  AnimationReferenceHeader,
    pub default: Quat,
    pub null:    Quat,
    pub unused:  i32,
}

/// UInt32AnimationReference — анимируемый u32.
/// version 0, size 20
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct UInt32AnimationReference {
    pub header:  AnimationReferenceHeader,
    pub default: u32,
    pub null:    u32,
    pub unused:  i32,
}

/// UInt16AnimationReference — анимируемый u16.
/// version 0, size 16
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct UInt16AnimationReference {
    pub header:  AnimationReferenceHeader,
    pub default: u16,
    pub null:    u16,
    pub unused:  i32,
}

/// Int16AnimationReference — анимируемый i16.
/// version 0, size 16
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Int16AnimationReference {
    pub header:  AnimationReferenceHeader,
    pub default: i16,
    pub null:    i16,
    pub unused:  i32,
}

/// FloatAnimationReference — анимируемый f32.
/// version 0, size 20
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct FloatAnimationReference {
    pub header:  AnimationReferenceHeader,
    pub default: f32,
    pub null:    f32,
    pub unused:  i32,
}

/// ColorAnimationReference — анимируемый цвет COL.
/// version 0, size 20
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct ColorAnimationReference {
    pub header:  AnimationReferenceHeader,
    pub default: Col,
    pub null:    Col,
    pub unused:  i32,
}

/// FlagAnimationReference — анимируемый булев флаг (хранится как u32).
/// version 0, size 20
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct FlagAnimationReference {
    pub header:  AnimationReferenceHeader,
    pub default: u32,
    pub null:    u32,
    pub unused:  i32,
}

/// BNDSAnimationReference — анимируемый bounding volume.
/// version 0, size 68
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct BndsAnimationReference {
    pub header:  AnimationReferenceHeader,
    pub default: Bnds,
    pub null:    Bnds,
    pub unused:  i32,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  БАЗОВЫЕ СЕКЦИИ-СПИСКИ
// ═══════════════════════════════════════════════════════════════════════════════

/// SCHR — строка пути к ресурсу.
/// version 0, size 12
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Schr {
    pub path: Reference,
}

/// SR32 — список анимируемых float.
/// version 0, size 20
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Sr32 {
    pub value: FloatAnimationReference,
}

/// SVC3 — список анимируемых 3D-векторов.
/// version 0, size 36
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Svc3 {
    pub location: Vector3AnimationReference,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  АНИМАЦИОННЫЕ БЛОКИ КЛЮЧЕВЫХ КАДРОВ (SD**)
// ═══════════════════════════════════════════════════════════════════════════════

/// Общий layout всех SD** блоков: size 32
/// frames → ref to I32_, flags u32, fend u32, keys → ref to <type>
macro_rules! sd_block {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        /// version 0, size 32
        #[repr(C)]
        #[derive(Debug, Clone, Copy, Pod, Zeroable)]
        pub struct $name {
            pub frames: Reference,
            pub flags:  u32,
            pub fend:   u32,
            pub keys:   Reference,
        }
    };
}

sd_block!(Sdev, "SDEV — ключевые кадры событий (ссылка на EVNT).");
sd_block!(Sd2v, "SD2V — ключевые кадры VEC2.");
sd_block!(Sd3v, "SD3V — ключевые кадры VEC3.");
sd_block!(Sdr3, "SDR3 — ключевые кадры REAL (float).");
sd_block!(Sdcc, "SDCC — ключевые кадры COL.");
sd_block!(Sdu8, "SDU8 — ключевые кадры U8__.");
sd_block!(Sds6, "SDS6 — ключевые кадры I16_.");
sd_block!(Sds3, "SDS3 — ключевые кадры I32_.");
sd_block!(Sdu6, "SDU6 — ключевые кадры U16_.");
sd_block!(Sdu3, "SDU3 — ключевые кадры U32_.");
sd_block!(Sd4q, "SD4Q — ключевые кадры QUAT.");
sd_block!(Sdfg, "SDFG — ключевые кадры FLAG.");
sd_block!(Sdmb, "SDMB — ключевые кадры BNDS.");

// ═══════════════════════════════════════════════════════════════════════════════
//  STC_ — КОЛЛЕКЦИЯ ТРАНСФОРМАЦИЙ ПОСЛЕДОВАТЕЛЬНОСТИ
// ═══════════════════════════════════════════════════════════════════════════════

/// STC_ — Sequence Transformations Collection.
/// version 4, size 204
///
/// Layout:
///   +0    name            Reference  12
///   +12   concurrent      u16         2
///   +14   priority        u16         2
///   +16   sts_index       u16         2
///   +18   sts_index_fb    u16         2
///   +20   anim_ids        Reference  12
///   +32   anim_refs       Reference  12
///   +44   ref_count       u32         4
///   +48   sdev            Reference  12
///   +60   sd2v            Reference  12
///   +72   sd3v            Reference  12
///   +84   sd4q            Reference  12
///   +96   sdcc            Reference  12
///   +108  sdr3            Reference  12
///   +120  sdu8            Reference  12
///   +132  sds6            Reference  12
///   +144  sdu6            Reference  12
///   +156  sds3            Reference  12
///   +168  sdu3            Reference  12
///   +180  sdfg            Reference  12
///   +192  sdmb            Reference  12
///   = 204
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct StcV4 {
    pub name:         Reference,
    pub concurrent:   u16,
    pub priority:     u16,
    pub sts_index:    u16,
    pub sts_index_fb: u16,
    pub anim_ids:     Reference,
    pub anim_refs:    Reference,
    pub ref_count:    u32,
    pub sdev:         Reference,
    pub sd2v:         Reference,
    pub sd3v:         Reference,
    pub sd4q:         Reference,
    pub sdcc:         Reference,
    pub sdr3:         Reference,
    pub sdu8:         Reference,
    pub sds6:         Reference,
    pub sdu6:         Reference,
    pub sds3:         Reference,
    pub sdu3:         Reference,
    pub sdfg:         Reference,
    pub sdmb:         Reference,
}

pub type Stc = StcV4;

// ═══════════════════════════════════════════════════════════════════════════════
//  SEQS — АНИМАЦИОННАЯ ПОСЛЕДОВАТЕЛЬНОСТЬ
// ═══════════════════════════════════════════════════════════════════════════════

/// SEQS — Animation Sequence.
/// version 2, size 92  (без поля unknown05)
///
/// Layout v2 (92 bytes):
///   +0   id              i32    4   (default -1)
///   +4   index           i32    4   (default -1)
///   +8   name            Ref   12
///   +20  anim_ms_start   u32    4
///   +24  anim_ms_end     u32    4
///   +28  movement_speed  f32    4
///   +32  flags           u32    4
///   +36  frequency       u32    4
///   +40  replay_start    u32    4   (default 1)
///   +44  replay_end      u32    4   (default 1)
///   +48  ms_blend        u32    4   (default 100)
///   +52  bounding_sphere Bnds  28
///   +80  anim_sets       Ref   12
///   = 92
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct SeqsV2 {
    pub id:              i32,
    pub index:           i32,
    pub name:            Reference,
    pub anim_ms_start:   u32,
    pub anim_ms_end:     u32,
    pub movement_speed:  f32,
    pub flags:           u32,
    pub frequency:       u32,
    pub replay_start:    u32,
    pub replay_end:      u32,
    pub ms_blend:        u32,
    pub bounding_sphere: Bnds,
    pub anim_sets:       Reference,
}

/// SEQS version 1, size 96 (включает unknown05: u32)
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct SeqsV1 {
    pub id:              i32,
    pub index:           i32,
    pub name:            Reference,
    pub anim_ms_start:   u32,
    pub anim_ms_end:     u32,
    pub movement_speed:  f32,
    pub flags:           u32,
    pub frequency:       u32,
    pub replay_start:    u32,
    pub replay_end:      u32,
    pub ms_blend:        u32,
    pub unknown05:       u32,
    pub bounding_sphere: Bnds,
    pub anim_sets:       Reference,
}

pub type Seqs = SeqsV2;

// ═══════════════════════════════════════════════════════════════════════════════
//  STG_ — ГРУППА ТРАНСФОРМАЦИЙ
// ═══════════════════════════════════════════════════════════════════════════════

/// STG_ — Sequence Transformation Group.
/// version 0, size 24
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Stg {
    pub name:        Reference,
    pub stc_indices: Reference,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  BSET — ANIM SET DATA
// ═══════════════════════════════════════════════════════════════════════════════

/// BSET — SAnimSetData (заменяет SBonesetData).
/// version 0, size 32
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Bset {
    pub flags:        u32,
    pub anim_index:   u16,
    pub anim_index_fb: u16,
    pub name:         Reference,
    pub split_items:  Reference,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  STS_ — SEQUENCE TRANSFORM SET
// ═══════════════════════════════════════════════════════════════════════════════

/// STS_ — Sequence Transform Set.
/// version 0, size 28
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Sts {
    pub anim_ids:    Reference,
    pub parent_index: i32,
    pub next_index:  i32,
    pub child_index: i32,
    pub s1:          i16,
    pub s2:          u16,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  BONE — КОСТЬ СКЕЛЕТА
// ═══════════════════════════════════════════════════════════════════════════════

/// BONE — кость скелета.
/// version 1, size 160
///
/// Layout:
///   +0    id          i32                             4
///   +4    name        Reference                      12
///   +16   flags       u32                             4
///   +20   parent      i16  (default -1)               2
///   +22   s1          u16                             2
///   +24   location    Vector3AnimationReference      36
///   +60   rotation    QuaternionAnimationReference   44
///   +104  scale       Vector3AnimationReference      36
///   +140  batching    FlagAnimationReference         20
///   = 160
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct BoneV1 {
    pub id:       i32,
    pub name:     Reference,
    pub flags:    u32,
    pub parent:   i16,
    pub s1:       u16,
    pub location: Vector3AnimationReference,
    pub rotation: QuaternionAnimationReference,
    pub scale:    Vector3AnimationReference,
    pub batching: FlagAnimationReference,
}

pub type Bone = BoneV1;

// ═══════════════════════════════════════════════════════════════════════════════
//  IREF — ИСХОДНАЯ ПОЗИЦИЯ КОСТИ
// ═══════════════════════════════════════════════════════════════════════════════

/// IREF — baseline position/orientation матрица кости.
/// version 0, size 64
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Iref {
    pub matrix: Matrix44,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  REGN — РЕГИОН МЕША
// ═══════════════════════════════════════════════════════════════════════════════

/// REGN version 5 — полный layout, size 48.
///
///   +0   id                      u32  4
///   +4   unknown01                u32  4  (since v3)
///   +8   first_vertex_index       u32  4  (since v3)
///   +12  vertex_count             u32  4  (since v3)
///   +16  first_face_index         u32  4
///   +20  face_count               u32  4
///   +24  bone_count               u16  2
///   +26  first_bone_lookup_index  u16  2
///   +28  bone_lookup_count        u16  2
///   +30  unknown02                u16  2
///   +32  vertex_lookups_used      u8   1
///   +33  unknown04                u8   1  (default 1)
///   +34  root_bone                u16  2
///   +36  flags                    u32  4  (since v4)
///   +40  uv_multiply              f32  4  (since v5, default 16.0)
///   +44  uv_offset                f32  4  (since v5, default 0.0)
///   = 48
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct RegnV5 {
    pub id:                    u32,
    pub unknown01:             u32,
    pub first_vertex_index:    u32,
    pub vertex_count:          u32,
    pub first_face_index:      u32,
    pub face_count:            u32,
    pub bone_count:            u16,
    pub first_bone_lookup_idx: u16,
    pub bone_lookup_count:     u16,
    pub unknown02:             u16,
    pub vertex_lookups_used:   u8,
    pub unknown04:             u8,
    pub root_bone:             u16,
    pub flags:                 u32,
    pub uv_multiply:           f32,
    pub uv_offset:             f32,
}

/// REGN version 4, size 40 (без uv_multiply / uv_offset).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct RegnV4 {
    pub id:                    u32,
    pub unknown01:             u32,
    pub first_vertex_index:    u32,
    pub vertex_count:          u32,
    pub first_face_index:      u32,
    pub face_count:            u32,
    pub bone_count:            u16,
    pub first_bone_lookup_idx: u16,
    pub bone_lookup_count:     u16,
    pub unknown02:             u16,
    pub vertex_lookups_used:   u8,
    pub unknown04:             u8,
    pub root_bone:             u16,
    pub flags:                 u32,
}

/// REGN version 3, size 36 (без flags).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct RegnV3 {
    pub id:                    u32,
    pub unknown01:             u32,
    pub first_vertex_index:    u32,
    pub vertex_count:          u32,
    pub first_face_index:      u32,
    pub face_count:            u32,
    pub bone_count:            u16,
    pub first_bone_lookup_idx: u16,
    pub bone_lookup_count:     u16,
    pub unknown02:             u16,
    pub vertex_lookups_used:   u8,
    pub unknown04:             u8,
    pub root_bone:             u16,
}

/// REGN version 2, size 28 (first_vertex_index и vertex_count — u16).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct RegnV2 {
    pub id:                    u32,
    pub first_vertex_index:    u16,
    pub vertex_count:          u16,
    pub first_face_index:      u32,
    pub face_count:            u32,
    pub bone_count:            u16,
    pub first_bone_lookup_idx: u16,
    pub bone_lookup_count:     u16,
    pub unknown02:             u16,
    pub vertex_lookups_used:   u8,
    pub unknown04:             u8,
    pub root_bone:             u16,
}

pub type Regn = RegnV5;

// ═══════════════════════════════════════════════════════════════════════════════
//  BAT_ — BATCH
// ═══════════════════════════════════════════════════════════════════════════════

/// BAT_ — связывает регион с материалом.
/// version 1, size 14
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct BatV1 {
    pub flags:                    u16,
    pub priority_plane:           u16,
    pub region_index:             u16,
    pub bounds_index:             u16,
    pub color_index:              u16,
    pub material_reference_index: u16,
    pub bone:                     i16,
}

pub type Bat = BatV1;

// ═══════════════════════════════════════════════════════════════════════════════
//  MSEC — MESH SECTION
// ═══════════════════════════════════════════════════════════════════════════════

/// MSEC — секция меша с bounding.
/// version 1, size 72
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct MsecV1 {
    pub node_index: u32,
    pub bounding:   BndsAnimationReference,
}

pub type Msec = MsecV1;

// ═══════════════════════════════════════════════════════════════════════════════
//  DIV_ — MESH DIVISION
// ═══════════════════════════════════════════════════════════════════════════════

/// DIV_ — Mesh Division.
/// version 2, size 52
///
///   +0   faces     Reference  12
///   +12  regions   Reference  12
///   +24  batches   Reference  12
///   +36  msec      Reference  12
///   +48  instances u32         4   (default 1)
///   = 52
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct DivV2 {
    pub faces:     Reference,
    pub regions:   Reference,
    pub batches:   Reference,
    pub msec:      Reference,
    pub instances: u32,
}

pub type Div = DivV2;

// ═══════════════════════════════════════════════════════════════════════════════
//  ATT_ — ATTACHMENT POINT
// ═══════════════════════════════════════════════════════════════════════════════

/// ATT_ — точка присоединения.
/// version 1, size 20
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct AttV1 {
    pub unknown00: i32,
    pub name:      Reference,
    pub bone:      u32,
}

pub type Att = AttV1;

// ═══════════════════════════════════════════════════════════════════════════════
//  LITE — ИСТОЧНИК СВЕТА
// ═══════════════════════════════════════════════════════════════════════════════

/// LITE — Light. types: 0=directional 1=point 2=spot.
/// version 7, size 212
///
///   +0    shape            u16                      2
///   +2    bone             u16                      2
///   +4    flags            u32                      4
///   +8    lod_cut          u32                      4
///   +12   shadow_lod_cut   i32                      4
///   +16   color            Vector3AnimationRef     36
///   +52   intensity        FloatAnimationRef       20
///   +72   spec_color       Vector3AnimationRef     36
///   +108  spec_intensity   FloatAnimationRef       20
///   +128  attenuation_far  FloatAnimationRef       20
///   +148  unknown148       f32                      4
///   +152  attenuation_near FloatAnimationRef       20
///   +172  hotspot          FloatAnimationRef       20
///   +192  falloff          FloatAnimationRef       20
///   = 212
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct LiteV7 {
    pub shape:           u16,
    pub bone:            u16,
    pub flags:           u32,
    pub lod_cut:         u32,
    pub shadow_lod_cut:  i32,
    pub color:           Vector3AnimationReference,
    pub intensity:       FloatAnimationReference,
    pub spec_color:      Vector3AnimationReference,
    pub spec_intensity:  FloatAnimationReference,
    pub attenuation_far: FloatAnimationReference,
    pub unknown148:      f32,
    pub attenuation_near: FloatAnimationReference,
    pub hotspot:         FloatAnimationReference,
    pub falloff:         FloatAnimationReference,
}

pub type Lite = LiteV7;

// ═══════════════════════════════════════════════════════════════════════════════
//  PATU — PART OF TURRET
// ═══════════════════════════════════════════════════════════════════════════════

/// PATU version 4, size 152
///
///   +0    matrix_forward  Matrix44  64
///   +64   quat_up0        Vec4      16
///   +80   quat_up1        Vec4      16
///   +96   bone            u16        2
///   +98   flags           u8         1
///   +99   group_id        u8         1  (default 1)
///   +100  yaw_flags       u32        4
///   +104  yaw_min         f32        4
///   +108  yaw_max         f32        4
///   +112  yaw_weight      f32        4
///   +116  pitch_flags     u32        4
///   +120  pitch_min       f32        4
///   +124  pitch_max       f32        4
///   +128  pitch_weight    f32        4
///   +132  unknown132      f32        4  (default 1.0)
///   +136  unknown136      f32        4  (default 1.0)
///   +140  main_bone_offset Vec3     12
///   = 152
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct PatuV4 {
    pub matrix_forward:   Matrix44,
    pub quat_up0:         Vec4,
    pub quat_up1:         Vec4,
    pub bone:             u16,
    pub flags:            u8,
    pub group_id:         u8,
    pub yaw_flags:        u32,
    pub yaw_min:          f32,
    pub yaw_max:          f32,
    pub yaw_weight:       f32,
    pub pitch_flags:      u32,
    pub pitch_min:        f32,
    pub pitch_max:        f32,
    pub pitch_weight:     f32,
    pub unknown132:       f32,
    pub unknown136:       f32,
    pub main_bone_offset: Vec3,
}

/// PATU version 1, size 100 (без quat_up0/1, yaw_weight, pitch_weight, main_bone_offset)
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct PatuV1 {
    pub matrix_forward: Matrix44,
    pub bone:           u16,
    pub flags:          u8,
    pub group_id:       u8,
    pub yaw_flags:      u32,
    pub yaw_min:        f32,
    pub yaw_max:        f32,
    pub pitch_flags:    u32,
    pub pitch_min:      f32,
    pub pitch_max:      f32,
    pub unknown132:     f32,
    pub unknown136:     f32,
}

pub type Patu = PatuV4;

// ═══════════════════════════════════════════════════════════════════════════════
//  TRGD — TURRET BEHAVIOR
// ═══════════════════════════════════════════════════════════════════════════════

/// TRGD — Turret Behavior.
/// version 0, size 24
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Trgd {
    pub parts: Reference,
    pub name:  Reference,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  DMMN / DMMT / DMME / MT16 / MT32 — ФИЗИКА МЕША
// ═══════════════════════════════════════════════════════════════════════════════

/// DMMN version 1, size 8 (используется с PHSH v3).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct DmmnV1 {
    pub unknown_00: u8,
    pub unknown_01: u8,
    pub unknown_02: u8,
    pub unknown_03: u8,
    pub unknown_04: u8,
    pub unknown_05: u8,
    pub unknown_06: u8,
    pub unknown_07: u8,
}

/// DMMN version 0, size 12 (используется с PHSH v2).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct DmmnV0 {
    pub dmmt_index: u32,
    pub unknown_00: u8,
    pub unknown_01: u8,
    pub unknown_02: u8,
    pub unknown_03: u8,
    pub unknown_04: u8,
    pub unknown_05: u8,
    pub unknown_06: u8,
    pub unknown_07: u8,
}

/// DMMT — треугольник mesh для PHSH v2.
/// version 0, size 28
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Dmmt {
    pub vertex_index_0: u32,
    pub vertex_index_1: u32,
    pub vertex_index_2: u32,
    pub dmme_index_0:   u32,
    pub dmme_index_1:   u32,
    pub dmme_index_2:   u32,
    pub dmmt_float:     f32,
}

/// DMME — edge структура для PHSH v2.
/// version 0, size 20
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Dmme {
    pub vertex_index_0: u32,
    pub vertex_index_1: u32,
    pub vertex_index_2: u32,
    pub dmmt_index_0:   u32,
    pub dmmt_index_1:   u32,
}

/// MT16 — для PHSH v3.
/// version 0, size 14
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Mt16 {
    pub unknown_4c11db70: u16,
    pub unknown_2e003dc5: u16,
    pub unknown_fa325055: u16,
    pub unknown_902b669d: u16,
    pub unknown_b6238b25: u16,
    pub unknown_44190b0d: u16,
    pub unknown_fbb8bb46: u16,
}

/// MT32 — для PHSH v3.
/// version 0, size 28
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Mt32 {
    pub unknown_4c11db70: u32,
    pub unknown_2e003dc5: u32,
    pub unknown_fa325055: u32,
    pub unknown_902b669d: u32,
    pub unknown_b6238b25: u32,
    pub unknown_44190b0d: u32,
    pub unknown_fbb8bb46: u32,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  LAYR — СЛОЙ МАТЕРИАЛА
// ═══════════════════════════════════════════════════════════════════════════════

/// LAYR version 20–22 (базовый), size 356.
///
/// Layout:
///   +0    id                  u32                   4
///   +4    color_bitmap        Reference            12   ← путь к текстуре
///   +16   color_value         ColorAnimationRef    20
///   +36   flags               u32                   4
///   +40   uv_source           u32                   4
///   +44   color_channels      u32                   4
///   +48   color_multiply      FloatAnimationRef    20
///   +68   color_add           FloatAnimationRef    20
///   +88   noise_type          u32                   4
///   +92   video_channel       i32                   4
///   +96   video_frame_rate    u32                   4
///   +100  video_frame_start   u32                   4
///   +104  video_frame_end     i32                   4
///   +108  video_mode          u32                   4
///   +112  video_sync_timing   u32                   4
///   +116  video_play          UInt32AnimationRef   20
///   +136  video_restart       FlagAnimationRef     20
///   +156  uv_flipbook_rows    u32                   4
///   +160  uv_flipbook_cols    u32                   4
///   +164  uv_flipbook_frame   UInt16AnimationRef   16
///   +180  uv_offset           Vector2AnimationRef  28
///   +208  uv_angle            Vector3AnimationRef  36
///   +244  uv_tiling           Vector2AnimationRef  28
///   +272  uv_w_translation    FloatAnimationRef    20
///   +292  uv_w_scale          FloatAnimationRef    20
///   +312  color_brightness    FloatAnimationRef    20
///   +332  uv_source_related   i32                   4
///   +336  fresnel_type        u32                   4
///   +340  fresnel_exponent    f32                   4
///   +344  fresnel_min         f32                   4
///   +348  fresnel_max_offset  f32                   4
///   +352  uv_density          f32                   4   (till_version 25)
///   = 356
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct LayrV20 {
    pub id:                 u32,
    pub color_bitmap:       Reference,
    pub color_value:        ColorAnimationReference,
    pub flags:              u32,
    pub uv_source:          u32,
    pub color_channels:     u32,
    pub color_multiply:     FloatAnimationReference,
    pub color_add:          FloatAnimationReference,
    pub noise_type:         u32,
    pub video_channel:      i32,
    pub video_frame_rate:   u32,
    pub video_frame_start:  u32,
    pub video_frame_end:    i32,
    pub video_mode:         u32,
    pub video_sync_timing:  u32,
    pub video_play:         UInt32AnimationReference,
    pub video_restart:      FlagAnimationReference,
    pub uv_flipbook_rows:   u32,
    pub uv_flipbook_cols:   u32,
    pub uv_flipbook_frame:  UInt16AnimationReference,
    pub uv_offset:          Vector2AnimationReference,
    pub uv_angle:           Vector3AnimationReference,
    pub uv_tiling:          Vector2AnimationReference,
    pub uv_w_translation:   FloatAnimationReference,
    pub uv_w_scale:         FloatAnimationReference,
    pub color_brightness:   FloatAnimationReference,
    pub uv_source_related:  i32,
    pub fresnel_type:       u32,
    pub fresnel_exponent:   f32,
    pub fresnel_min:        f32,
    pub fresnel_max_offset: f32,
    pub uv_density:         f32,
}

pub type Layr = LayrV20;

// ═══════════════════════════════════════════════════════════════════════════════
//  MATM — ССЫЛКА НА МАТЕРИАЛ
// ═══════════════════════════════════════════════════════════════════════════════

/// MATM — Material Reference.
/// version 0, size 8
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Matm {
    pub mat_type:       u32,
    pub material_index: u32,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  MAT_ — СТАНДАРТНЫЙ МАТЕРИАЛ
// ═══════════════════════════════════════════════════════════════════════════════

/// MAT_ version 20, size 352.
///
/// Layout:
///   +0    name                      Reference  12
///   +12   additional_flags          u32         4
///   +16   flags                     u32         4
///   +20   blend_mode                u32         4
///   +24   priority                  i32         4
///   +28   rtt_channels_used         u32         4
///   +32   specularity               f32         4
///   +36   depth_blend_falloff       f32         4
///   +40   alpha_test_threshold      u32         4
///   +44   hdr_spec                  f32         4
///   +48   hdr_emis                  f32         4
///   +52   hdr_envi_const            f32         4   (since v20)
///   +56   hdr_envi_diff             f32         4   (since v20)
///   +60   hdr_envi_spec             f32         4   (since v20)
///   +64   layer_diff                Reference  12
///   +76   layer_decal               Reference  12
///   +88   layer_spec                Reference  12
///   +100  layer_gloss               Reference  12   (since v16)
///   +112  layer_emis1               Reference  12
///   +124  layer_emis2               Reference  12
///   +136  layer_envi                Reference  12
///   +148  layer_envi_mask           Reference  12
///   +160  layer_alpha1              Reference  12
///   +172  layer_alpha2              Reference  12
///   +184  layer_norm                Reference  12
///   +196  layer_height              Reference  12
///   +208  layer_light               Reference  12
///   +220  layer_ao                  Reference  12
///   +232  layer_norm_blend1_mask    Reference  12   (since v19)
///   +244  layer_norm_blend2_mask    Reference  12   (since v19)
///   +256  layer_norm_blend1         Reference  12   (since v19)
///   +268  layer_norm_blend2         Reference  12   (since v19)
///   +280  material_class            u32         4
///   +284  blend_mode_layer          u32         4
///   +288  blend_mode_emis1          u32         4
///   +292  blend_mode_emis2          u32         4
///   +296  spec_mode                 u32         4
///   +300  parallax_height           FloatAnimRef 20
///   +320  motion_blur               FloatAnimRef 20
///   +340  normal_blend_mask_factor  Reference  12   (since v19)
///   = 352
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct MatV20 {
    pub name:                     Reference,
    pub additional_flags:         u32,
    pub flags:                    u32,
    pub blend_mode:               u32,
    pub priority:                 i32,
    pub rtt_channels_used:        u32,
    pub specularity:              f32,
    pub depth_blend_falloff:      f32,
    pub alpha_test_threshold:     u32,
    pub hdr_spec:                 f32,
    pub hdr_emis:                 f32,
    pub hdr_envi_const:           f32,
    pub hdr_envi_diff:            f32,
    pub hdr_envi_spec:            f32,
    pub layer_diff:               Reference,
    pub layer_decal:              Reference,
    pub layer_spec:               Reference,
    pub layer_gloss:              Reference,
    pub layer_emis1:              Reference,
    pub layer_emis2:              Reference,
    pub layer_envi:               Reference,
    pub layer_envi_mask:          Reference,
    pub layer_alpha1:             Reference,
    pub layer_alpha2:             Reference,
    pub layer_norm:               Reference,
    pub layer_height:             Reference,
    pub layer_light:              Reference,
    pub layer_ao:                 Reference,
    pub layer_norm_blend1_mask:   Reference,
    pub layer_norm_blend2_mask:   Reference,
    pub layer_norm_blend1:        Reference,
    pub layer_norm_blend2:        Reference,
    pub material_class:           u32,
    pub blend_mode_layer:         u32,
    pub blend_mode_emis1:         u32,
    pub blend_mode_emis2:         u32,
    pub spec_mode:                u32,
    pub parallax_height:          FloatAnimationReference,
    pub motion_blur:              FloatAnimationReference,
    pub normal_blend_mask_factor: Reference,
}

pub type Mat = MatV20;

// ═══════════════════════════════════════════════════════════════════════════════
//  DIS_ — DISPLACEMENT MATERIAL
// ═══════════════════════════════════════════════════════════════════════════════

/// DIS_ version 4, size 68
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct DisV4 {
    pub name:            Reference,
    pub unknown00:       u32,
    pub strength_factor: FloatAnimationReference,
    pub layer_norm:      Reference,
    pub layer_strength:  Reference,
    pub flags:           u32,
    pub priority:        i32,
}

pub type Dis = DisV4;

// ═══════════════════════════════════════════════════════════════════════════════
//  CMS_ — COMPOSITE MATERIAL SECTION
// ═══════════════════════════════════════════════════════════════════════════════

/// CMS_ — Composite Material Section.
/// version 0, size 24
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Cms {
    pub material_reference_index: u32,
    pub alpha_factor:             FloatAnimationReference,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  CMP_ — COMPOSITE MATERIAL
// ═══════════════════════════════════════════════════════════════════════════════

/// CMP_ version 2, size 28
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct CmpV2 {
    pub name:     Reference,
    pub priority: i32,
    pub sections: Reference,
}

pub type Cmp = CmpV2;

// ═══════════════════════════════════════════════════════════════════════════════
//  TER_ — TERRAIN MATERIAL
// ═══════════════════════════════════════════════════════════════════════════════

/// TER_ version 1, size 28
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct TerV1 {
    pub name:               Reference,
    pub layer_terrain:      Reference,
    pub unknown_633fd422:   u32,
}

/// TER_ version 0, size 24
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct TerV0 {
    pub name:          Reference,
    pub layer_terrain: Reference,
}

pub type Ter = TerV1;

// ═══════════════════════════════════════════════════════════════════════════════
//  VOL_ — VOLUME MATERIAL
// ═══════════════════════════════════════════════════════════════════════════════

/// VOL_ version 0, size 84
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Vol {
    pub name:           Reference,
    pub unknown00:      u32,
    pub unknown01:      u32,
    pub density:        FloatAnimationReference,
    pub layer_color:    Reference,
    pub layer_unknown1: Reference,
    pub layer_unknown2: Reference,
    pub unknown02:      u32,
    pub unknown03:      u32,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  HAI_ — HAIR MATERIAL (DEFUNCT)
// ═══════════════════════════════════════════════════════════════════════════════

/// HAI_ version 0, size 116
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Hai {
    pub name:            Reference,
    pub layer_base:      Reference,
    pub layer_spec_shift: Reference,
    pub layer_spec_noise: Reference,
    pub layer_ao:        Reference,
    pub shift_primary:   f32,
    pub shift_secondary: f32,
    pub color_diffuse:   ColorAnimationReference,
    pub color_spec:      ColorAnimationReference,
    pub spec_exponent0:  f32,
    pub spec_exponent1:  f32,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  VON_ — VOLUME NOISE MATERIAL
// ═══════════════════════════════════════════════════════════════════════════════

/// VON_ version 0, size 268
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Von {
    pub name:               Reference,
    pub unknown_50762f82:   u32,
    pub flags:              u32,
    pub density:            FloatAnimationReference,
    pub near_plane:         FloatAnimationReference,
    pub falloff:            FloatAnimationReference,
    pub layer_color:        Reference,
    pub layer_noise1:       Reference,
    pub layer_noise2:       Reference,
    pub scroll_rate:        Vector3AnimationReference,
    pub translation:        Vector3AnimationReference,
    pub scale:              Vector3AnimationReference,
    pub rotation:           Vector3AnimationReference,
    pub alpha_threshhold:   u32,
    pub unknown_1d13acfe:   u32,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  CREP — CREEP MATERIAL
// ═══════════════════════════════════════════════════════════════════════════════

/// CREP version 1, size 28
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct CrepV1 {
    pub name:               Reference,
    pub creep_layer:        Reference,
    pub unknown_da1b4eb3:   u32,
}

/// CREP version 0, size 24
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct CrepV0 {
    pub name:        Reference,
    pub creep_layer: Reference,
}

pub type Crep = CrepV1;

// ═══════════════════════════════════════════════════════════════════════════════
//  STBM — SPLAT TERRAIN BAKE MATERIAL
// ═══════════════════════════════════════════════════════════════════════════════

/// STBM version 0, size 48
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Stbm {
    pub name:       Reference,
    pub layer_diff: Reference,
    pub layer_norm: Reference,
    pub layer_spec: Reference,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  LFSB — LENS FLARE SUBSTRUCTURE
// ═══════════════════════════════════════════════════════════════════════════════

/// LFSB version 2, size 56
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct LfsbV2 {
    pub uv_index:          u32,
    pub distance_factor:   f32,
    pub width:             f32,
    pub height:            f32,
    pub width_falloff:     f32,
    pub height_falloff:    f32,
    pub unk00:             u32,
    pub unk01:             u32,
    pub falloff_threshold: f32,
    pub falloff_rate:      f32,
    pub color:             Col,
    pub face_source:       u32,
    pub unk02:             f32,
    pub unk03:             f32,
}

pub type Lfsb = LfsbV2;

// ═══════════════════════════════════════════════════════════════════════════════
//  LFLR — LENS FLARE MATERIAL
// ═══════════════════════════════════════════════════════════════════════════════

/// LFLR version 3, size 152
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct LflrV3 {
    pub name:             Reference,
    pub layer_color:      Reference,
    pub layer_unknown:    Reference,
    pub starbursts:       Reference,
    pub uv_cols:          u32,
    pub uv_rows:          u32,
    pub render_distance:  f32,
    pub unknown_ref_char: Reference,
    pub intensity:        FloatAnimationReference,
    pub color:            ColorAnimationReference,
    pub intensity2:       FloatAnimationReference,
    pub uniform_scale:    FloatAnimationReference,
}

/// LFLR version 2, size 80
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct LflrV2 {
    pub name:            Reference,
    pub layer_color:     Reference,
    pub layer_unknown:   Reference,
    pub starbursts:      Reference,
    pub uv_cols:         u32,
    pub uv_rows:         u32,
    pub render_distance: f32,
    pub intensity:       FloatAnimationReference,
}

pub type Lflr = LflrV3;

// ═══════════════════════════════════════════════════════════════════════════════
//  REF_ — REFLECTION MATERIAL
// ═══════════════════════════════════════════════════════════════════════════════

/// REF_ version 3, size 160
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct RefMatV3 {
    pub name:                  Reference,
    pub unknown_8f5ea924:      u32,
    pub reflection_strength:   FloatAnimationReference,
    pub displacement_strength: FloatAnimationReference,
    pub reflection_offset:     FloatAnimationReference,
    pub blur_angle:            FloatAnimationReference,
    pub blur_distance:         FloatAnimationReference,
    pub layer_norm:            Reference,
    pub layer_strength:        Reference,
    pub layer_blur:            Reference,
    pub flags:                 u32,
    pub unknown_49626d0e:      u32,
}

/// REF_ version 2, size 156
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct RefMatV2 {
    pub name:                  Reference,
    pub unknown_8f5ea924:      u32,
    pub reflection_strength:   FloatAnimationReference,
    pub displacement_strength: FloatAnimationReference,
    pub reflection_offset:     FloatAnimationReference,
    pub blur_angle:            FloatAnimationReference,
    pub blur_distance:         FloatAnimationReference,
    pub layer_norm:            Reference,
    pub layer_strength:        Reference,
    pub layer_blur:            Reference,
    pub flags:                 u32,
}

/// REF_ version 1, size 84
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct RefMatV1 {
    pub name:                  Reference,
    pub unknown_8f5ea924:      u32,
    pub reflection_strength:   FloatAnimationReference,
    pub displacement_strength: FloatAnimationReference,
    pub layer_norm:            Reference,
    pub layer_strength:        Reference,
    pub flags:                 u32,
}

pub type RefMat = RefMatV3;

// ═══════════════════════════════════════════════════════════════════════════════
//  PAR_ — PARTICLE SYSTEM
// ═══════════════════════════════════════════════════════════════════════════════

/// PAR_ version 24, size 1496.
///
/// Это самая большая структура в формате M3.
/// Ниже — полный layout версии 24 (since_version/till_version учтены):
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct ParV24 {
    pub bone:                          u32,
    pub material_reference_index:      u32,
    pub additional_flags:              u32,       // since v17
    pub emit_speed:                    FloatAnimationReference,
    pub emit_speed_random:             FloatAnimationReference,
    pub emit_angle_x:                  FloatAnimationReference,
    pub emit_angle_y:                  FloatAnimationReference,
    pub emit_spread_x:                 FloatAnimationReference,
    pub emit_spread_y:                 FloatAnimationReference,
    pub lifespan:                      FloatAnimationReference,
    pub lifespan_random:               FloatAnimationReference,
    pub distance_limit:                f32,
    pub gravity_x:                     u32,
    pub gravity_y:                     u32,
    pub gravity:                       f32,
    pub size_anim_mid:                 f32,       // since v12
    pub color_anim_mid:                f32,       // since v12
    pub alpha_anim_mid:                f32,       // since v12
    pub rotation_anim_mid:             f32,       // since v12
    pub size_hold:                     f32,       // since v14
    pub color_hold:                    f32,       // since v14
    pub alpha_hold:                    f32,       // since v14
    pub rotation_hold:                 f32,       // since v14
    pub size:                          Vector3AnimationReference,
    pub rotation:                      Vector3AnimationReference,
    pub color_init:                    ColorAnimationReference,
    pub color_mid:                     ColorAnimationReference,
    pub color_end:                     ColorAnimationReference,
    pub drag:                          f32,
    pub mass:                          f32,
    pub mass2:                         f32,
    pub mass_size_factor:              f32,       // since v12
    pub local_forces:                  u16,
    pub world_forces:                  u16,
    pub local_forces_fb:               u16,
    pub world_forces_fb:               u16,
    pub world_forces_mass_mult:        f32,       // since v24
    pub noise_amplitude:               f32,
    pub noise_frequency:               f32,
    pub noise_cohesion:                f32,
    pub noise_edge:                    f32,
    pub index_plus_length:             u32,       // since v11
    pub emit_max:                      u32,
    pub emit_rate:                     FloatAnimationReference,
    pub emit_shape:                    u32,
    pub emit_shape_size:               Vector3AnimationReference,
    pub emit_shape_size_cutout:        Vector3AnimationReference,
    pub emit_shape_radius:             FloatAnimationReference,
    pub emit_shape_radius_cutout:      FloatAnimationReference,
    pub emit_shape_regions:            Reference,  // since v14
    pub emit_type:                     u32,
    pub size_randomize:                u32,
    pub size2:                         Vector3AnimationReference,
    pub rotation_randomize:            u32,
    pub rotation2:                     Vector3AnimationReference,
    pub color_randomize:               u32,
    pub color2_init:                   ColorAnimationReference,
    pub color2_mid:                    ColorAnimationReference,
    pub color2_end:                    ColorAnimationReference,
    pub alpha_randomize:               u32,
    pub emit_count:                    Int16AnimationReference,
    pub uv_flipbook_start_init_index:  u8,
    pub uv_flipbook_start_stop_index:  u8,
    pub uv_flipbook_end_init_index:    u8,
    pub uv_flipbook_end_stop_index:    u8,
    pub uv_flipbook_start_lifespan_factor: f32,
    pub uv_flipbook_cols:              u16,
    pub uv_flipbook_rows:              u16,
    pub uv_flipbook_col_fraction:      f32,       // since v12
    pub uv_flipbook_row_fraction:      f32,       // since v12
    pub bounce:                        f32,
    pub friction:                      f32,
    pub collide_system:                i32,
    pub collide_emit_min:              u32,
    pub collide_emit_max:              u32,
    pub collide_emit_chance:           f32,
    pub collide_emit_energy:           f32,
    pub collide_events_cull:           u32,
    pub particle_type:                 u32,
    pub instance_tail:                 f32,
    pub instance_direction:            Vec3,
    pub instance_distance:             f32,       // since v17
    pub pitch_var_shape:               u32,
    pub pitch_var_amplitude:           FloatAnimationReference,
    pub pitch_var_frequency:           FloatAnimationReference,
    pub yaw_var_shape:                 u32,
    pub yaw_var_amplitude:             FloatAnimationReference,
    pub yaw_var_frequency:             FloatAnimationReference,
    pub speed_var_shape:               u32,
    pub speed_var_amplitude:           FloatAnimationReference,
    pub speed_var_frequency:           FloatAnimationReference,
    pub size_var_shape:                u32,
    pub size_var_amplitude:            FloatAnimationReference,
    pub size_var_frequency:            FloatAnimationReference,
    pub alpha_var_shape:               u32,
    pub alpha_var_amplitude:           FloatAnimationReference,
    pub alpha_var_frequency:           FloatAnimationReference,
    pub color_var_shape:               u32,
    pub color_var_amplitude:           FloatAnimationReference,
    pub color_var_frequency:           FloatAnimationReference,
    pub rotation_var_shape:            u32,
    pub rotation_var_amplitude:        FloatAnimationReference,
    pub rotation_var_frequency:        FloatAnimationReference,
    pub spread_x_var_shape:            u32,
    pub spread_x_var_amplitude:        FloatAnimationReference,
    pub spread_x_var_frequency:        FloatAnimationReference,
    pub spread_y_var_shape:            u32,
    pub spread_y_var_amplitude:        FloatAnimationReference,
    pub spread_y_var_frequency:        FloatAnimationReference,
    pub parent_velocity:               FloatAnimationReference,
    pub phase_shift:                   FloatAnimationReference,  // since v22
    pub flags:                         u32,
    pub rotation_flags:                u32,    // since v18
    pub color_smoothing:               u32,    // since v14
    pub size_smoothing:                u32,    // since v14
    pub rotation_smoothing:            u32,    // since v14
    pub uv_ss_threshold:               FloatAnimationReference,   // since v17
    pub uv_ss_offset:                  Vector2AnimationReference, // since v17
    pub uv_ss_angle:                   Vector3AnimationReference, // since v17
    pub uv_ss_tiling:                  Vector2AnimationReference, // since v17
    pub emit_shape_spline:             Reference,
    pub wind_multiplier:               f32,
    pub lod_reduce:                    u32,
    pub lod_cut:                       u32,
    pub spline_bounds_min:             FloatAnimationReference,
    pub spline_bounds_max:             FloatAnimationReference,
    pub trail_system:                  i32,
    pub trail_chance:                  f32,
    pub trail_rate:                    FloatAnimationReference,
    pub collide_splat:                 i32,
    pub collide_splat_chance:          f32,
    pub model_paths:                   Reference,
    pub copy_indices:                  Reference,
    pub unknown_9a7afdf2:              u32,     // since v23
    pub unknown_87d57a7a:              i32,     // since v23
}

pub type Par = ParV24;

// ═══════════════════════════════════════════════════════════════════════════════
//  PARC — PARTICLE SYSTEM COPY
// ═══════════════════════════════════════════════════════════════════════════════

/// PARC version 0, size 40
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Parc {
    pub emit_rate:  FloatAnimationReference,
    pub emit_count: Int16AnimationReference,
    pub bone:       u32,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  PROJ — PROJECTION
// ═══════════════════════════════════════════════════════════════════════════════

/// PROJ version 5, size 388
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct ProjV5 {
    pub projection_type:      u32,
    pub bone:                 u32,
    pub material_reference_index: u32,
    pub offset:               Vector3AnimationReference,
    pub pitch:                FloatAnimationReference,
    pub yaw:                  FloatAnimationReference,
    pub roll:                 FloatAnimationReference,
    pub field_of_view:        FloatAnimationReference,
    pub aspect_ratio:         FloatAnimationReference,
    pub near:                 FloatAnimationReference,
    pub far:                  FloatAnimationReference,
    pub box_offset_z_bottom:  FloatAnimationReference,
    pub box_offset_z_top:     FloatAnimationReference,
    pub box_offset_x_left:    FloatAnimationReference,
    pub box_offset_x_right:   FloatAnimationReference,
    pub box_offset_y_front:   FloatAnimationReference,
    pub box_offset_y_back:    FloatAnimationReference,
    pub falloff:              f32,
    pub alpha_init:           f32,
    pub alpha_mid:            f32,
    pub alpha_end:            f32,
    pub lifetime_attack:      f32,
    pub lifetime_attack_to:   f32,
    pub lifetime_hold:        f32,
    pub lifetime_hold_to:     f32,
    pub lifetime_decay:       f32,
    pub lifetime_decay_to:    f32,
    pub attenuation_distance: f32,
    pub active:               UInt32AnimationReference,
    pub layer:                u32,
    pub lod_reduce:           u32,
    pub lod_cut:              u32,
    pub flags:                u32,
}

pub type Proj = ProjV5;

// ═══════════════════════════════════════════════════════════════════════════════
//  PHYJ — PHYSICAL JOINTS
// ═══════════════════════════════════════════════════════════════════════════════

/// PHYJ version 0, size 180
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Phyj {
    pub joint_type:            u32,
    pub bone1:                 u32,
    pub bone2:                 u32,
    pub matrix1:               Matrix44,
    pub matrix2:               Matrix44,
    pub limit_bool:            u32,
    pub limit_min:             f32,
    pub limit_max:             f32,
    pub limit_angle:           f32,
    pub friction_bool:         u32,
    pub friction:              f32,
    pub damping_ratio:         f32,
    pub angular_frequency:     f32,
    pub break_threshold:       f32,
    pub shape_collision_value: i32,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  PHCC — CLOTH CONSTRAINT
// ═══════════════════════════════════════════════════════════════════════════════

/// PHCC version 0, size 76
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Phcc {
    pub matrix:  Matrix44,
    pub radius:  f32,
    pub height:  f32,
    pub bone:    i16,
    pub bone_fb: u16,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  PHAC — CLOTH INFLUENCE MAP
// ═══════════════════════════════════════════════════════════════════════════════

/// PHAC version 0, size 32
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Phac {
    pub influenced_region_index:  u32,
    pub simulation_region_index:  u32,
    pub simulation_vert_lookups:  Reference,
    pub simulation_vert_weights:  Reference,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  PHCL — CLOTH BEHAVIOR
// ═══════════════════════════════════════════════════════════════════════════════

/// PHCL version 4, size 192
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct PhclV4 {
    pub simulation_region_index: u32,
    pub unknown00:               u32,
    pub skin_bones:              Reference,
    pub vertex_simulated:        Reference,
    pub vertex_bones:            Reference,
    pub vertex_weights:          Reference,
    pub constraints:             Reference,
    pub influence_map:           Reference,
    pub density:                 f32,
    pub tracking:                f32,
    pub stiffness_stretching:    f32,
    pub stiffness_horizontal:    f32,
    pub stiffness_blending:      f32,
    pub damping:                 f32,
    pub friction:                f32,
    pub gravity:                 f32,
    pub explosion_scale:         f32,
    pub wind_scale:              f32,
    pub stiffness_shear:         f32,
    pub drag_factor:             f32,
    pub lift_factor:             f32,
    pub stiffness_spheres:       f32,
    pub unknown01:               u32,
    pub unknown02:               u32,
    pub unknown03:               u32,
    pub unknown04:               u32,
    pub unknown05:               u32,
    pub unknown06:               i32,
    pub skin_collision:          u32,
    pub skin_offset:             f32,
    pub skin_exponent:           f32,
    pub skin_stiffness:          f32,
    pub unknown7:                u32,
    pub local_wind:              Vec3,
}

/// PHCL version 2, size 136 (без полей since_version 4)
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct PhclV2 {
    pub simulation_region_index: u32,
    pub unknown00:               u32,
    pub skin_bones:              Reference,
    pub vertex_simulated:        Reference,
    pub vertex_bones:            Reference,
    pub vertex_weights:          Reference,
    pub constraints:             Reference,
    pub influence_map:           Reference,
    pub density:                 f32,
    pub tracking:                f32,
    pub stiffness_stretching:    f32,
    pub stiffness_horizontal:    f32,
    pub stiffness_blending:      f32,
    pub damping:                 f32,
    pub friction:                f32,
    pub gravity:                 f32,
    pub explosion_scale:         f32,
    pub wind_scale:              f32,
    pub stiffness_shear:         f32,
    pub drag_factor:             f32,
    pub lift_factor:             f32,
    pub stiffness_spheres:       f32,
}

pub type Phcl = PhclV4;

// ═══════════════════════════════════════════════════════════════════════════════
//  FOR_ — FORCE
// ═══════════════════════════════════════════════════════════════════════════════

/// FOR_ version 2, size 104
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct ForV2 {
    pub force_type:   u32,
    pub shape:        u32,
    pub unknown_for0: u32,
    pub bone:         u32,
    pub flags:        u32,
    pub channels:     u32,
    pub strength:     FloatAnimationReference,
    pub width:        FloatAnimationReference,
    pub height:       FloatAnimationReference,
    pub length:       FloatAnimationReference,
}

pub type For = ForV2;

// ═══════════════════════════════════════════════════════════════════════════════
//  DMSE — MESH LOOP STRUCTURE
// ═══════════════════════════════════════════════════════════════════════════════

/// DMSE version 0, size 4
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Dmse {
    pub unknown00: u8,
    pub vertex:    u8,
    pub polygon:   u8,
    pub loop_:     u8,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  PHSH — PHYSICS SHAPE
// ═══════════════════════════════════════════════════════════════════════════════

/// PHSH version 3, size 300.
///
/// Shape types: 0=box, 1=sphere, 2=capsule, 3=cylinder, 4=convex hull, 5=mesh.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct PhshV3 {
    pub matrix:             Matrix44,
    pub shape:              u8,
    pub shape_fb8:          u8,
    pub shape_fb16:         u16,
    pub unknown_66ce545e:   [u8; 20],
    pub unknown_66ce545e_b: u32,
    pub size0:              f32,
    pub size1:              f32,
    pub size2:              f32,
    pub vertices:           Reference,
    pub plane_equations:    Reference,
    pub loops:              Reference,
    pub polygons:           Reference,
    pub polygons_center:    Vec3,
    pub vertices_count:     u32,
    pub polygons_count:     u32,
    pub loops_count:        u32,
    pub unknown_8d41b3f8:   f32,
    pub unknown_56fece2:    f32,
    pub unknown_90e72e22:   Reference,  // → DMMN (v3)
    pub unknown_9836ec85:   Reference,  // → VEC4 (v3)
    pub unknown_ac0ac492:   Reference,  // → MT16 (v3)
    pub unknown_90e72e22b:  Reference,  // → MT32 (v3)
    pub unknown_6f23bce8:   f32,
    pub unknown_6f23bce9:   f32,
    pub unknown_6f23bce0:   f32,
    pub unknown_9836ec85b:  f32,
    pub unknown_9836ec86:   f32,
    pub unknown_9836ec87:   f32,
    pub unknown_ac0ac492b:  f32,
    pub unknown_ac0ac493:   f32,
    pub unknown_ac0ac494:   f32,
    pub dmmn_count:         u32,
    pub vec4_count:         u32,
    pub unknown00_count:    u32,
    pub unknown01_count:    u32,
    pub mt16_count:         u32,
    pub mt32_count:         u32,
    pub unknown02_count:    u32,
    pub unknown_9d115fa7:   f32,
    pub unknown_721b9a99:   f32,
    pub unknown03_count:    u32,
    pub unknown04_count:    u32,
    pub unknown_9d115fb0:   u16,
    pub unknown_9d115fb1:   u16,
    pub unknown_9d115fb2:   u32,
    pub unknown_9d115fc0:   u16,
    pub unknown_9d115fc1:   u16,
    pub unknown_9d115fc2:   u32,
    pub unknown_9d115fd0:   u16,
    pub unknown_9d115fd1:   u16,
    pub unknown_9d115fd2:   u32,
}

pub type Phsh = PhshV3;

// ═══════════════════════════════════════════════════════════════════════════════
//  PHRB — RIGID BODY
// ═══════════════════════════════════════════════════════════════════════════════

/// PHRB version 4, size 80
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct PhrbV4 {
    pub simulation_type:     u16,
    pub bone:                u16,
    pub physical_material:   u32,
    pub mass:                f32,
    pub friction:            f32,
    pub bounce:              f32,
    pub damping_linear:      f32,
    pub damping_angular:     f32,
    pub gravity_factor:      f32,
    pub unknown_8de065e8:    UInt32AnimationReference,
    pub unknown_57796a4d:    f32,
    pub physics_shape:       Reference,
    pub flags:               u32,
    pub local_forces:        u16,
    pub world_forces:        u16,
    pub priority:            u32,
}

/// PHRB version 3, size 56
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct PhrbV3 {
    pub simulation_type:   u16,
    pub bone:              u16,
    pub physical_material: u32,
    pub mass:              f32,
    pub friction:          f32,
    pub bounce:            f32,
    pub damping_linear:    f32,
    pub damping_angular:   f32,
    pub gravity_factor:    f32,
    pub physics_shape:     Reference,
    pub flags:             u32,
    pub local_forces:      u16,
    pub world_forces:      u16,
    pub priority:          u32,
}

pub type Phrb = PhrbV4;

// ═══════════════════════════════════════════════════════════════════════════════
//  SSGS — SUPER SIMPLE GEOMETRIC SHAPE
// ═══════════════════════════════════════════════════════════════════════════════

/// SSGS version 1, size 108
///
/// shape: 0=Cuboid, 1=Sphere, 2=Cylinder
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct SsgsV1 {
    pub shape:     u32,
    pub bone:      i16,
    pub bone_fb:   u16,
    pub matrix:    Matrix44,
    pub vertices:  Reference,
    pub face_data: Reference,
    pub size0:     f32,
    pub size1:     f32,
    pub size2:     f32,
}

pub type Ssgs = SsgsV1;

// ═══════════════════════════════════════════════════════════════════════════════
//  ATVL — ATTACHMENT VOLUME
// ═══════════════════════════════════════════════════════════════════════════════

/// ATVL version 0, size 116
///
/// shape: 0=Cuboid, 1=Sphere, 2=Cylinder
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Atvl {
    pub bone0:     u32,
    pub bone1:     u32,
    pub shape:     u32,
    pub bone2:     u32,
    pub matrix:    Matrix44,
    pub vertices:  Reference,
    pub face_data: Reference,
    pub size0:     f32,
    pub size1:     f32,
    pub size2:     f32,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  BBSC — BILLBOARD BEHAVIOR
// ═══════════════════════════════════════════════════════════════════════════════

/// BBSC version 0, size 48
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Bbsc {
    pub dependents:      Reference,
    pub bone:            u16,
    pub billboard_type:  u8,
    pub camera_look_at:  u8,
    pub up:              Quat,
    pub forward:         Quat,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  TMD_ — TRAILING MODEL (DEFUNCT)
// ═══════════════════════════════════════════════════════════════════════════════

/// TMD_ version 1, size 72
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct TmdV1 {
    pub vectors:           Reference,
    pub unknown_d3f6c7b8:  f32,
    pub unknown_74229b33:  f32,
    pub unknown_d4f91286:  FloatAnimationReference,
    pub unknown_77f047c2:  FloatAnimationReference,
    pub unknown_bc1e64c1:  u32,
    pub unknown_6cd3476c:  u32,
    pub unknown_ccd5a5af:  u32,
}

pub type Tmd = TmdV1;

// ═══════════════════════════════════════════════════════════════════════════════
//  SRIB — SPLINE RIBBON END POINT
// ═══════════════════════════════════════════════════════════════════════════════

/// SRIB version 0, size 272
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Srib {
    pub emission_offset:       Vec3,
    pub emission_vector:       Vec3,
    pub velocity:              FloatAnimationReference,
    pub unknown_eee1a711:      u32,
    pub bone:                  u32,
    pub velocity_base_fac:     FloatAnimationReference,
    pub velocity_end_fac:      FloatAnimationReference,
    pub yaw_var_shape:         u32,
    pub yaw_var_amplitude:     FloatAnimationReference,
    pub yaw_var_frequency:     FloatAnimationReference,
    pub pitch_var_shape:       u32,
    pub pitch_var_amplitude:   FloatAnimationReference,
    pub pitch_var_frequency:   FloatAnimationReference,
    pub velocity_var_shape:    u32,
    pub velocity_var_amplitude: FloatAnimationReference,
    pub velocity_var_frequency: FloatAnimationReference,
    pub yaw:                   FloatAnimationReference,
    pub pitch:                 FloatAnimationReference,
    pub unknown02:             f32,
    pub unknown03:             f32,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  RIB_ — RIBBON EFFECT
// ═══════════════════════════════════════════════════════════════════════════════

/// RIB_ version 9, size 760.
///
/// ribbon_type: 0=PlanarBillboarded, 1=Planar, 2=Cylinder, 3=Star Shaped
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct RibV9 {
    pub bone:                    u16,
    pub bone_fb:                 u16,
    pub material_reference_index: u32,
    pub additional_flags:        u32,        // since v8
    pub speed:                   FloatAnimationReference,
    pub speed2:                  FloatAnimationReference,
    pub yaw:                     FloatAnimationReference,   // since v8
    pub pitch:                   FloatAnimationReference,   // since v8
    pub spread_x:                FloatAnimationReference,
    pub spread_y:                FloatAnimationReference,
    pub lifespan:                FloatAnimationReference,
    pub lifespan2:               FloatAnimationReference,
    pub distance_limit:          u32,
    pub gravity_x:               f32,
    pub gravity_y:               f32,
    pub gravity:                 f32,
    pub scale_anim_mid:          f32,
    pub color_anim_mid:          f32,
    pub alpha_anim_mid:          f32,
    pub twist_anim_mid:          f32,
    pub scale_anim_mid_time:     f32,        // since v8
    pub color_anim_mid_time:     f32,        // since v8
    pub alpha_anim_mid_time:     f32,        // since v8
    pub twist_anim_mid_time:     f32,        // since v8
    pub scale:                   Vector3AnimationReference,
    pub twist:                   Vector3AnimationReference,
    pub color_base:              ColorAnimationReference,
    pub color_mid:               ColorAnimationReference,
    pub color_tip:               ColorAnimationReference,
    pub drag:                    f32,
    pub mass:                    f32,
    pub mass2:                   f32,
    pub mass_size_factor:        f32,
    pub local_forces:            u16,
    pub world_forces:            u16,
    pub local_forces_fb:         u16,
    pub world_forces_fb:         u16,
    pub world_forces_mass_mult:  f32,        // since v9
    pub noise_amplitude:         f32,
    pub noise_frequency:         f32,
    pub noise_cohesion:          f32,
    pub noise_edge:              f32,
    pub index_plus_length:       u32,        // since v5
    pub ribbon_type:             u32,
    pub cull_method:             u32,
    pub divisions:               f32,
    pub sides:                   u32,
    pub star_ratio:              f32,
    pub length:                  FloatAnimationReference,
    pub spline:                  Reference,
    pub active:                  UInt32AnimationReference,
    pub flags:                   u32,
    pub scale_smoothing:         u32,        // since v8
    pub color_smoothing:         u32,        // since v8
    pub friction:                f32,
    pub bounce:                  f32,
    pub lod_reduce:              u32,
    pub lod_cut:                 u32,
    pub yaw_var_shape:           u32,        // since v8
    pub yaw_var_amplitude:       FloatAnimationReference,   // since v8
    pub yaw_var_frequency:       FloatAnimationReference,   // since v8
    pub pitch_var_shape:         u32,        // since v8
    pub pitch_var_amplitude:     FloatAnimationReference,   // since v8
    pub pitch_var_frequency:     FloatAnimationReference,   // since v8
    pub length_var_shape:        u32,
    pub length_var_amplitude:    FloatAnimationReference,
    pub length_var_frequency:    FloatAnimationReference,
    pub scale_var_shape:         u32,
    pub scale_var_amplitude:     FloatAnimationReference,
    pub scale_var_frequency:     FloatAnimationReference,
    pub alpha_var_shape:         u32,
    pub alpha_var_amplitude:     FloatAnimationReference,
    pub alpha_var_frequency:     FloatAnimationReference,
    pub parent_velocity:         FloatAnimationReference,
    pub phase_shift:             FloatAnimationReference,
}

pub type Rib = RibV9;

// ═══════════════════════════════════════════════════════════════════════════════
//  IK СТРУКТУРЫ
// ═══════════════════════════════════════════════════════════════════════════════

/// IK2J — Two Joints IK Solver.
/// version 0, size 48
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Ik2j {
    pub dependents:           Reference,
    pub bone_base:            u16,
    pub bone_target:          u16,
    pub bone_end:             u16,
    pub bone_fb:              u16,
    pub hinge_axis:           Vec3,
    pub cos_min_hinge_angle:  f32,
    pub cos_max_hinge_angle:  f32,
    pub search_up:            f32,
    pub search_down:          f32,
}

/// IKCC — CCD IK Solver.
/// version 0, size 24
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Ikcc {
    pub dependents:  Reference,
    pub bone_base:   u16,
    pub bone_target: u16,
    pub search_up:   f32,
    pub search_down: f32,
}

/// IKJT — IK Joint Behavior.
/// version 0, size 32
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Ikjt {
    pub dependents:     Reference,
    pub bone_base:      u16,
    pub bone_target:    u16,
    pub search_up:      f32,
    pub search_down:    f32,
    pub search_speed:   f32,
    pub goal_threshold: f32,
}

/// PAOB — One Bone Solver.
/// version 0, size 24
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Paob {
    pub dependents: Reference,
    pub bone:       u16,
    pub bone_fb:    u16,
    pub flags:      u32,
    pub angle:      f32,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  SHBX — SHADOW BOX
// ═══════════════════════════════════════════════════════════════════════════════

/// SHBX version 0, size 64
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Shbx {
    pub bone:      u16,
    pub unknown00: u16,
    pub length:    FloatAnimationReference,
    pub width:     FloatAnimationReference,
    pub height:    FloatAnimationReference,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  CAM_ — CAMERA
// ═══════════════════════════════════════════════════════════════════════════════

/// CAM_ version 5, size 264
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct CamV5 {
    pub bone:                  u32,
    pub name:                  Reference,
    pub field_of_view:         FloatAnimationReference,
    pub use_vertical_fov:      u32,
    pub depth_of_field_type:   u32,
    pub far_clip:              FloatAnimationReference,
    pub near_clip:             FloatAnimationReference,
    pub clip2:                 FloatAnimationReference,
    pub focal_depth:           FloatAnimationReference,
    pub falloff_start:         FloatAnimationReference,
    pub falloff_end:           FloatAnimationReference,
    pub unknown_587dc7fb:      FloatAnimationReference,
    pub unknown_fff8cb33:      FloatAnimationReference,
    pub depth_of_field:        FloatAnimationReference,
    pub unknown_f726f834:      FloatAnimationReference,
    pub unknown_d506807d:      FloatAnimationReference,
}

/// CAM_ version 3, size 180
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct CamV3 {
    pub bone:             u32,
    pub name:             Reference,
    pub field_of_view:    FloatAnimationReference,
    pub use_vertical_fov: u32,
    pub far_clip:         FloatAnimationReference,
    pub near_clip:        FloatAnimationReference,
    pub clip2:            FloatAnimationReference,
    pub focal_depth:      FloatAnimationReference,
    pub falloff_start:    FloatAnimationReference,
    pub falloff_end:      FloatAnimationReference,
    pub depth_of_field:   FloatAnimationReference,
}

pub type Cam = CamV5;

// ═══════════════════════════════════════════════════════════════════════════════
//  WRP_ — WARP
// ═══════════════════════════════════════════════════════════════════════════════

/// WRP_ version 1, size 132
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct WrpV1 {
    pub unknown_861f507b:  u32,
    pub bone:              u32,
    pub unknown_cf03b856:  u32,
    pub radius:            FloatAnimationReference,
    pub unknown_9306aac0:  FloatAnimationReference,
    pub strength:          FloatAnimationReference,
    pub unknown_50c7f2b4:  FloatAnimationReference,
    pub unknown_8d9c977c:  FloatAnimationReference,
    pub unknown_ca6025a2:  FloatAnimationReference,
}

pub type Wrp = WrpV1;

// ═══════════════════════════════════════════════════════════════════════════════
//  VVOL — VIEW VOLUME
// ═══════════════════════════════════════════════════════════════════════════════

/// VVOL version 0, size 40
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Vvol {
    pub node_index: u32,
    pub size:       Vector3AnimationReference,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  MADD — MATERIAL BUFFER (NODE BASED)
// ═══════════════════════════════════════════════════════════════════════════════

/// MADD version 3, size 160
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct MaddV3 {
    pub name:          Reference,
    pub unknown2:      Reference,
    pub unknown3:      Reference,
    pub unknown4:      Reference,
    pub texture_paths: Reference,
    pub unknown_00:    u32,
    pub unknown_01:    u32,
    pub unknown_02:    u32,
    pub unknown_03:    u32,
    pub unknown_04:    u32,
    pub unknown_05:    u32,
    pub unknown_06:    u32,
    pub unknown_07:    u32,
    pub unknown_08:    u32,
    pub unknown_09:    u32,
    pub unknown_10:    u32,
    pub unknown_11:    u32,
    pub unknown_12:    f32,
    pub unknown_13:    f32,
    pub unknown_14:    u32,
    pub unknown_150:   u16,
    pub unknown_151:   u16,
    pub unknown_16:    u32,
    pub unknown_17:    u32,
    pub unknown_18:    u32,
    pub unknown_190:   u16,
    pub unknown_191:   u16,
    pub unknown_20:    u32,
    pub unknown_21:    u32,
    pub unknown_22:    u16,
    pub unknown_23:    u16,
    pub unknown_b9c70ff0: i32,
    pub unknown_b9c70ff1: i32,
}

pub type Madd = MaddV3;

// ═══════════════════════════════════════════════════════════════════════════════
//  MODL — КОРНЕВАЯ МОДЕЛЬ
// ═══════════════════════════════════════════════════════════════════════════════

/// MODL version 30, size 868 — самая полная версия.
///
/// Это корневая структура M3 файла. Содержит ссылки на все подсекции.
/// Размер и состав полей постепенно росли с версии 20 до 30.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct ModlV30 {
    pub model_name:                     Reference,
    pub flags:                          u32,
    pub sequences:                      Reference,   // → SEQS
    pub sequence_transformation_collections: Reference, // → STC_
    pub sequence_transformation_groups: Reference,  // → STG_
    pub bone_anim_sets:                 Reference,  // → BSET
    pub split_count:                    u32,
    pub sts:                            Reference,  // → STS_
    pub bones:                          Reference,  // → BONE
    pub skin_bone_count:                u32,
    pub vertex_flags:                   u32,
    pub vertices:                       Reference,  // → U8__
    pub divisions:                      Reference,  // → DIV_
    pub bone_lookup:                    Reference,  // → U16_
    pub boundings:                      Bnds,
    pub collision_boundings:            Bnds,
    pub collision_faces:                Reference,  // → U16_
    pub collision_verts:                Reference,  // → VEC3
    pub collision_face_normals:         Reference,  // → VEC3
    pub attachment_points:              Reference,  // → ATT_
    pub attachment_points_addon:        Reference,  // → U16_
    pub lights:                         Reference,  // → LITE
    pub shadow_boxes:                   Reference,  // → SHBX (since v21)
    pub cameras:                        Reference,  // → CAM_
    pub cameras_addon:                  Reference,  // → U16_
    pub material_references:            Reference,  // → MATM
    pub materials_standard:             Reference,  // → MAT_
    pub materials_displacement:         Reference,  // → DIS_
    pub materials_composite:            Reference,  // → CMP_
    pub materials_terrain:              Reference,  // → TER_
    pub materials_volume:               Reference,  // → VOL_
    pub materials_hair:                 Reference,  // → HAI_ (defunct)
    pub materials_creep:                Reference,  // → CREP
    pub materials_volumenoise:          Reference,  // → VON_ (since v25)
    pub materials_splatterrainbake:     Reference,  // → STBM (since v26)
    pub materials_reflection:           Reference,  // → REF_ (since v28)
    pub materials_lensflare:            Reference,  // → LFLR (since v29)
    pub materials_buffer:               Reference,  // → MADD (since v30)
    pub particle_systems:               Reference,  // → PAR_
    pub particle_copies:                Reference,  // → PARC
    pub ribbons:                        Reference,  // → RIB_
    pub projections:                    Reference,  // → PROJ
    pub forces:                         Reference,  // → FOR_
    pub warps:                          Reference,  // → WRP_
    pub view_volumes:                   Reference,  // → VVOL
    pub physics_rigidbodies:            Reference,  // → PHRB
    pub physics_constraints:            Reference,  // → PHCT
    pub physics_joints:                 Reference,  // → PHYJ
    pub physics_cloths:                 Reference,  // → PHCL (since v28)
    pub ik_two_joints:                  Reference,  // → IK2J
    pub ik_ccd:                         Reference,  // → IKCC (since v24)
    pub ik_joints:                      Reference,  // → IKJT
    pub one_bone_solvers:               Reference,  // → PAOB
    pub turret_parts:                   Reference,  // → PATU
    pub turrets:                        Reference,  // → TRGD
    pub bone_rests:                     Reference,  // → IREF
    pub hittest_tight:                  SsgsV1,
    pub hittests:                       Reference,  // → SSGS
    pub attachment_volumes:             Reference,  // → ATVL
    pub attachment_volumes_addon0:      Reference,  // → U16_ (since v23)
    pub attachment_volumes_addon1:      Reference,  // → U16_ (since v23)
    pub billboards:                     Reference,  // → BBSC
    pub tmd_data:                       Reference,  // → TMD_
    pub m3a_hash:                       u32,
    pub m3a_hashes:                     Reference,  // → U32_
}

pub type Modl = ModlV30;

// ═══════════════════════════════════════════════════════════════════════════════
//  ЗАГОЛОВКИ ФАЙЛА
// ═══════════════════════════════════════════════════════════════════════════════

/// MD34 — заголовок M3 файла.
/// Всегда находится в начале файла.
/// version 11, size 24
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Md34 {
    /// Magic: b"MD34"
    pub tag:          u32,
    pub index_offset: u32,
    pub index_size:   u32,
    pub model:        Reference,
}

/// MD33 — заголовок M3 файла (SC2 Beta версия, использует SmallReference).
/// version 11, size 20
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Md33 {
    /// Magic: b"MD33"
    pub tag:          u32,
    pub index_offset: u32,
    pub index_size:   u32,
    pub model:        SmallReference,
}

/// MDIndexEntry — запись в таблице тегов файла.
/// Существует в версиях 33 (MD33) и 34 (MD34).
/// version 33/34, size 16
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct MdIndexEntry {
    /// Тег (4 ASCII символа, например b"REGN", b"BONE")
    pub tag:         u32,
    /// Смещение данных от начала файла (в байтах)
    pub offset:      u32,
    /// Количество элементов
    pub repetitions: u32,
    /// Версия секции
    pub version:     u32,
}

impl MdIndexEntry {
    /// Возвращает тег как срез байт.
    #[inline]
    pub fn tag_bytes(&self) -> [u8; 4] {
        self.tag.to_le_bytes()
    }

    /// Возвращает диапазон байт в файле для данной секции.
    #[inline]
    pub fn byte_range(&self, elem_size: usize) -> std::ops::Range<usize> {
        let start = self.offset as usize;
        let end   = start + self.repetitions as usize * elem_size;
        start..end
    }
}
