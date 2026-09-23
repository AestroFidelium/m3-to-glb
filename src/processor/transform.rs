//! SIMD-accelerated vertex transforms.
//!
//! # Strategy
//!
//! 1. `multiversion` generates AVX2 + SSE4.1 + scalar variants at compile time.
//! 2. Runtime dispatch picks the best variant automatically.
//! 3. `wide::f32x8` processes 8 vertices per SIMD iteration.
//!
//! # M3 encoding
//!
//! - Normals: `[u8; 4]` → f32: `x / 255 * 2 - 1`, then renormalised
//! - UV: `[i16; 2]` → f32: `x * region.uv_multiply / 32768 + region.uv_offset`

use super::soa::MeshDataSoA;
use anyhow::{Result, ensure};
use multiversion::multiversion;
use wide::f32x8;

/// A run of vertices inside the interleaved M3 vertex buffer: `count`
/// vertices of `stride` bytes each, starting at vertex `first`.
#[derive(Debug, Clone, Copy)]
pub struct VertexSpan<'a> {
    pub data:   &'a [u8],
    pub first:  usize,
    pub count:  usize,
    pub stride: usize,
}

/// Whether every vertex of `span` lies inside its buffer. Each decoder checks
/// this once up front (together with its component fitting in the stride), so
/// the per-vertex loops index without further checks.
fn span_fits(span: VertexSpan<'_>) -> bool {
    span.first
        .checked_add(span.count)
        .and_then(|n| n.checked_mul(span.stride))
        .is_some_and(|end| end <= span.data.len())
}

/// Normal-decoding constant: 1.0 / 255.0 (uint8 → float, M3 format).
const SNORM8_SCALE: f32 = 1.0 / 255.0;

// ─── Position extraction ─────────────────────────────────────────────────────

/// Extract vertex positions from the AoS buffer into the SoA buffers.
/// SIMD via multiversion (AVX2 / SSE4.1 / scalar fallback).
pub fn extract_positions_to_soa(span: VertexSpan<'_>, soa: &mut MeshDataSoA) -> Result<()> {
    let VertexSpan { data: vertex_data, first: first_vertex, count: vertex_count, stride: vertex_stride } = span;
    ensure!(
        vertex_stride >= 12,
        "vertex_stride {} too small for position (need ≥ 12)",
        vertex_stride
    );

    ensure!(
        span_fits(span),
        "vertex buffer out of bounds: vertices {}..{} of {} bytes each, buffer has {}",
        first_vertex, first_vertex + vertex_count, vertex_stride, vertex_data.len()
    );

    soa.reserve(vertex_count, 0);

    // Pull X, Y, Z into separate scratch buffers for SIMD processing.
    let mut xs: Vec<f32> = Vec::with_capacity(vertex_count);
    let mut ys: Vec<f32> = Vec::with_capacity(vertex_count);
    let mut zs: Vec<f32> = Vec::with_capacity(vertex_count);

    for i in 0..vertex_count {
        let base = (first_vertex + i) * vertex_stride;
        // In bounds: `span_fits` was verified above.
        let [x, y, z]: [f32; 3] = bytemuck::pod_read_unaligned(&vertex_data[base..base + 12]);
        // glTF positions must be finite (and the accessor min/max are JSON).
        let finite = |v: f32| if v.is_finite() { v } else { 0.0 };
        let (x, y, z) = (finite(x), finite(y), finite(z));
        xs.push(x);
        ys.push(y);
        zs.push(z);
    }

    // SIMD: update AABB across the dense arrays (8 floats per iteration).
    let (aabb_min, aabb_max) = compute_aabb_simd(&xs, &ys, &zs);

    // Merge into the mesh AABB.
    soa.aabb_min[0] = soa.aabb_min[0].min(aabb_min[0]);
    soa.aabb_min[1] = soa.aabb_min[1].min(aabb_min[1]);
    soa.aabb_min[2] = soa.aabb_min[2].min(aabb_min[2]);
    soa.aabb_max[0] = soa.aabb_max[0].max(aabb_max[0]);
    soa.aabb_max[1] = soa.aabb_max[1].max(aabb_max[1]);
    soa.aabb_max[2] = soa.aabb_max[2].max(aabb_max[2]);

    soa.positions_x.extend_from_slice(&xs);
    soa.positions_y.extend_from_slice(&ys);
    soa.positions_z.extend_from_slice(&zs);

    Ok(())
}

/// SIMD AABB computation across SoA arrays.
/// Processes 8 floats per iteration with `wide::f32x8`.
#[multiversion(targets("x86_64+avx2", "x86_64+sse4.1", "aarch64+neon"))]
fn compute_aabb_simd(xs: &[f32], ys: &[f32], zs: &[f32]) -> ([f32; 3], [f32; 3]) {
    let inf = f32x8::splat(f32::MAX);
    let neg_inf = f32x8::splat(f32::MIN);

    let mut min_x = inf;
    let mut min_y = inf;
    let mut min_z = inf;
    let mut max_x = neg_inf;
    let mut max_y = neg_inf;
    let mut max_z = neg_inf;

    let chunks_x = xs.chunks_exact(8);
    let chunks_y = ys.chunks_exact(8);
    let chunks_z = zs.chunks_exact(8);
    let tail_x = chunks_x.remainder();
    let tail_y = chunks_y.remainder();
    let tail_z = chunks_z.remainder();

    for ((cx, cy), cz) in chunks_x.zip(chunks_y).zip(chunks_z) {
        let vx = f32x8::from(cx.try_into().unwrap_or([0.0f32; 8]));
        let vy = f32x8::from(cy.try_into().unwrap_or([0.0f32; 8]));
        let vz = f32x8::from(cz.try_into().unwrap_or([0.0f32; 8]));

        min_x = min_x.fast_min(vx);
        min_y = min_y.fast_min(vy);
        min_z = min_z.fast_min(vz);
        max_x = max_x.fast_max(vx);
        max_y = max_y.fast_max(vy);
        max_z = max_z.fast_max(vz);
    }

    // Horizontal reduction: SIMD vector → scalar.
    let min_xa: [f32; 8] = min_x.into();
    let min_ya: [f32; 8] = min_y.into();
    let min_za: [f32; 8] = min_z.into();
    let max_xa: [f32; 8] = max_x.into();
    let max_ya: [f32; 8] = max_y.into();
    let max_za: [f32; 8] = max_z.into();

    let mut aabb_min = [
        min_xa.iter().cloned().fold(f32::MAX, f32::min),
        min_ya.iter().cloned().fold(f32::MAX, f32::min),
        min_za.iter().cloned().fold(f32::MAX, f32::min),
    ];
    let mut aabb_max = [
        max_xa.iter().cloned().fold(f32::MIN, f32::max),
        max_ya.iter().cloned().fold(f32::MIN, f32::max),
        max_za.iter().cloned().fold(f32::MIN, f32::max),
    ];

    // Tail (< 8 vertices).
    for i in 0..tail_x.len() {
        aabb_min[0] = aabb_min[0].min(tail_x[i]);
        aabb_min[1] = aabb_min[1].min(tail_y[i]);
        aabb_min[2] = aabb_min[2].min(tail_z[i]);
        aabb_max[0] = aabb_max[0].max(tail_x[i]);
        aabb_max[1] = aabb_max[1].max(tail_y[i]);
        aabb_max[2] = aabb_max[2].max(tail_z[i]);
    }

    (aabb_min, aabb_max)
}

// ─── Normal decoding ─────────────────────────────────────────────────────────

/// Decode normals from uint8 → f32 (Vector3As3uint8 per structures.xml).
/// Formula: `n / 255.0` — output range [0..1].
/// For a true unit vector we also do `n * 2.0 - 1.0`; glTF nominally accepts
/// the [0..1] range too but we normalise downstream.
///
/// SIMD: 8 normals per iteration via `wide::f32x8`.
pub fn decode_normals_simd(
    span: VertexSpan<'_>,
    component_offset: usize, // normal offset inside the vertex (from vertex_flags)
    soa: &mut MeshDataSoA,
) -> Result<()> {
    let VertexSpan { data: vertex_data, first: first_vertex, count: vertex_count, stride: vertex_stride } = span;
    ensure!(
        vertex_stride >= component_offset + 4,
        "vertex_stride {} too small for normals (offset={})",
        vertex_stride,
        component_offset
    );

    let mut raw_nx: Vec<f32> = Vec::with_capacity(vertex_count);
    let mut raw_ny: Vec<f32> = Vec::with_capacity(vertex_count);
    let mut raw_nz: Vec<f32> = Vec::with_capacity(vertex_count);

    ensure!(span_fits(span), "normal data out of bounds");
    for i in 0..vertex_count {
        let base = (first_vertex + i) * vertex_stride + component_offset;

        // uint8 → f32; we normalise to [-1..1] via `*2-1` below.
        let nx = vertex_data[base] as f32;
        let ny = vertex_data[base + 1] as f32;
        let nz = vertex_data[base + 2] as f32;

        raw_nx.push(nx);
        raw_ny.push(ny);
        raw_nz.push(nz);
    }

    // SIMD: multiply by 1/255, then `*2-1` to map into [-1..1].
    let scale = SNORM8_SCALE;
    let mut nx_f = scale_array_simd(&raw_nx, scale);
    let mut ny_f = scale_array_simd(&raw_ny, scale);
    let mut nz_f = scale_array_simd(&raw_nz, scale);
    // Recentre: [0..1] → [-1..1].
    for v in nx_f.iter_mut() {
        *v = *v * 2.0 - 1.0;
    }
    for v in ny_f.iter_mut() {
        *v = *v * 2.0 - 1.0;
    }
    for v in nz_f.iter_mut() {
        *v = *v * 2.0 - 1.0;
    }

    // Normalise to unit length (glTF requires unit vectors).
    for i in 0..vertex_count {
        let x = nx_f[i];
        let y = ny_f[i];
        let z = nz_f[i];
        // Never zero: `b / 255 * 2 - 1` is at least 1/255 away from 0 for
        // every byte `b`, so the shortest decodable vector has length ~0.007.
        let inv_len = 1.0 / (x * x + y * y + z * z).sqrt();
        nx_f[i] = x * inv_len;
        ny_f[i] = y * inv_len;
        nz_f[i] = z * inv_len;
    }

    soa.normals_x.extend_from_slice(&nx_f);
    soa.normals_y.extend_from_slice(&ny_f);
    soa.normals_z.extend_from_slice(&nz_f);

    Ok(())
}

// ─── Skinning decoding ───────────────────────────────────────────────────────

use super::SkinLayout;

/// Pull skin data (joints + weights) out of the vertex buffer.
///
/// The layout follows m3studio (io_m3.py:144 `get_vertex_description`):
/// first `pairs` weight bytes, then `pairs` lookup bytes. The lookup byte
/// indexes a window
/// `bone_lookup_full[region.first_bone_lookup_index..+region.bone_lookup_count]`,
/// and the value at that window position is the global bone index
/// (= index in `skin.joints[]`).
///
/// glTF JOINTS_0 / WEIGHTS_0 are always VEC4 — for 2-pair models (skin0
/// only or skin1 only) slots 2..3 are filled with index 0 and weight 0.
pub fn decode_skin(
    span:          VertexSpan<'_>,
    layout:        SkinLayout,
    region_lookup: &[u16],
    soa:           &mut MeshDataSoA,
) -> Result<()> {
    let VertexSpan { data: vertex_data, first: first_vertex, count: vertex_count, stride: vertex_stride } = span;
    let SkinLayout { weights_offset, lookups_offset, pairs } = layout;
    ensure!(
        pairs <= 4,
        "skin pairs ({}) > 4 unsupported by glTF VEC4",
        pairs
    );
    ensure!(
        vertex_stride >= weights_offset + pairs && vertex_stride >= lookups_offset + pairs,
        "vertex_stride {} too small for skin (w_off={} l_off={} pairs={})",
        vertex_stride, weights_offset, lookups_offset, pairs
    );

    soa.has_skin = true;
    soa.joints.reserve(vertex_count);
    soa.weights.reserve(vertex_count);

    ensure!(span_fits(span), "skin data out of bounds");
    for i in 0..vertex_count {
        let base = (first_vertex + i) * vertex_stride;
        let w_off = base + weights_offset;
        let l_off = base + lookups_offset;

        let mut joints  = [0u16; 4];
        let mut weights = [0u8; 4];
        for j in 0..pairs {
            let weight = vertex_data[w_off + j];
            if weight == 0 {
                // glTF: when weight is zero the joint must also be zero
                // (otherwise the validator emits ACCESSOR_JOINTS_USED_ZERO_WEIGHT
                // and engines may behave unpredictably).
                continue;
            }
            let lookup = vertex_data[l_off + j] as usize;
            let bone_idx = region_lookup.get(lookup).copied().unwrap_or(0);
            joints[j]  = bone_idx;
            weights[j] = weight;
        }

        soa.joints.push(joints);
        soa.weights.push(weights);
    }

    Ok(())
}

// ─── Tangent decoding ────────────────────────────────────────────────────────

/// Decode tangents from uint8x4 SNORM → f32 VEC4.
///
/// The first 3 bytes of the tangent block encode `tangent.xyz` the same way
/// as normals: `x = b/255*2-1`. The **4th byte of the tangent block is
/// unused** (see m3studio `io_m3.py` — `M3FieldInt('unused', 'uint8')`).
///
/// glTF requires `tangent.w` = bitangent handedness. That value lives in
/// the **4th byte of the NORMAL block** (`m3studio io_m3_export.py:1802-1804`):
/// the exporter writes `m3_vert.sign = 0 if d < 0 else 255`, where `d` is
/// the signed UV area of the face. Map back to glTF:
///   sign byte == 0  →  tangent.w = -1.0  (UV mirrored, flip bitangent)
///   sign byte != 0  →  tangent.w = +1.0  (standard winding)
///
/// `normal_offset` is the offset of the normal block inside the vertex; it
/// is `None` when the mesh has no compressed normal (in which case we fall
/// back to +1, since there is no face winding hint to consult).
pub fn decode_tangents(
    span: VertexSpan<'_>,
    component_offset: usize,
    normal_offset: Option<usize>,
    soa: &mut MeshDataSoA,
) -> Result<()> {
    let VertexSpan { data: vertex_data, first: first_vertex, count: vertex_count, stride: vertex_stride } = span;
    ensure!(
        vertex_stride >= component_offset + 4,
        "vertex_stride {} too small for tangent (offset={})",
        vertex_stride,
        component_offset
    );
    if let Some(n_off) = normal_offset {
        ensure!(
            vertex_stride >= n_off + 4,
            "vertex_stride {} too small for normal sign byte (normal_off={})",
            vertex_stride,
            n_off
        );
    }

    ensure!(span_fits(span), "tangent data out of bounds");
    for i in 0..vertex_count {
        let base = (first_vertex + i) * vertex_stride + component_offset;

        let tx_raw = vertex_data[base] as f32;
        let ty_raw = vertex_data[base + 1] as f32;
        let tz_raw = vertex_data[base + 2] as f32;

        let tx = (tx_raw / 255.0) * 2.0 - 1.0;
        let ty = (ty_raw / 255.0) * 2.0 - 1.0;
        let tz = (tz_raw / 255.0) * 2.0 - 1.0;

        // Bitangent sign comes from the normal block's 4th byte, not the
        // tangent block's (which is the "unused" field per io_m3.py).
        let tw = match normal_offset {
            Some(n_off) => {
                let sign_byte = vertex_data[(first_vertex + i) * vertex_stride + n_off + 3];
                if sign_byte == 0 { -1.0_f32 } else { 1.0_f32 }
            }
            None => 1.0_f32,
        };

        // Normalise the xyz part (never zero-length — see `decode_normals_simd`).
        let inv = 1.0 / (tx * tx + ty * ty + tz * tz).sqrt();
        let (tx, ty, tz) = (tx * inv, ty * inv, tz * inv);

        soa.tangents_x.push(tx);
        soa.tangents_y.push(ty);
        soa.tangents_z.push(tz);
        soa.tangents_w.push(tw);
    }

    Ok(())
}

// ─── UV decoding ─────────────────────────────────────────────────────────────

/// Decode UV coordinates from int16 → f32.
/// Formula: `raw * uv_multiply / 32768 + uv_offset` (NO V flip).
///
/// m3studio (io_m3_import.py:37-41) does `v = 1 - (y*scale + offset)`, but that
/// adjustment is for Blender (V=0 at bottom, OpenGL convention). Blender's
/// glTF exporter applies a second flip, so the net effect is `v_glTF ≡ v_m3`.
/// We write glTF directly and pass the same V the M3 file holds, no flip.
/// Empirically: with the flip, tree textures swap (the trunk picks up the
/// foliage texture and vice versa) — a clear signal it's wrong here.
pub fn decode_uvs(
    span: VertexSpan<'_>,
    component_offset: usize,
    uv_multiply: f32,
    uv_offset: f32,
    soa: &mut MeshDataSoA,
) -> Result<()> {
    let VertexSpan { data: vertex_data, first: first_vertex, count: vertex_count, stride: vertex_stride } = span;
    ensure!(
        vertex_stride >= component_offset + 4,
        "vertex_stride {} too small for UV (offset={})",
        vertex_stride,
        component_offset
    );
    let scale: f32 = uv_multiply / 32768.0;
    ensure!(span_fits(span), "UV data out of bounds");
    for i in 0..vertex_count {
        let base = (first_vertex + i) * vertex_stride + component_offset;

        let u_raw = i16::from_le_bytes([vertex_data[base], vertex_data[base + 1]]);
        let v_raw = i16::from_le_bytes([vertex_data[base + 2], vertex_data[base + 3]]);

        // A corrupt region's multiply/offset can be non-finite.
        let finite = |v: f32| if v.is_finite() { v } else { 0.0 };
        soa.uvs_u.push(finite(u_raw as f32 * scale + uv_offset));
        soa.uvs_v.push(finite(v_raw as f32 * scale + uv_offset));
    }
    let dbg_start = soa.uvs_u.len().saturating_sub(vertex_count);
    for i in 0..vertex_count.min(3) {
        tracing::debug!(
            "  UV[{}]: u={:.4} v={:.4} (multiply={} offset={})",
            i,
            soa.uvs_u[dbg_start + i],
            soa.uvs_v[dbg_start + i],
            uv_multiply,
            uv_offset
        );
    }
    Ok(())
}

// ─── SIMD helper ─────────────────────────────────────────────────────────────

/// Multiply each element by `scale` via SIMD (8 floats per iteration).
#[multiversion(targets("x86_64+avx2", "x86_64+sse4.1", "aarch64+neon"))]
fn scale_array_simd(data: &[f32], scale: f32) -> Vec<f32> {
    let scale_v = f32x8::splat(scale);
    let mut out = Vec::with_capacity(data.len());

    let chunks = data.chunks_exact(8);
    let tail = chunks.remainder();

    for chunk in chunks {
        let arr: [f32; 8] = chunk.try_into().unwrap();
        let v = f32x8::from(arr) * scale_v;
        let result: [f32; 8] = v.into();
        out.extend_from_slice(&result);
    }

    // Tail (< 8 elements) — scalar.
    for &x in tail {
        out.push(x * scale);
    }

    out
}
