//! Mesh data in SoA (Structure of Arrays) form.
//!
//! Each field is a dense single-type array. Both SIMD and rayon work most
//! efficiently against dense arrays.

use smallvec::SmallVec;

/// Primitive info for a single region of a mesh.
/// Lets a single mesh carry different materials per region.
#[derive(Debug, Clone, Default)]
pub struct RegionPrimitiveInfo {
    /// Start of the index range inside the shared `indices` array (u32 elements).
    pub index_start: usize,
    /// Length of the index range.
    pub index_count: usize,
    /// Index into the MATM array (`material_references` in MODL).
    /// glb/mod.rs reads `matm[idx].mat_type` to dispatch to MAT_ or MADD.
    /// `None` — material type is unsupported (mat_type ∉ {1, 12}).
    pub material_index: Option<usize>,
}

/// SoA mesh data. Ready to be written straight into the GLB buffer.
///
/// # Invariants
///
/// All `positions_*`, `normals_*`, `uvs_*` arrays share the same length.
/// `indices` is a flat list of triangles (every 3 elements form one triangle).
#[derive(Default)]
pub struct MeshDataSoA {
    // ── Positions (hot data — touched on every transform) ───────────────────
    pub positions_x: Vec<f32>,
    pub positions_y: Vec<f32>,
    pub positions_z: Vec<f32>,

    // ── Normals ─────────────────────────────────────────────────────────────
    pub normals_x: Vec<f32>,
    pub normals_y: Vec<f32>,
    pub normals_z: Vec<f32>,

    // ── Tangents ────────────────────────────────────────────────────────────
    pub tangents_x: Vec<f32>,
    pub tangents_y: Vec<f32>,
    pub tangents_z: Vec<f32>,
    pub tangents_w: Vec<f32>, // bitangent sign

    // ── UV coordinates ──────────────────────────────────────────────────────
    pub uvs_u: Vec<f32>,
    pub uvs_v: Vec<f32>,

    // ── Skinning ────────────────────────────────────────────────────────────
    /// JOINTS_0 attribute: 4 bone indices per vertex (into the skin joints array).
    /// u16 to support models with > 255 bones.
    pub joints: Vec<[u16; 4]>,
    /// WEIGHTS_0 attribute: 4 weights per vertex as normalised uint8 (0..255).
    /// We mark `normalized = true` on output → the shader divides by 255.
    pub weights: Vec<[u8; 4]>,
    /// True when the mesh is skinned (skin0/skin1 set in vertex_flags). When
    /// false, joints/weights are empty and primitives carry no JOINTS_0/WEIGHTS_0.
    pub has_skin: bool,

    // ── Triangles ───────────────────────────────────────────────────────────
    pub indices: Vec<u32>,

    // ── Metadata ────────────────────────────────────────────────────────────

    /// AABB (bounding box) — computed during conversion.
    pub aabb_min: [f32; 3],
    pub aabb_max: [f32; 3],

    /// Region names (small vec — region count is usually low).
    pub region_names: SmallVec<[String; 4]>,

    /// Per-region primitive info (for multi-material meshes).
    pub region_primitives: Vec<RegionPrimitiveInfo>,

    /// Vertex counter at the start of the next region.
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

    /// Pre-allocate space for the given vertex count.
    /// Call before processing to cut down on reallocations.
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

    /// Vertex count.
    #[inline]
    pub fn vertex_count(&self) -> usize {
        self.positions_x.len()
    }

    /// Triangle count.
    #[inline]
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Base vertex index for the next region.
    /// Used when stitching multiple regions into one mesh.
    #[inline]
    pub(super) fn base_vertex_for_region(&self) -> usize {
        self.vertex_cursor
    }

    /// Call after every vertex of a region has been pushed.
    pub(super) fn commit_region(&mut self) {
        self.vertex_cursor = self.vertex_count();
    }

    /// Apply the Z-up → Y-up rotation (-90° around X) to positions, normals,
    /// tangents and the AABB. Z-up → Y-up: `(x, y, z) → (x, z, -y)`.
    ///
    /// For skinned meshes the same rotation must be applied to the root
    /// bones (see `glb/mod.rs::build_glb_content`); otherwise the rest pose
    /// stays in M3 coordinates.
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
            // tangents_w (bitangent sign) is left alone.
        }
        // AABB: y_new = z_old; z_new = -y_old → min/max along z flip.
        let new_min = [self.aabb_min[0], self.aabb_min[2], -self.aabb_max[1]];
        let new_max = [self.aabb_max[0], self.aabb_max[2], -self.aabb_min[1]];
        self.aabb_min = new_min;
        self.aabb_max = new_max;
    }

    /// Joints (UNSIGNED_SHORT VEC4) → bytes for the GLB buffer.
    pub fn joints_as_bytes(&self) -> &[u8] {
        bytemuck::cast_slice(&self.joints)
    }

    /// Weights (UNSIGNED_BYTE VEC4 normalised) → bytes for the GLB buffer.
    pub fn weights_as_bytes(&self) -> &[u8] {
        bytemuck::cast_slice(&self.weights)
    }

    /// Convert SoA back to AoS for the GLB write phase.
    ///
    /// GLB stores attributes in interleaved form, so we reinterleave at the
    /// final write step. `bytemuck::cast_slice` lets us emit the f32 arrays
    /// directly without per-element work.
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

    /// UVs scaled by `uv_tiling` from the material.
    /// In M3: `uv_final = uv_raw * tiling` (multiply, not divide).
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

    /// Tangents as VEC4 (xyzw) — glTF requires 4 components, w is the bitangent sign.
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
