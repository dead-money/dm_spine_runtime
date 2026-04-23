// Spine Runtimes License Agreement
// Last updated April 5, 2025. Replaces all prior versions.
//
// Copyright (c) 2013-2025, Esoteric Software LLC
//
// Integration of the Spine Runtimes into software or otherwise creating
// derivative works of the Spine Runtimes is permitted under the terms and
// conditions of Section 2 of the Spine Editor License Agreement:
// http://esotericsoftware.com/spine-editor-license
//
// Otherwise, it is permitted to integrate the Spine Runtimes into software
// or otherwise create derivative works of the Spine Runtimes (collectively,
// "Products"), provided that each user of the Products must obtain their own
// Spine Editor license and redistribution of the Products in any form must
// include this license and copyright notice.
//
// THE SPINE RUNTIMES ARE PROVIDED BY ESOTERIC SOFTWARE LLC "AS IS" AND ANY
// EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
// WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
// DISCLAIMED. IN NO EVENT SHALL ESOTERIC SOFTWARE LLC BE LIABLE FOR ANY
// DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
// (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES,
// BUSINESS INTERRUPTION, OR LOSS OF USE, DATA, OR PROFITS) HOWEVER CAUSED AND
// ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
// (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
// THE SPINE RUNTIMES, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

//! Degree-based trig helpers matching `spine-cpp/MathUtil`.
//!
//! Spine stores bone rotations and constraint mixes in degrees, so the runtime
//! calls `sinDeg` / `cosDeg` / `atan2Deg` pervasively. The spine-cpp versions
//! are thin wrappers over libm despite the "lookup table" doc comments, so the
//! Rust ports here just delegate to `f32::sin` / `f32::cos` / `f32::atan2`.
//!
//! `clamp`, `abs`, `signum`, and `rem` in Spine's `MathUtil` map 1:1 to the
//! `f32` inherent methods (`f32::clamp`, `f32::abs`, `f32::signum`, `%`) and
//! are not re-exported.

/// `sin(degrees)`.
#[inline]
#[must_use]
pub fn sin_deg(degrees: f32) -> f32 {
    degrees.to_radians().sin()
}

/// `cos(degrees)`.
#[inline]
#[must_use]
pub fn cos_deg(degrees: f32) -> f32 {
    degrees.to_radians().cos()
}

/// `atan2(y, x)` returned in degrees.
#[inline]
#[must_use]
pub fn atan2_deg(y: f32, x: f32) -> f32 {
    y.atan2(x).to_degrees()
}

/// Wraps an angle in degrees into `[-180, 180)` — useful for computing the
/// shortest-path rotation between two angles, as Spine does in several
/// constraint solvers.
#[inline]
#[must_use]
pub fn wrap_deg(degrees: f32) -> f32 {
    let mut d = degrees % 360.0;
    if d >= 180.0 {
        d -= 360.0;
    } else if d < -180.0 {
        d += 360.0;
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;
    use proptest::prelude::*;

    const EPS: f32 = 1e-5;

    #[test]
    fn sin_deg_known_values() {
        assert_abs_diff_eq!(sin_deg(0.0), 0.0, epsilon = EPS);
        assert_abs_diff_eq!(sin_deg(30.0), 0.5, epsilon = EPS);
        assert_abs_diff_eq!(sin_deg(90.0), 1.0, epsilon = EPS);
        assert_abs_diff_eq!(sin_deg(180.0), 0.0, epsilon = EPS);
        assert_abs_diff_eq!(sin_deg(270.0), -1.0, epsilon = EPS);
        assert_abs_diff_eq!(sin_deg(360.0), 0.0, epsilon = EPS);
    }

    #[test]
    fn cos_deg_known_values() {
        assert_abs_diff_eq!(cos_deg(0.0), 1.0, epsilon = EPS);
        assert_abs_diff_eq!(cos_deg(60.0), 0.5, epsilon = EPS);
        assert_abs_diff_eq!(cos_deg(90.0), 0.0, epsilon = EPS);
        assert_abs_diff_eq!(cos_deg(180.0), -1.0, epsilon = EPS);
        assert_abs_diff_eq!(cos_deg(270.0), 0.0, epsilon = EPS);
        assert_abs_diff_eq!(cos_deg(360.0), 1.0, epsilon = EPS);
    }

    #[test]
    fn atan2_deg_known_values() {
        assert_abs_diff_eq!(atan2_deg(0.0, 1.0), 0.0, epsilon = EPS);
        assert_abs_diff_eq!(atan2_deg(1.0, 0.0), 90.0, epsilon = EPS);
        assert_abs_diff_eq!(atan2_deg(1.0, 1.0), 45.0, epsilon = EPS);
        assert_abs_diff_eq!(atan2_deg(0.0, -1.0), 180.0, epsilon = EPS);
        assert_abs_diff_eq!(atan2_deg(-1.0, 0.0), -90.0, epsilon = EPS);
    }

    #[test]
    fn wrap_deg_known_values() {
        assert_abs_diff_eq!(wrap_deg(0.0), 0.0);
        assert_abs_diff_eq!(wrap_deg(179.0), 179.0);
        assert_abs_diff_eq!(wrap_deg(-179.0), -179.0);
        assert_abs_diff_eq!(wrap_deg(180.0), -180.0);
        assert_abs_diff_eq!(wrap_deg(181.0), -179.0);
        assert_abs_diff_eq!(wrap_deg(-181.0), 179.0);
        assert_abs_diff_eq!(wrap_deg(540.0), -180.0);
        assert_abs_diff_eq!(wrap_deg(-540.0), -180.0);
    }

    proptest! {
        /// Round-trip identity: polar coords that never land at the origin
        /// must recover the angle within float tolerance.
        #[test]
        fn atan2_deg_round_trip(
            // Keep angles in [-179, 179] to avoid the atan2 branch-cut discontinuity at ±180.
            angle in -179.0f32..=179.0,
            r in 1.0e-3f32..1.0e3,
        ) {
            let x = r * cos_deg(angle);
            let y = r * sin_deg(angle);
            let recovered = atan2_deg(y, x);
            prop_assert!(
                (recovered - angle).abs() < 1.0e-3,
                "angle={angle} recovered={recovered}",
            );
        }

        /// sin² + cos² = 1 for any angle.
        #[test]
        fn trig_pythagorean_identity(angle in -1.0e6f32..1.0e6) {
            let s = sin_deg(angle);
            let c = cos_deg(angle);
            prop_assert!((s * s + c * c - 1.0).abs() < 1.0e-4, "angle={angle}");
        }

        /// wrap_deg always returns a value in [-180, 180).
        #[test]
        fn wrap_deg_in_range(d in -1.0e6f32..1.0e6) {
            let w = wrap_deg(d);
            prop_assert!((-180.0..180.0).contains(&w), "w={w}");
        }
    }
}
