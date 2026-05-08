//! Структура данных меша в формате SoA (Structure of Arrays).
//!
//! Каждое поле — плотный массив одного типа.
//! SIMD и rayon работают максимально эффективно на плотных массивах.

use smallvec::SmallVec;

/// Информация о primitive для отдельного региона меша.
/// Позволяет назначать разные материалы на разные регионы внутри одного меша.
#[derive(Debug, Clone, Default)]
pub struct RegionPrimitiveInfo {
    /// Начало диапазона индексов в общем `indices` массиве (элементы u32)
    pub index_start: usize,
    /// Количество индексов в диапазоне
    pub index_count: usize,
    /// Индекс записи в массиве MATM (`material_references` в MODL).
    /// glb/mod.rs читает `matm[idx].mat_type` чтобы диспатчить на MAT_ или MADD.
    /// `None` — материал не поддерживается (mat_type ∉ {1, 12}).
    pub material_index: Option<usize>,
}

/// Данные меша в SoA формате. Готовы к прямой записи в GLB буфер.
///
/// # Инварианты
///
/// Все массивы `positions_*`, `normals_*`, `uvs_*` имеют одинаковую длину.
/// `indices` — плоский список треугольников (каждые 3 элемента = один треугольник).
#[derive(Default)]
pub struct MeshDataSoA {
    // ── Позиции (горячие данные — используются в каждом вычислении) ─────────
    pub positions_x: Vec<f32>,
    pub positions_y: Vec<f32>,
    pub positions_z: Vec<f32>,

    // ── Нормали ──────────────────────────────────────────────────────────────
    pub normals_x: Vec<f32>,
    pub normals_y: Vec<f32>,
    pub normals_z: Vec<f32>,

    // ── Тангенты ─────────────────────────────────────────────────────────────
    pub tangents_x: Vec<f32>,
    pub tangents_y: Vec<f32>,
    pub tangents_z: Vec<f32>,
    pub tangents_w: Vec<f32>, // знак битангента (bitangent sign)

    // ── UV-координаты ────────────────────────────────────────────────────────
    pub uvs_u: Vec<f32>,
    pub uvs_v: Vec<f32>,

    // ── Скиннинг ─────────────────────────────────────────────────────────────
    /// JOINTS_0 атрибут: 4 индекса костей на вершину (в скин joints array).
    /// Используем u16 чтобы поддержать модели с > 255 костями.
    pub joints: Vec<[u16; 4]>,
    /// WEIGHTS_0 атрибут: 4 веса на вершину как uint8 нормализованные (0..255).
    /// При выводе помечаем normalized=true → шейдер делит на 255.
    pub weights: Vec<[u8; 4]>,
    /// True если меш скиннинговый (есть skin0/skin1 во vertex_flags). Если
    /// false — joints/weights пусты, primitives не содержат JOINTS_0/WEIGHTS_0.
    pub has_skin: bool,

    // ── Треугольники ─────────────────────────────────────────────────────────
    pub indices: Vec<u32>,

    // ── Метаданные ───────────────────────────────────────────────────────────

    /// AABB (bounding box) — вычисляется при конвертации
    pub aabb_min: [f32; 3],
    pub aabb_max: [f32; 3],

    /// Имена регионов (маленький vec — регионов обычно мало)
    pub region_names: SmallVec<[String; 4]>,

    /// Информация о primitives для каждого региона (для multi-material мешей)
    pub region_primitives: Vec<RegionPrimitiveInfo>,

    /// Счётчик вершин до начала следующего региона
    vertex_cursor: usize,
}

impl MeshDataSoA {
    pub fn new() -> Self {
        Self {
            aabb_min: [f32::MAX; 3],
            aabb_max: [f32::MIN; 3],
            ..Default::default()
        }
    }

    /// Предаллоцирует память под заданное количество вершин.
    /// Вызывай перед началом обработки для снижения количества реаллокаций.
    pub fn reserve(&mut self, vertex_count: usize, index_count: usize) {
        self.positions_x.reserve(vertex_count);
        self.positions_y.reserve(vertex_count);
        self.positions_z.reserve(vertex_count);
        self.normals_x.reserve(vertex_count);
        self.normals_y.reserve(vertex_count);
        self.normals_z.reserve(vertex_count);
        self.tangents_x.reserve(vertex_count);
        self.tangents_y.reserve(vertex_count);
        self.tangents_z.reserve(vertex_count);
        self.tangents_w.reserve(vertex_count);
        self.uvs_u.reserve(vertex_count);
        self.uvs_v.reserve(vertex_count);
        self.joints.reserve(vertex_count);
        self.weights.reserve(vertex_count);
        self.indices.reserve(index_count);
    }

    /// Количество вершин в меше.
    #[inline]
    pub fn vertex_count(&self) -> usize {
        self.positions_x.len()
    }

    /// Количество треугольников.
    #[inline]
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Возвращает базовый индекс вершины для следующего региона.
    /// Используется при склейке нескольких регионов в один меш.
    #[inline]
    pub(super) fn base_vertex_for_region(&self) -> usize {
        self.vertex_cursor
    }

    /// Вызывается после добавления всех вершин одного региона.
    pub(super) fn commit_region(&mut self) {
        self.vertex_cursor = self.vertex_count();
    }

    /// Применяет ротацию Z-up → Y-up (поворот -90° вокруг X) к позициям,
    /// нормалям, тангентам и AABB. Z-up → Y-up: (x, y, z) → (x, z, -y).
    ///
    /// Для скиннинговых мешей этот же поворот должен быть применён к корневым
    /// костям (см. `glb/mod.rs::build_glb_content`), иначе rest pose останется
    /// в M3-координатах.
    pub fn apply_zup_to_yup(&mut self) {
        for i in 0..self.vertex_count() {
            let py = self.positions_y[i];
            self.positions_y[i] = self.positions_z[i];
            self.positions_z[i] = -py;

            let ny = self.normals_y[i];
            self.normals_y[i] = self.normals_z[i];
            self.normals_z[i] = -ny;

            let ty = self.tangents_y[i];
            self.tangents_y[i] = self.tangents_z[i];
            self.tangents_z[i] = -ty;
            // tangents_w (bitangent sign) не трогаем
        }
        // AABB: y_new = z_old; z_new = -y_old → min/max по z флипают.
        let new_min = [self.aabb_min[0], self.aabb_min[2], -self.aabb_max[1]];
        let new_max = [self.aabb_max[0], self.aabb_max[2], -self.aabb_min[1]];
        self.aabb_min = new_min;
        self.aabb_max = new_max;
    }

    /// Joints (UNSIGNED_SHORT VEC4) → байты для записи в GLB.
    pub fn joints_as_bytes(&self) -> &[u8] {
        bytemuck::cast_slice(&self.joints)
    }

    /// Weights (UNSIGNED_BYTE VEC4 normalized) → байты для записи в GLB.
    pub fn weights_as_bytes(&self) -> &[u8] {
        bytemuck::cast_slice(&self.weights)
    }

    /// Конвертирует SoA обратно в AoS для записи в GLB.
    ///
    /// GLB хранит данные в interleaved формате, поэтому нам нужно
    /// перемежить данные при финальной записи.
    /// `bytemuck::cast_slice` позволяет записывать f32 массивы напрямую.
    pub fn positions_as_bytes(&self) -> Vec<u8> {
        // interleaved XYZ
        let mut out = Vec::with_capacity(self.vertex_count() * 12);
        for i in 0..self.vertex_count() {
            out.extend_from_slice(bytemuck::bytes_of(&self.positions_x[i]));
            out.extend_from_slice(bytemuck::bytes_of(&self.positions_y[i]));
            out.extend_from_slice(bytemuck::bytes_of(&self.positions_z[i]));
        }
        out
    }

    pub fn normals_as_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.vertex_count() * 12);
        for i in 0..self.vertex_count() {
            out.extend_from_slice(bytemuck::bytes_of(&self.normals_x[i]));
            out.extend_from_slice(bytemuck::bytes_of(&self.normals_y[i]));
            out.extend_from_slice(bytemuck::bytes_of(&self.normals_z[i]));
        }
        out
    }

    pub fn uvs_as_bytes(&self) -> Vec<u8> {
        self.uvs_as_bytes_scaled(1.0, 1.0)
    }

    /// UV с применением масштаба от uv_tiling из материала.
    /// В M3: uv_final = uv_raw * tiling (тайлинг умножается, а не делится).
    pub fn uvs_as_bytes_scaled(&self, tiling_u: f32, tiling_v: f32) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.vertex_count() * 8);
        for i in 0..self.vertex_count() {
            let u = self.uvs_u[i] * tiling_u;
            let v = self.uvs_v[i] * tiling_v;
            out.extend_from_slice(bytemuck::bytes_of(&u));
            out.extend_from_slice(bytemuck::bytes_of(&v));
        }
        out
    }

    pub fn indices_as_bytes(&self) -> &[u8] {
        bytemuck::cast_slice(&self.indices)
    }

    /// Тангенты как VEC4 (xyzw) — glTF требует 4 компонента, w = знак битангента.
    pub fn tangents_as_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.vertex_count() * 16);
        for i in 0..self.vertex_count() {
            out.extend_from_slice(bytemuck::bytes_of(&self.tangents_x[i]));
            out.extend_from_slice(bytemuck::bytes_of(&self.tangents_y[i]));
            out.extend_from_slice(bytemuck::bytes_of(&self.tangents_z[i]));
            out.extend_from_slice(bytemuck::bytes_of(&self.tangents_w[i]));
        }
        out
    }
}
