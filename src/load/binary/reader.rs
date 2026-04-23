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

//! Primitive readers for Spine's big-endian binary format.
//!
//! The wire layout (from `spine-cpp/SkeletonBinary.cpp`):
//!
//! - Integers: big-endian 4-byte `i32`.
//! - Floats: big-endian IEEE 754 (same bits as the `i32` above, reinterpreted).
//! - Varints: 1–5 bytes, MSB-continuation. Optional zigzag decoding for
//!   signed values (`optimize_positive == false`).
//! - Strings: varint length prefix `n`. `n == 0` means `None`. Otherwise
//!   the payload is `n - 1` bytes of UTF-8 (no trailing NUL on the wire).
//! - Colors: 4 bytes RGBA, each `u8 / 255.0`.
//!
//! The reader is panic-free on malformed input: every primitive returns a
//! `Result<_, BinaryError>` carrying the byte offset where the problem was
//! detected.

// These casts are intentional: spine's binary format encodes a mix of
// signed / unsigned values with shared bit-width. The reader returns
// `i32` for varints even in "optimize positive" mode to match spine-cpp's
// signature, and the zigzag path casts unsigned bit patterns to signed.
#![allow(clippy::cast_possible_wrap, clippy::cast_lossless)]

use thiserror::Error;

use crate::load::AttachmentLoaderError;
use crate::math::Color;

/// Errors produced while parsing a `.skel` file. Byte offsets are 0-based
/// from the start of the buffer.
#[derive(Debug, Error)]
pub enum BinaryError {
    #[error("unexpected end of input at byte {at} (wanted {wanted} more byte(s))")]
    UnexpectedEof { at: usize, wanted: usize },

    #[error("byte {at}: varint overflowed 32 bits")]
    VarintOverflow { at: usize },

    #[error("byte {at}: string is not valid UTF-8: {source}")]
    InvalidUtf8 {
        at: usize,
        #[source]
        source: std::str::Utf8Error,
    },

    #[error("byte {at}: string-table index {index} out of range ({len} entries)")]
    StringRefOutOfRange { at: usize, index: usize, len: usize },

    #[error("skeleton version mismatch: file reports {found:?}, runtime targets {expected:?}")]
    UnsupportedVersion { found: String, expected: String },

    #[error("byte {at}: {entity} index {index} out of range ({len} entries)")]
    IndexOutOfRange {
        at: usize,
        entity: &'static str,
        index: usize,
        len: usize,
    },

    #[error("byte {at}: unknown {entity} discriminant {value}")]
    UnknownDiscriminant {
        at: usize,
        entity: &'static str,
        value: u32,
    },

    #[error("byte {at}: linked-mesh parent {parent:?} not found on skin {skin:?} slot {slot}")]
    LinkedMeshParentMissing {
        at: usize,
        skin: String,
        slot: usize,
        parent: String,
    },

    #[error("attachment loader failed: {0}")]
    AttachmentLoader(#[from] AttachmentLoaderError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Cursor-style reader over a `&[u8]` buffer.
pub(crate) struct BinaryReader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> BinaryReader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// Current byte offset from the start of the buffer. Used in error
    /// diagnostics and to sanity-check that a full parse consumed the file.
    pub fn position(&self) -> usize {
        self.pos
    }

    fn need(&self, n: usize) -> Result<(), BinaryError> {
        if self.pos + n > self.buf.len() {
            Err(BinaryError::UnexpectedEof {
                at: self.pos,
                wanted: n,
            })
        } else {
            Ok(())
        }
    }

    pub fn read_byte(&mut self) -> Result<u8, BinaryError> {
        self.need(1)?;
        let b = self.buf[self.pos];
        self.pos += 1;
        Ok(b)
    }

    pub fn read_sbyte(&mut self) -> Result<i8, BinaryError> {
        self.read_byte().map(|b| b as i8)
    }

    pub fn read_bool(&mut self) -> Result<bool, BinaryError> {
        self.read_byte().map(|b| b != 0)
    }

    /// Big-endian signed 32-bit integer.
    pub fn read_int(&mut self) -> Result<i32, BinaryError> {
        self.need(4)?;
        let b0 = self.buf[self.pos];
        let b1 = self.buf[self.pos + 1];
        let b2 = self.buf[self.pos + 2];
        let b3 = self.buf[self.pos + 3];
        self.pos += 4;
        Ok(i32::from_be_bytes([b0, b1, b2, b3]))
    }

    /// Big-endian IEEE-754 single-precision float (same bit pattern as
    /// [`Self::read_int`] reinterpreted).
    pub fn read_float(&mut self) -> Result<f32, BinaryError> {
        self.read_int().map(|bits| f32::from_bits(bits as u32))
    }

    /// Variable-length integer. When `optimize_positive == true`, the value
    /// is treated as unsigned. When `false`, Spine's zigzag decoding is
    /// applied so that small magnitudes (positive or negative) encode in few
    /// bytes.
    ///
    /// The upper 4-bit bit of the 5th byte is masked off to match spine-cpp,
    /// which also caps at 32 bits.
    pub fn read_varint(&mut self, optimize_positive: bool) -> Result<i32, BinaryError> {
        // Up to 5 bytes; each carries 7 value bits in the low nibble-and-some,
        // with the high bit signaling continuation. Matches spine-cpp.
        let start = self.pos;
        let mut b = self.read_byte()?;
        let mut value = u32::from(b & 0x7F);
        if b & 0x80 != 0 {
            b = self.read_byte()?;
            value |= u32::from(b & 0x7F) << 7;
            if b & 0x80 != 0 {
                b = self.read_byte()?;
                value |= u32::from(b & 0x7F) << 14;
                if b & 0x80 != 0 {
                    b = self.read_byte()?;
                    value |= u32::from(b & 0x7F) << 21;
                    if b & 0x80 != 0 {
                        b = self.read_byte()?;
                        value |= u32::from(b & 0x7F) << 28;
                    }
                }
            }
        }
        // 32-bit cap — if more bytes were present they'd indicate overflow.
        // spine-cpp silently truncates; we surface the error.
        if self.pos == start + 5 && (b & 0x80) != 0 {
            return Err(BinaryError::VarintOverflow { at: start });
        }

        Ok(if optimize_positive {
            value as i32
        } else {
            // Zigzag decode: the sign mask is `-(value & 1)` in two's
            // complement — i.e. all-zeros for even values, all-ones for odd.
            // XOR with the unsigned right shift recovers the original signed
            // integer: 0→0, 1→-1, 2→1, 3→-2, 4→2, …
            let sign_mask = (value & 1).wrapping_neg();
            ((value >> 1) ^ sign_mask) as i32
        })
    }

    /// Same as [`Self::read_varint`] with `optimize_positive = true`, returning
    /// a `usize` — convenient for count fields.
    pub fn read_uvarint(&mut self) -> Result<usize, BinaryError> {
        let v = self.read_varint(true)?;
        Ok(v as u32 as usize)
    }

    /// Length-prefixed string. Returns `None` when the length field is zero,
    /// matching spine-cpp's NULL convention for "absent string".
    pub fn read_string(&mut self) -> Result<Option<String>, BinaryError> {
        let len = self.read_uvarint()?;
        if len == 0 {
            return Ok(None);
        }
        let payload_len = len - 1;
        self.need(payload_len)?;
        let start = self.pos;
        let slice = &self.buf[start..start + payload_len];
        self.pos += payload_len;
        std::str::from_utf8(slice)
            .map(|s| Some(s.to_string()))
            .map_err(|source| BinaryError::InvalidUtf8 { at: start, source })
    }

    /// String-table indexed string. The file stores a flat `Vec<String>`
    /// early on; subsequent references encode `index + 1` (so `0` still
    /// means `None`).
    pub fn read_string_ref(&mut self, strings: &[String]) -> Result<Option<String>, BinaryError> {
        let at = self.pos;
        let index = self.read_uvarint()?;
        if index == 0 {
            return Ok(None);
        }
        let idx = index - 1;
        strings
            .get(idx)
            .cloned()
            .map(Some)
            .ok_or(BinaryError::StringRefOutOfRange {
                at,
                index: idx,
                len: strings.len(),
            })
    }

    /// 4-byte RGBA color (each channel `u8 / 255.0`).
    pub fn read_color(&mut self) -> Result<Color, BinaryError> {
        let r = f32::from(self.read_byte()?) / 255.0;
        let g = f32::from(self.read_byte()?) / 255.0;
        let b = f32::from(self.read_byte()?) / 255.0;
        let a = f32::from(self.read_byte()?) / 255.0;
        Ok(Color::new(r, g, b, a))
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn read_byte_sbyte_bool() {
        let mut r = BinaryReader::new(&[0x00, 0xFF, 0x7F, 0x80]);
        assert_eq!(r.read_byte().unwrap(), 0x00);
        assert!(r.read_bool().unwrap());
        assert_eq!(r.read_sbyte().unwrap(), 0x7F);
        assert_eq!(r.read_sbyte().unwrap(), -128);
    }

    #[test]
    fn read_int_is_big_endian() {
        let mut r = BinaryReader::new(&[0x00, 0x00, 0x00, 0x2A]);
        assert_eq!(r.read_int().unwrap(), 42);

        let mut r = BinaryReader::new(&[0xFF, 0xFF, 0xFF, 0xFF]);
        assert_eq!(r.read_int().unwrap(), -1);

        let mut r = BinaryReader::new(&[0x12, 0x34, 0x56, 0x78]);
        assert_eq!(r.read_int().unwrap(), 0x1234_5678);
    }

    #[test]
    fn read_float_reinterprets_int_bits() {
        // 1.0f32 = 0x3F800000 in IEEE 754.
        let mut r = BinaryReader::new(&[0x3F, 0x80, 0x00, 0x00]);
        assert_eq!(r.read_float().unwrap(), 1.0);
    }

    #[test]
    fn read_varint_positive_single_byte() {
        let mut r = BinaryReader::new(&[0x2A]);
        assert_eq!(r.read_varint(true).unwrap(), 42);
    }

    #[test]
    fn read_varint_positive_multi_byte() {
        // 300 = 0xAC 0x02 in Spine's little-endian-by-group scheme:
        // low 7 bits = 0x2C (0b0101100) with continuation → 0xAC, then 0x02 (high bits).
        let mut r = BinaryReader::new(&[0xAC, 0x02]);
        assert_eq!(r.read_varint(true).unwrap(), 300);
    }

    #[test]
    fn read_varint_zigzag_negative() {
        // -1 zigzag-encodes to 1 = 0x01. 1 encodes to 2 = 0x02. -2 encodes to 3 = 0x03.
        let mut r = BinaryReader::new(&[0x01, 0x02, 0x03]);
        assert_eq!(r.read_varint(false).unwrap(), -1);
        assert_eq!(r.read_varint(false).unwrap(), 1);
        assert_eq!(r.read_varint(false).unwrap(), -2);
    }

    #[test]
    fn read_string_none_on_zero_length() {
        let mut r = BinaryReader::new(&[0x00]);
        assert_eq!(r.read_string().unwrap(), None);
    }

    #[test]
    fn read_string_payload_is_length_minus_one_bytes() {
        // "hi" is 2 bytes of payload. Length prefix on the wire is 3.
        let mut r = BinaryReader::new(&[0x03, b'h', b'i']);
        assert_eq!(r.read_string().unwrap().as_deref(), Some("hi"));
    }

    #[test]
    fn read_string_ref_resolves_to_table() {
        let strings = vec!["alpha".to_string(), "beta".to_string()];
        let mut r = BinaryReader::new(&[0x00, 0x01, 0x02]);
        assert_eq!(r.read_string_ref(&strings).unwrap(), None);
        assert_eq!(
            r.read_string_ref(&strings).unwrap().as_deref(),
            Some("alpha")
        );
        assert_eq!(
            r.read_string_ref(&strings).unwrap().as_deref(),
            Some("beta")
        );
    }

    #[test]
    fn read_string_ref_out_of_range_is_error() {
        let strings = vec!["alpha".to_string()];
        let mut r = BinaryReader::new(&[0x03]);
        match r.read_string_ref(&strings) {
            Err(BinaryError::StringRefOutOfRange { index, len, .. }) => {
                assert_eq!(index, 2);
                assert_eq!(len, 1);
            }
            other => panic!("expected StringRefOutOfRange, got {other:?}"),
        }
    }

    #[test]
    fn read_color_rgba_normalized() {
        let mut r = BinaryReader::new(&[0xFF, 0x00, 0x80, 0x40]);
        let c = r.read_color().unwrap();
        assert_eq!(c.r, 1.0);
        assert_eq!(c.g, 0.0);
        assert!((c.b - 0x80 as f32 / 255.0).abs() < 1e-6);
        assert!((c.a - 0x40 as f32 / 255.0).abs() < 1e-6);
    }

    #[test]
    fn eof_surfaces_as_error() {
        let mut r = BinaryReader::new(&[0x00]);
        assert!(matches!(
            r.read_int(),
            Err(BinaryError::UnexpectedEof { .. })
        ));
    }

    #[test]
    fn position_tracks_cursor() {
        let mut r = BinaryReader::new(&[0x01, 0x02, 0x03, 0x04, 0x05]);
        assert_eq!(r.position(), 0);
        r.read_byte().unwrap();
        assert_eq!(r.position(), 1);
        r.read_int().unwrap();
        assert_eq!(r.position(), 5);
    }
}
