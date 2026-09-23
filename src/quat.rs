//! Quaternion helpers shared across the converter.
//!
//! glTF requires unit quaternions and validates that every rotation component
//! lies in `[-1, 1]` (`VALUE_NOT_IN_RANGE`). M3 stores quaternions that are
//! only *approximately* unit-length, and composing them with the Z-up→Y-up
//! rotation can push a component to e.g. `1.0000001`. Every quaternion we emit
//! into glTF — animation samplers *and* node rest TRS — must therefore be
//! normalized and clamped through [`normalize_and_clamp`].

/// M3 is Z-up, glTF Y-up: the conversion is a -90° rotation about X,
/// `(-sin 45°, 0, 0, cos 45°)` as `[x, y, z, w]`. It is baked into vertices,
/// root bones and root-bone animation keys alike.
pub const Z_UP_TO_Y_UP: [f32; 4] = [
    -std::f32::consts::FRAC_1_SQRT_2,
    0.0,
    0.0,
    std::f32::consts::FRAC_1_SQRT_2,
];

/// Hamilton product `a ⊗ b` of `[x, y, z, w]` quaternions — `b` applied first.
#[inline]
#[must_use]
pub fn mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

/// Rotate `v` by the unit quaternion `q`: `q · v · q⁻¹`.
#[inline]
#[must_use]
pub fn rotate(v: [f32; 3], q: [f32; 4]) -> [f32; 3] {
    let [qx, qy, qz, qw] = q;
    // t = 2 · (q.xyz × v);  v' = v + w·t + q.xyz × t
    let t = [
        2.0 * (qy * v[2] - qz * v[1]),
        2.0 * (qz * v[0] - qx * v[2]),
        2.0 * (qx * v[1] - qy * v[0]),
    ];
    [
        v[0] + qw * t[0] + (qy * t[2] - qz * t[1]),
        v[1] + qw * t[1] + (qz * t[0] - qx * t[2]),
        v[2] + qw * t[2] + (qx * t[1] - qy * t[0]),
    ]
}

/// Normalize a quaternion `[x, y, z, w]` to unit length. Degenerate
/// (near-zero) input collapses to the identity quaternion.
#[inline]
#[must_use]
pub fn normalize(q: [f32; 4]) -> [f32; 4] {
    let len_sq = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    if len_sq > 1e-12 {
        let inv = 1.0 / len_sq.sqrt();
        [q[0] * inv, q[1] * inv, q[2] * inv, q[3] * inv]
    } else {
        [0.0, 0.0, 0.0, 1.0] // identity quaternion
    }
}

/// Normalize, then clamp each component to `[-1.0, 1.0]`.
///
/// Normalization alone can still leave a component a hair outside the range
/// because of `sqrt` rounding (a unit quaternion whose largest component is
/// `1.0` may round to `1.0000001`). glTF's validator rejects that, so we clamp
/// the residue. This is the only quaternion form that may be written to glTF.
#[inline]
#[must_use]
pub fn normalize_and_clamp(q: [f32; 4]) -> [f32; 4] {
    let n = normalize(q);
    [
        n[0].clamp(-1.0, 1.0),
        n[1].clamp(-1.0, 1.0),
        n[2].clamp(-1.0, 1.0),
        n[3].clamp(-1.0, 1.0),
    ]
}

#[cfg(test)]
#[allow(clippy::float_cmp, reason = "identity and clamping results are exact")]
mod tests {
    use super::*;

    fn len(q: [f32; 4]) -> f32 {
        (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt()
    }

    #[test]
    fn z_up_to_y_up_maps_z_to_y() {
        let v = rotate([0.0, 0.0, 1.0], Z_UP_TO_Y_UP);
        assert!((v[1] - 1.0).abs() < 1e-6 && v[0].abs() < 1e-6 && v[2].abs() < 1e-6, "{v:?}");
        // Composing with the identity changes nothing.
        assert_eq!(mul(Z_UP_TO_Y_UP, [0.0, 0.0, 0.0, 1.0]), Z_UP_TO_Y_UP);
    }

    #[test]
    fn normalize_unit_length() {
        let q = normalize([3.0, 0.0, 4.0, 0.0]);
        assert!((len(q) - 1.0).abs() < 1e-6, "expected unit length, got {}", len(q));
    }

    #[test]
    fn normalize_degenerate_is_identity() {
        assert_eq!(normalize([0.0, 0.0, 0.0, 0.0]), [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn clamp_kills_out_of_range_residue() {
        // The exact spec violation seen on War3_Kelthuzad: /nodes/0/rotation/3.
        let q = normalize_and_clamp([0.0, 0.0, 0.0, 1.000_000_1]);
        for (i, &c) in q.iter().enumerate() {
            assert!(
                (-1.0..=1.0).contains(&c),
                "component {i} = {c} is out of glTF range [-1, 1]",
            );
        }
        assert_eq!(q[3], 1.0);
    }

    #[test]
    fn clamp_preserves_already_unit() {
        let q = normalize_and_clamp([0.0, std::f32::consts::FRAC_1_SQRT_2, 0.0, std::f32::consts::FRAC_1_SQRT_2]);
        assert!((len(q) - 1.0).abs() < 1e-5);
    }
}
