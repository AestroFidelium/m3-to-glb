//! Quaternion helpers shared across the converter.
//!
//! glTF requires unit quaternions and validates that every rotation component
//! lies in `[-1, 1]` (`VALUE_NOT_IN_RANGE`). M3 stores quaternions that are
//! only *approximately* unit-length, and composing them with the Z-up→Y-up
//! rotation can push a component to e.g. `1.0000001`. Every quaternion we emit
//! into glTF — animation samplers *and* node rest TRS — must therefore be
//! normalized and clamped through [`normalize_and_clamp`].

/// Normalize a quaternion `[x, y, z, w]` to unit length. Degenerate
/// (near-zero) input collapses to the identity quaternion.
#[inline]
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
mod tests {
    use super::*;

    fn len(q: [f32; 4]) -> f32 {
        (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt()
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
        let q = normalize_and_clamp([0.0, 0.7071068, 0.0, 0.7071068]);
        assert!((len(q) - 1.0).abs() < 1e-5);
    }
}
