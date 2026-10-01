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

//! Curve-timeline sampling and blending.
//!
//! Functions take the raw `frames` / `curves` slices of a
//! [`CurveFrames`][crate::data::CurveFrames], so they also work on scratch
//! buffers. Frames are laid out `[t0, values…, t1, values…]`; `curves` is
//! laid out as described on [`BEZIER_SIZE`]. Slices that don't follow this
//! layout panic on an out-of-bounds index.
//!
//! The blending functions take the mix parameters described in
//! [`apply`][crate::animation::apply]: `alpha`, `from`, `add`, and `out`.

// spine-cpp's short variable names, kept so the code diffs against it.
#![allow(clippy::many_single_char_names)]
// `alpha == 1.0` is an exact sentinel check, as in spine-cpp.
#![allow(clippy::float_cmp)]

use crate::animation::{BEZIER_SIZE, CURVE_BEZIER, CURVE_LINEAR, CURVE_STEPPED, MixFrom};

/// The 9 `(x, y)` samples of one bezier segment, laid out `x0, y0, x1, y1, …`
/// as stored in `CurveFrames::curves`. `(time1, value1)` and `(time2, value2)`
/// are the segment's keys; `cx1..cy2` are its control points.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn compute_bezier_samples(
    time1: f32,
    value1: f32,
    cx1: f32,
    cy1: f32,
    cx2: f32,
    cy2: f32,
    time2: f32,
    value2: f32,
) -> [f32; BEZIER_SIZE] {
    // Forward differencing at t = 0.1, 0.2, …, 0.9, from
    // `CurveTimeline::setBezier`. Keep the constants literal for bit-exact
    // parity.
    let tmpx = (time1 - cx1 * 2.0 + cx2) * 0.03;
    let tmpy = (value1 - cy1 * 2.0 + cy2) * 0.03;
    let dddx = ((cx1 - cx2) * 3.0 - time1 + time2) * 0.006;
    let dddy = ((cy1 - cy2) * 3.0 - value1 + value2) * 0.006;
    let mut ddx = tmpx * 2.0 + dddx;
    let mut ddy = tmpy * 2.0 + dddy;
    let mut dx = (cx1 - time1) * 0.3 + tmpx + dddx * 0.166_666_67;
    let mut dy = (cy1 - value1) * 0.3 + tmpy + dddy * 0.166_666_67;
    let mut x = time1 + dx;
    let mut y = value1 + dy;
    let mut out = [0.0_f32; BEZIER_SIZE];
    for k in 0..(BEZIER_SIZE / 2) {
        out[k * 2] = x;
        out[k * 2 + 1] = y;
        dx += ddx;
        dy += ddy;
        ddx += dddx;
        ddy += dddy;
        x += dx;
        y += dy;
    }
    out
}

/// Index of the last frame (a multiple of `step`) whose time is `<= target`.
/// Returns 0 when `target` precedes the second frame, and `frames.len() - step`
/// past the last frame.
#[must_use]
pub fn search(frames: &[f32], target: f32, step: usize) -> usize {
    let n = frames.len();
    let mut i = step;
    while i < n {
        if frames[i] > target {
            return i - step;
        }
        i += step;
    }
    n - step
}

/// Samples one channel of a bezier segment at `time`.
///
/// `frame_index` is the segment's start frame in `frames`, `value_offset`
/// the channel's offset within a frame, and `frame_entries` the frame stride.
/// `i` is the segment's start offset in `curves`:
/// `curves[frame_index / frame_entries] - CURVE_BEZIER`, plus `BEZIER_SIZE`
/// per later channel.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn bezier_value(
    frames: &[f32],
    curves: &[f32],
    time: f32,
    frame_index: usize,
    value_offset: usize,
    i: usize,
    frame_entries: usize,
) -> f32 {
    // Before the first sample: interpolate from the start key.
    if curves[i] > time {
        let x = frames[frame_index];
        let y = frames[frame_index + value_offset];
        return y + (time - x) / (curves[i] - x) * (curves[i + 1] - y);
    }

    let n = i + BEZIER_SIZE;
    let mut j = i + 2;
    while j < n {
        if curves[j] >= time {
            let x = curves[j - 2];
            let y = curves[j - 1];
            return y + (time - x) / (curves[j] - x) * (curves[j + 1] - y);
        }
        j += 2;
    }

    // Past the last sample: interpolate to the next key.
    let next_frame = frame_index + frame_entries;
    let x = curves[n - 2];
    let y = curves[n - 1];
    y + (time - x) / (frames[next_frame] - x) * (frames[next_frame + value_offset] - y)
}

/// Samples a one-value curve timeline (`frames = [t0, v0, t1, v1, …]`) at
/// `time`. `time` must be at or after the first key.
#[must_use]
pub fn curve_value1(frames: &[f32], curves: &[f32], time: f32) -> f32 {
    const ENTRIES: usize = 2;
    const VALUE: usize = 1;

    // spine-cpp's inlined search, kept literal.
    let mut i: isize = frames.len() as isize - ENTRIES as isize;
    let mut ii = ENTRIES;
    while ii as isize <= i {
        if frames[ii] > time {
            i = ii as isize - ENTRIES as isize;
            break;
        }
        ii += ENTRIES;
    }
    let i = i as usize;

    let curve_type = curves[i / ENTRIES] as i32;
    match curve_type {
        CURVE_LINEAR => {
            let before = frames[i];
            let value = frames[i + VALUE];
            value
                + (time - before) / (frames[i + ENTRIES] - before)
                    * (frames[i + ENTRIES + VALUE] - value)
        }
        CURVE_STEPPED => frames[i + VALUE],
        _ => bezier_value(
            frames,
            curves,
            time,
            i,
            VALUE,
            (curve_type - CURVE_BEZIER) as usize,
            ENTRIES,
        ),
    }
}

/// Samples a two-value curve timeline (`frames = [t0, x0, y0, …]`) at `time`.
/// `time` must be at or after the first key.
#[must_use]
pub fn curve_value2(frames: &[f32], curves: &[f32], time: f32) -> (f32, f32) {
    const ENTRIES: usize = 3;
    const VALUE1: usize = 1;
    const VALUE2: usize = 2;

    let i = search(frames, time, ENTRIES);
    let curve_type = curves[i / ENTRIES] as i32;
    match curve_type {
        CURVE_LINEAR => {
            let before = frames[i];
            let mut x = frames[i + VALUE1];
            let mut y = frames[i + VALUE2];
            let t = (time - before) / (frames[i + ENTRIES] - before);
            x += (frames[i + ENTRIES + VALUE1] - x) * t;
            y += (frames[i + ENTRIES + VALUE2] - y) * t;
            (x, y)
        }
        CURVE_STEPPED => (frames[i + VALUE1], frames[i + VALUE2]),
        _ => {
            let bezier_i = (curve_type - CURVE_BEZIER) as usize;
            let x = bezier_value(frames, curves, time, i, VALUE1, bezier_i, ENTRIES);
            let y = bezier_value(
                frames,
                curves,
                time,
                i,
                VALUE2,
                bezier_i + BEZIER_SIZE,
                ENTRIES,
            );
            (x, y)
        }
    }
}

/// The value a property takes before a timeline's first key.
#[inline]
#[must_use]
pub fn before_first_key(from: MixFrom, alpha: f32, current: f32, setup: f32) -> f32 {
    match from {
        MixFrom::Setup => setup,
        MixFrom::First => current + (setup - current) * alpha,
        MixFrom::Current => current,
    }
}

/// Blends a timeline whose keyed values are offsets from `setup`, such as
/// rotation or translation. `current` is the property's value before this
/// timeline.
#[must_use]
pub fn relative_value(
    frames: &[f32],
    curves: &[f32],
    time: f32,
    alpha: f32,
    from: MixFrom,
    add: bool,
    current: f32,
    setup: f32,
) -> f32 {
    if time < frames[0] {
        return before_first_key(from, alpha, current, setup);
    }
    let value = curve_value1(frames, curves, time);
    if from == MixFrom::Setup {
        setup + value * alpha
    } else {
        current + (if add { value } else { value + setup - current }) * alpha
    }
}

/// Blends a timeline whose keyed values replace the property. `value`, when
/// given, is used instead of the curve sample.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn absolute_value(
    frames: &[f32],
    curves: &[f32],
    time: f32,
    alpha: f32,
    from: MixFrom,
    add: bool,
    current: f32,
    setup: f32,
    value: Option<f32>,
) -> f32 {
    if time < frames[0] {
        return before_first_key(from, alpha, current, setup);
    }
    let value = value.unwrap_or_else(|| curve_value1(frames, curves, time));
    if from == MixFrom::Setup {
        setup + (if add { value } else { value - setup }) * alpha
    } else {
        current + (if add { value } else { value - current }) * alpha
    }
}

/// Blends a scale timeline, whose keyed values multiply `setup`. A
/// non-additive blend takes the keyed value's sign, or with `out` the sign
/// of the value it mixes from, so it doesn't pass through zero.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn scale_value(
    frames: &[f32],
    curves: &[f32],
    time: f32,
    alpha: f32,
    from: MixFrom,
    add: bool,
    out: bool,
    current: f32,
    setup: f32,
) -> f32 {
    if time < frames[0] {
        return before_first_key(from, alpha, current, setup);
    }
    let value = curve_value1(frames, curves, time) * setup;
    if alpha == 1.0 && !add {
        return value;
    }
    let mut base = if from == MixFrom::Setup {
        setup
    } else {
        current
    };
    if add {
        return base + (value - setup) * alpha;
    }
    if out {
        return base + (value.abs() * sign(base) - base) * alpha;
    }
    base = base.abs() * sign(value);
    base + (value - base) * alpha
}

/// Returns 0 for 0 (and NaN), unlike `f32::signum`.
#[inline]
#[must_use]
pub fn sign(v: f32) -> f32 {
    if v < 0.0 {
        -1.0
    } else if v > 0.0 {
        1.0
    } else {
        0.0
    }
}

/// Samples `N` channels of a multi-value curve timeline with `entries` floats
/// per frame. Returns the frame's start index and the values.
#[must_use]
pub fn curve_values<const N: usize>(
    frames: &[f32],
    curves: &[f32],
    time: f32,
    entries: usize,
) -> (usize, [f32; N]) {
    let i = search(frames, time, entries);
    let curve_type = curves[i / entries] as i32;
    let mut out = [0.0; N];
    match curve_type {
        CURVE_LINEAR => {
            let before = frames[i];
            let t = (time - before) / (frames[i + entries] - before);
            for (c, v) in out.iter_mut().enumerate() {
                let a = frames[i + 1 + c];
                *v = a + (frames[i + entries + 1 + c] - a) * t;
            }
        }
        CURVE_STEPPED => {
            for (c, v) in out.iter_mut().enumerate() {
                *v = frames[i + 1 + c];
            }
        }
        _ => {
            let base = (curve_type - CURVE_BEZIER) as usize;
            for (c, v) in out.iter_mut().enumerate() {
                *v = bezier_value(
                    frames,
                    curves,
                    time,
                    i,
                    1 + c,
                    base + BEZIER_SIZE * c,
                    entries,
                );
            }
        }
    }
    (i, out)
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // testing exact algebra on small inputs
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    #[test]
    fn search_finds_largest_le() {
        // frames = [0.0, v, 1.0, v, 2.5, v] — step=2, values in odd slots.
        let frames = [0.0_f32, 10.0, 1.0, 20.0, 2.5, 30.0];
        assert_eq!(search(&frames, -0.5, 2), 0);
        assert_eq!(search(&frames, 0.0, 2), 0);
        assert_eq!(search(&frames, 0.5, 2), 0);
        assert_eq!(search(&frames, 1.0, 2), 2);
        assert_eq!(search(&frames, 2.0, 2), 2);
        assert_eq!(search(&frames, 2.5, 2), 4);
        assert_eq!(search(&frames, 100.0, 2), 4);
    }

    /// Two-frame linear ramp from 0 at t=0 to 10 at t=1.
    fn linear_ramp_1() -> (Vec<f32>, Vec<f32>) {
        // Frame layout: [t0, v0, t1, v1] = [0, 0, 1, 10].
        // The last frame's curve type is never read.
        (
            vec![0.0, 0.0, 1.0, 10.0],
            vec![CURVE_LINEAR as f32, CURVE_STEPPED as f32],
        )
    }

    #[test]
    fn curve_value1_linear_interpolation() {
        let (f, c) = linear_ramp_1();
        assert_abs_diff_eq!(curve_value1(&f, &c, 0.0), 0.0);
        assert_abs_diff_eq!(curve_value1(&f, &c, 0.25), 2.5);
        assert_abs_diff_eq!(curve_value1(&f, &c, 0.5), 5.0);
        assert_abs_diff_eq!(curve_value1(&f, &c, 1.0), 10.0);
    }

    #[test]
    fn curve_value1_stepped_returns_left_value() {
        let frames = vec![0.0, 1.0, 1.0, 42.0];
        let curves = vec![CURVE_STEPPED as f32, CURVE_STEPPED as f32];
        assert_abs_diff_eq!(curve_value1(&frames, &curves, 0.0), 1.0);
        assert_abs_diff_eq!(curve_value1(&frames, &curves, 0.5), 1.0);
        assert_abs_diff_eq!(curve_value1(&frames, &curves, 0.999), 1.0);
    }

    #[test]
    fn curve_value2_linear_interpolation_on_translate_layout() {
        // TranslateTimeline stride 3: [t0, x0, y0, t1, x1, y1].
        let frames = vec![0.0_f32, 1.0, 2.0, 1.0, 11.0, 22.0];
        let curves = vec![CURVE_LINEAR as f32, CURVE_STEPPED as f32];
        let (x0, y0) = curve_value2(&frames, &curves, 0.0);
        let (xm, ym) = curve_value2(&frames, &curves, 0.5);
        let (x1, y1) = curve_value2(&frames, &curves, 1.0);
        assert_abs_diff_eq!(x0, 1.0);
        assert_abs_diff_eq!(y0, 2.0);
        assert_abs_diff_eq!(xm, 6.0);
        assert_abs_diff_eq!(ym, 12.0);
        assert_abs_diff_eq!(x1, 11.0);
        assert_abs_diff_eq!(y1, 22.0);
    }

    #[test]
    fn relative_value_by_mix_from() {
        let (f, c) = linear_ramp_1();
        // Curve value 5 at t=0.5; current 7, setup 3, alpha 0.5.
        assert_abs_diff_eq!(
            relative_value(&f, &c, 0.5, 0.5, MixFrom::Setup, false, 7.0, 3.0),
            5.5
        );
        assert_abs_diff_eq!(
            relative_value(&f, &c, 0.5, 0.5, MixFrom::Current, true, 7.0, 3.0),
            9.5
        );
        assert_abs_diff_eq!(
            relative_value(&f, &c, 0.5, 0.5, MixFrom::Current, false, 7.0, 3.0),
            7.5
        );
    }

    #[test]
    fn before_first_key_by_mix_from() {
        let (f, c) = linear_ramp_1();
        let v = |from| relative_value(&f, &c, -1.0, 0.5, from, false, 7.0, 3.0);
        assert_abs_diff_eq!(v(MixFrom::Setup), 3.0);
        assert_abs_diff_eq!(v(MixFrom::First), 5.0);
        assert_abs_diff_eq!(v(MixFrom::Current), 7.0);
    }

    #[test]
    fn absolute_value_by_mix_from() {
        let (f, c) = linear_ramp_1();
        let v = |from, add| absolute_value(&f, &c, 0.5, 0.5, from, add, 7.0, 3.0, None);
        assert_abs_diff_eq!(v(MixFrom::Setup, false), 4.0);
        assert_abs_diff_eq!(v(MixFrom::Current, false), 6.0);
        assert_abs_diff_eq!(v(MixFrom::Current, true), 9.5);
    }
}
