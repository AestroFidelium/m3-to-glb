//! The node hierarchy: bones (with attachment points), effect nodes, the skin
//! and the mesh nodes, plus the rest-pose matrix math behind the skin.

use super::buffers::Buffers;
use super::json_builder::{Accessor, FLOAT, GltfNode, GltfSkin};
use crate::attach;
use crate::fx::{self, FxItem, curves::FxCurves};
use crate::m3::reader::M3File;
use crate::m3::structures::Bone;
use crate::quat;
use tracing::{debug, warn};

/// Nodes under construction. Bone `i` is node `i`; everything else is
/// appended after the bones.
pub(super) struct Scene {
    pub nodes: Vec<GltfNode>,
    bone_count: usize,
    bone_roots: Vec<usize>,
    mesh_nodes: Vec<usize>,
}

/// A bone's rest translation, rotation and scale. Root bones carry the
/// Z-up → Y-up bake: `T' = R·T`, `Q' = R ⊗ Q`.
fn bone_trs(bone: &Bone, index: usize) -> ([f32; 3], [f32; 4], [f32; 3]) {
    let t = bone.location.default;
    let r = bone.rotation.default;
    let s = bone.scale.default;
    let (t, r) = ([t.x, t.y, t.z], [r.x, r.y, r.z, r.w]);
    let (t, r) = if bone.parent_index(index).is_none() {
        (quat::rotate(t, quat::Z_UP_TO_Y_UP), quat::mul(quat::Z_UP_TO_Y_UP, r))
    } else {
        (t, r)
    };
    (t, r, [s.x, s.y, s.z])
}

impl Scene {
    /// One node per bone, parented by the bone hierarchy. Attachment points
    /// (`Ref_Head`, …) are not nodes of their own — each names a bone that is
    /// already emitted, so it is written into that bone node's `extras`.
    pub fn with_bones(m3: &M3File<'_>, bones: &[Bone]) -> Self {
        let mut attach_extras: ahash::AHashMap<usize, String> = ahash::AHashMap::new();
        let attachments = attach::collect(m3, bones.len());
        for a in &attachments {
            // Two attachments on one bone would fight over one `extras`; the
            // first wins, matching the order the game's own table lists them.
            if attach_extras.contains_key(&a.bone) {
                warn!("bone {} carries more than one attachment point — kept the first", a.bone);
            } else {
                attach_extras.insert(a.bone, a.extras_json());
            }
        }
        if !attachments.is_empty() {
            debug!("attachment points: {}", attachments.len());
        }

        let mut nodes: Vec<GltfNode> = bones
            .iter()
            .enumerate()
            .map(|(bi, bone)| {
                let name = m3.read_char(&bone.name).unwrap_or("");
                let (t, r, s) = bone_trs(bone, bi);
                // M3 rest quaternions are only approximately unit length, and
                // the Z-up bake can push a component past ±1 (1.0000001 on
                // War3_Kelthuzad's root), which glTF rejects.
                let r = quat::normalize_and_clamp(r);
                GltfNode {
                    name: Some(if name.is_empty() { format!("bone_{bi}") } else { name.to_owned() }),
                    translation: Some(t),
                    rotation: Some(r),
                    scale: Some(s),
                    extras: attach_extras.remove(&bi),
                    ..GltfNode::default()
                }
            })
            .collect();
        let mut bone_roots = Vec::new();
        for (bi, bone) in bones.iter().enumerate() {
            match bone.parent_index(bi) {
                Some(parent) => nodes[parent].children.push(bi),
                None => bone_roots.push(bi),
            }
        }
        Self { nodes, bone_count: bones.len(), bone_roots, mesh_nodes: Vec::new() }
    }

    /// One empty node per effect, parented to the bone it rides, carrying its
    /// parameters in `extras`. Nothing references these nodes: they exist so an
    /// engine walking the spawned scene finds the emitter already positioned,
    /// parented and animated.
    pub fn add_effects(
        &mut self,
        items: &[FxItem],
        materials: &ahash::AHashMap<usize, fx::MaterialResolve>,
        curves: &FxCurves,
    ) {
        // `fx::collect` already dropped effects on bones past the end.
        for item in items.iter().filter(|it| it.bone < self.bone_count) {
            let mat = item.matm_index.and_then(|i| materials.get(&i)).copied().unwrap_or_default();
            self.nodes.push(GltfNode {
                name: Some(item.name.clone()),
                translation: item.translation(),
                extras: Some(item.extras_json(&mat, curves)),
                ..GltfNode::default()
            });
            let node = self.nodes.len() - 1;
            self.nodes[item.bone].children.push(node);
        }
    }

    /// One node per mesh, after everything else but the armature. A skinned
    /// mesh node must be a scene root (`NODE_SKINNED_MESH_NON_ROOT`).
    pub fn add_meshes(&mut self, skinned: &[bool], skin: Option<usize>) {
        for (i, &is_skinned) in skinned.iter().enumerate() {
            self.mesh_nodes.push(self.nodes.len());
            self.nodes.push(GltfNode {
                name: Some(if i == 0 { "mesh".into() } else { format!("mesh_{i}") }),
                mesh: Some(i),
                skin: if is_skinned { skin } else { None },
                ..GltfNode::default()
            });
        }
    }

    /// Finish the hierarchy and return the scene roots. All joints of a skin
    /// need a common ancestor (`SKIN_NO_COMMON_ROOT`), so the root bones are
    /// wrapped in a transform-free `armature` node — the standard layout for
    /// skinned models.
    pub fn roots(&mut self) -> Vec<usize> {
        let mut roots = Vec::new();
        if self.bone_count > 0 {
            roots.push(self.nodes.len());
            self.nodes.push(GltfNode {
                name: Some("armature".into()),
                children: std::mem::take(&mut self.bone_roots),
                ..GltfNode::default()
            });
        }
        roots.extend(&self.mesh_nodes);
        roots
    }
}

/// The skin over every bone, with its inverse bind matrices in the buffer.
///
/// m3studio uses the file's `IREF` matrices as Blender bone-roll orientation,
/// not as glTF IBMs (`inverse(world bind matrix)`), so the world bind pose is
/// computed by walking the hierarchy and each matrix inverted.
pub(super) fn skin(bufs: &mut Buffers, bones: &[Bone]) -> GltfSkin {
    let mut ibm_bytes: Vec<u8> = Vec::with_capacity(bones.len() * 64);
    for world in compute_world_matrices(bones) {
        for col in &invert_4x4(&world) {
            ibm_bytes.extend_from_slice(bytemuck::cast_slice(col));
        }
    }
    let buffer_view = bufs.view(&ibm_bytes, None);
    let ibm = bufs.accessor(Accessor {
        buffer_view,
        byte_offset: 0,
        component_type: FLOAT,
        count: bones.len(),
        element_type: "MAT4",
        normalized: false,
        min: None,
        max: None,
    });
    // No `skeleton`: it is optional, and naming one trips
    // SKIN_SKELETON_INVALID when there are multiple bone roots.
    GltfSkin { joints: (0..bones.len()).collect(), inverse_bind_matrices: Some(ibm) }
}

// ─── Bone bind pose → IBM ─────────────────────────────────────────────────────

/// Computes per-bone world bind matrices via forward walk of the bone hierarchy.
/// Each matrix is column-major (`m[col][row]`). Bones must be topologically
/// sorted (parent index < child index), which is the standard M3 layout.
///
/// Root bones get the same Z-up → Y-up rotation as their node TRS
/// (`bone_trs`), so the IBMs match the rotated rest pose.
fn compute_world_matrices(bones: &[Bone]) -> Vec<[[f32; 4]; 4]> {
    let mut world = Vec::with_capacity(bones.len());
    for (i, bone) in bones.iter().enumerate() {
        let (t, q, s) = bone_trs(bone, i);
        let local = trs_to_mat4(t, q, s);
        let m = match bone.parent_index(i) {
            Some(p) => mul_4x4(&world[p], &local),
            None => local,
        };
        world.push(m);
    }
    world
}

#[expect(clippy::many_single_char_names, reason = "the textbook quaternion-to-matrix names")]
fn trs_to_mat4(t: [f32; 3], q: [f32; 4], s: [f32; 3]) -> [[f32; 4]; 4] {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let xx = x * x; let yy = y * y; let zz = z * z;
    let xy = x * y; let xz = x * z; let yz = y * z;
    let wx = w * x; let wy = w * y; let wz = w * z;

    let r00 = 1.0 - 2.0 * (yy + zz);
    let r01 = 2.0 * (xy - wz);
    let r02 = 2.0 * (xz + wy);
    let r10 = 2.0 * (xy + wz);
    let r11 = 1.0 - 2.0 * (xx + zz);
    let r12 = 2.0 * (yz - wx);
    let r20 = 2.0 * (xz - wy);
    let r21 = 2.0 * (yz + wx);
    let r22 = 1.0 - 2.0 * (xx + yy);

    [
        [r00 * s[0], r10 * s[0], r20 * s[0], 0.0],
        [r01 * s[1], r11 * s[1], r21 * s[1], 0.0],
        [r02 * s[2], r12 * s[2], r22 * s[2], 0.0],
        [t[0],       t[1],       t[2],       1.0],
    ]
}

/// `a · b` for column-major matrices (`m[col][row]`).
fn mul_4x4(a: &[[f32; 4]; 4], b: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
    // Column `c` of the product is `a` applied to column `c` of `b`; the dot
    // product sums in k order from +0.0, exactly as the scalar loop did.
    b.map(|b_col| std::array::from_fn(|r| a.iter().zip(b_col).fold(0.0, |s, (a_col, bk)| s + a_col[r] * bk)))
}

/// General 4×4 matrix inversion via cofactor expansion. Operates on column-major
/// arrays. Returns identity if the matrix is singular.
fn invert_4x4(m: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
    // Flatten to row-major for cofactor math, then transpose at the end.
    let a = [
        m[0][0], m[1][0], m[2][0], m[3][0],
        m[0][1], m[1][1], m[2][1], m[3][1],
        m[0][2], m[1][2], m[2][2], m[3][2],
        m[0][3], m[1][3], m[2][3], m[3][3],
    ];
    let mut inv = [0.0f32; 16];
    inv[0]  =  a[5]*a[10]*a[15] - a[5]*a[11]*a[14] - a[9]*a[6]*a[15] + a[9]*a[7]*a[14] + a[13]*a[6]*a[11] - a[13]*a[7]*a[10];
    inv[4]  = -a[4]*a[10]*a[15] + a[4]*a[11]*a[14] + a[8]*a[6]*a[15] - a[8]*a[7]*a[14] - a[12]*a[6]*a[11] + a[12]*a[7]*a[10];
    inv[8]  =  a[4]*a[9]*a[15]  - a[4]*a[11]*a[13] - a[8]*a[5]*a[15] + a[8]*a[7]*a[13] + a[12]*a[5]*a[11] - a[12]*a[7]*a[9];
    inv[12] = -a[4]*a[9]*a[14]  + a[4]*a[10]*a[13] + a[8]*a[5]*a[14] - a[8]*a[6]*a[13] - a[12]*a[5]*a[10] + a[12]*a[6]*a[9];

    inv[1]  = -a[1]*a[10]*a[15] + a[1]*a[11]*a[14] + a[9]*a[2]*a[15] - a[9]*a[3]*a[14] - a[13]*a[2]*a[11] + a[13]*a[3]*a[10];
    inv[5]  =  a[0]*a[10]*a[15] - a[0]*a[11]*a[14] - a[8]*a[2]*a[15] + a[8]*a[3]*a[14] + a[12]*a[2]*a[11] - a[12]*a[3]*a[10];
    inv[9]  = -a[0]*a[9]*a[15]  + a[0]*a[11]*a[13] + a[8]*a[1]*a[15] - a[8]*a[3]*a[13] - a[12]*a[1]*a[11] + a[12]*a[3]*a[9];
    inv[13] =  a[0]*a[9]*a[14]  - a[0]*a[10]*a[13] - a[8]*a[1]*a[14] + a[8]*a[2]*a[13] + a[12]*a[1]*a[10] - a[12]*a[2]*a[9];

    inv[2]  =  a[1]*a[6]*a[15]  - a[1]*a[7]*a[14]  - a[5]*a[2]*a[15] + a[5]*a[3]*a[14] + a[13]*a[2]*a[7]  - a[13]*a[3]*a[6];
    inv[6]  = -a[0]*a[6]*a[15]  + a[0]*a[7]*a[14]  + a[4]*a[2]*a[15] - a[4]*a[3]*a[14] - a[12]*a[2]*a[7]  + a[12]*a[3]*a[6];
    inv[10] =  a[0]*a[5]*a[15]  - a[0]*a[7]*a[13]  - a[4]*a[1]*a[15] + a[4]*a[3]*a[13] + a[12]*a[1]*a[7]  - a[12]*a[3]*a[5];
    inv[14] = -a[0]*a[5]*a[14]  + a[0]*a[6]*a[13]  + a[4]*a[1]*a[14] - a[4]*a[2]*a[13] - a[12]*a[1]*a[6]  + a[12]*a[2]*a[5];

    inv[3]  = -a[1]*a[6]*a[11]  + a[1]*a[7]*a[10]  + a[5]*a[2]*a[11] - a[5]*a[3]*a[10] - a[9]*a[2]*a[7]   + a[9]*a[3]*a[6];
    inv[7]  =  a[0]*a[6]*a[11]  - a[0]*a[7]*a[10]  - a[4]*a[2]*a[11] + a[4]*a[3]*a[10] + a[8]*a[2]*a[7]   - a[8]*a[3]*a[6];
    inv[11] = -a[0]*a[5]*a[11]  + a[0]*a[7]*a[9]   + a[4]*a[1]*a[11] - a[4]*a[3]*a[9]  - a[8]*a[1]*a[7]   + a[8]*a[3]*a[5];
    inv[15] =  a[0]*a[5]*a[10]  - a[0]*a[6]*a[9]   - a[4]*a[1]*a[10] + a[4]*a[2]*a[9]  + a[8]*a[1]*a[6]   - a[8]*a[2]*a[5];

    let det = a[0]*inv[0] + a[1]*inv[4] + a[2]*inv[8] + a[3]*inv[12];
    if det.abs() < 1e-12 {
        return [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
    }
    let inv_det = 1.0 / det;
    for v in &mut inv { *v *= inv_det; }
    // Convert back to column-major: out[col][row] = inv[row*4 + col].
    // Bone TRS produce affine matrices: bottom row is exactly [0,0,0,1].
    // Float precision in the cofactor inversion drifts that, so we snap it
    // — required by the glTF validator (ACCESSOR_INVALID_IBM otherwise).
    [
        [inv[0],  inv[4],  inv[8],  0.0],
        [inv[1],  inv[5],  inv[9],  0.0],
        [inv[2],  inv[6],  inv[10], 0.0],
        [inv[3],  inv[7],  inv[11], 1.0],
    ]
}
