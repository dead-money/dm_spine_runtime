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

//! Linear RGBA color with clamp-on-mutate semantics matching `spine-cpp/Color.h`.

#![allow(clippy::many_single_char_names)] // `r`, `g`, `b`, `a` are the natural names for color components.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const BLACK: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };
    pub const WHITE: Self = Self {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };
    pub const TRANSPARENT: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    };

    /// Construct an RGBA color, clamping each channel to `[0, 1]`.
    #[must_use]
    pub fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        let mut c = Self { r, g, b, a };
        c.clamp();
        c
    }

    /// Overwrite all four channels and clamp.
    pub fn set(&mut self, r: f32, g: f32, b: f32, a: f32) -> &mut Self {
        self.r = r;
        self.g = g;
        self.b = b;
        self.a = a;
        self.clamp()
    }

    /// Overwrite RGB only (alpha unchanged) and clamp.
    pub fn set_rgb(&mut self, r: f32, g: f32, b: f32) -> &mut Self {
        self.r = r;
        self.g = g;
        self.b = b;
        self.clamp()
    }

    /// Copy from another color and clamp.
    pub fn set_from(&mut self, other: &Color) -> &mut Self {
        self.r = other.r;
        self.g = other.g;
        self.b = other.b;
        self.a = other.a;
        self.clamp()
    }

    /// Add another color component-wise and clamp.
    pub fn add(&mut self, r: f32, g: f32, b: f32, a: f32) -> &mut Self {
        self.r += r;
        self.g += g;
        self.b += b;
        self.a += a;
        self.clamp()
    }

    /// Add another color's RGB component-wise (alpha unchanged) and clamp.
    pub fn add_rgb(&mut self, r: f32, g: f32, b: f32) -> &mut Self {
        self.r += r;
        self.g += g;
        self.b += b;
        self.clamp()
    }

    /// Add another color component-wise and clamp.
    pub fn add_color(&mut self, other: &Color) -> &mut Self {
        self.r += other.r;
        self.g += other.g;
        self.b += other.b;
        self.a += other.a;
        self.clamp()
    }

    /// Clamp each channel to `[0, 1]`.
    pub fn clamp(&mut self) -> &mut Self {
        self.r = self.r.clamp(0.0, 1.0);
        self.g = self.g.clamp(0.0, 1.0);
        self.b = self.b.clamp(0.0, 1.0);
        self.a = self.a.clamp(0.0, 1.0);
        self
    }
}

impl Default for Color {
    /// Matches `spine::Color()` which zero-initializes all channels including alpha.
    fn default() -> Self {
        Self {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use proptest::prelude::*;

    #[test]
    fn default_matches_spine_cpp() {
        // spine-cpp Color() initializes all fields to 0 including alpha.
        let c = Color::default();
        assert_eq!(
            c,
            Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0
            }
        );
    }

    #[test]
    fn new_clamps_out_of_range() {
        let c = Color::new(-0.5, 2.0, 0.3, 1.5);
        assert_relative_eq!(c.r, 0.0);
        assert_relative_eq!(c.g, 1.0);
        assert_relative_eq!(c.b, 0.3);
        assert_relative_eq!(c.a, 1.0);
    }

    #[test]
    fn set_clamps() {
        let mut c = Color::WHITE;
        c.set(2.0, -1.0, 0.5, 10.0);
        assert_eq!(
            c,
            Color {
                r: 1.0,
                g: 0.0,
                b: 0.5,
                a: 1.0
            }
        );
    }

    #[test]
    fn set_rgb_preserves_alpha() {
        let mut c = Color::new(0.1, 0.2, 0.3, 0.4);
        c.set_rgb(0.9, 0.8, 0.7);
        assert_relative_eq!(c.r, 0.9);
        assert_relative_eq!(c.g, 0.8);
        assert_relative_eq!(c.b, 0.7);
        assert_relative_eq!(c.a, 0.4);
    }

    #[test]
    fn add_clamps_overflow() {
        let mut c = Color::new(0.8, 0.8, 0.8, 0.8);
        c.add(0.5, 0.5, 0.5, 0.5);
        assert_eq!(c, Color::WHITE);
    }

    #[test]
    fn add_clamps_underflow() {
        let mut c = Color::new(0.2, 0.2, 0.2, 0.2);
        c.add(-0.5, -0.5, -0.5, -0.5);
        assert_eq!(
            c,
            Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0
            }
        );
    }

    #[test]
    fn add_rgb_preserves_alpha() {
        let mut c = Color::new(0.1, 0.1, 0.1, 0.5);
        c.add_rgb(0.2, 0.2, 0.2);
        assert_relative_eq!(c.a, 0.5);
        assert_relative_eq!(c.r, 0.3);
    }

    #[test]
    fn add_color_matches_componentwise() {
        let mut c = Color::new(0.1, 0.2, 0.3, 0.4);
        let d = Color::new(0.2, 0.2, 0.2, 0.2);
        c.add_color(&d);
        assert_relative_eq!(c.r, 0.3);
        assert_relative_eq!(c.g, 0.4);
        assert_relative_eq!(c.b, 0.5);
        assert_relative_eq!(c.a, 0.6);
    }

    proptest! {
        /// After any `new`, all channels must be in [0, 1] — the Spine invariant.
        #[test]
        fn new_always_in_unit_range(
            r in -10.0f32..10.0, g in -10.0f32..10.0,
            b in -10.0f32..10.0, a in -10.0f32..10.0,
        ) {
            let c = Color::new(r, g, b, a);
            prop_assert!((0.0..=1.0).contains(&c.r));
            prop_assert!((0.0..=1.0).contains(&c.g));
            prop_assert!((0.0..=1.0).contains(&c.b));
            prop_assert!((0.0..=1.0).contains(&c.a));
        }

        /// After any `add`, all channels must stay in [0, 1].
        #[test]
        fn add_preserves_unit_range(
            r0 in 0.0f32..=1.0, g0 in 0.0f32..=1.0, b0 in 0.0f32..=1.0, a0 in 0.0f32..=1.0,
            dr in -2.0f32..2.0, dg in -2.0f32..2.0, db in -2.0f32..2.0, da in -2.0f32..2.0,
        ) {
            let mut c = Color::new(r0, g0, b0, a0);
            c.add(dr, dg, db, da);
            prop_assert!((0.0..=1.0).contains(&c.r));
            prop_assert!((0.0..=1.0).contains(&c.g));
            prop_assert!((0.0..=1.0).contains(&c.b));
            prop_assert!((0.0..=1.0).contains(&c.a));
        }
    }
}
