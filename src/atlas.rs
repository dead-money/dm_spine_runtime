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

//! Parser for the Spine `.atlas` text format, ported from
//! `spine-cpp/src/spine/Atlas.cpp`.
//!
//! An atlas is a text file describing one or more texture pages and the
//! rectangular regions within them that skeletons reference. This module only
//! parses the metadata; actual texture pixels are loaded by the renderer
//! (e.g. `dm_spine_bevy` resolves `AtlasPage::name` to a `Handle<Image>`).
//!
//! # Supported features
//!
//! - Legacy per-region fields (`xy`, `size`, `offset`, `orig`).
//! - 4.1+ compact fields (`bounds`, `offsets`).
//! - Rotation written as `rotate: true` (90°), `rotate: false` (0°), or an
//!   explicit integer degree count.
//! - `repeat` / `filter` / `format` / `pma` page-level options.
//! - Unknown keys captured as `(name, values)` pairs on the region — matches
//!   the spine-cpp fallback so runtime extensions keep working.
//!
//! # Format example
//!
//! ```text
//! spineboy.png
//!   size: 1024, 256
//!   filter: Linear, Linear
//! crosshair
//!   bounds: 352, 7, 45, 45
//! eye-indifferent
//!   bounds: 862, 105, 47, 45
//! ```

use thiserror::Error;

/// Pixel format hint written by the Spine editor. Runtime consumers may use
/// this to pick an appropriate GPU texture format, but most just upload the
/// source PNG as RGBA8 and ignore the hint.
///
/// `Unknown` is used when the atlas omits the `format:` line or writes an
/// unrecognised value — matching spine-cpp's silent fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Unknown,
    Alpha,
    Intensity,
    LuminanceAlpha,
    Rgb565,
    Rgba4444,
    Rgb888,
    Rgba8888,
}

/// Texture minification / magnification filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureFilter {
    Unknown,
    Nearest,
    Linear,
    MipMap,
    MipMapNearestNearest,
    MipMapLinearNearest,
    MipMapNearestLinear,
    MipMapLinearLinear,
}

/// Texture coordinate wrap mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureWrap {
    MirroredRepeat,
    ClampToEdge,
    Repeat,
}

/// One texture page declared in an atlas — typically backed by a PNG file.
#[derive(Debug, Clone, PartialEq)]
pub struct AtlasPage {
    /// Texture filename as written in the atlas (e.g. `"spineboy.png"`). The
    /// renderer resolves this relative to the atlas file's directory.
    pub name: String,
    pub width: i32,
    pub height: i32,
    pub format: Format,
    pub min_filter: TextureFilter,
    pub mag_filter: TextureFilter,
    pub u_wrap: TextureWrap,
    pub v_wrap: TextureWrap,
    /// Premultiplied-alpha flag. When true, texture pixels have been
    /// preprocessed so that `rgb *= a` — the renderer must use `(ONE,
    /// ONE_MINUS_SRC_ALPHA)` blending instead of the standard
    /// `(SRC_ALPHA, ONE_MINUS_SRC_ALPHA)`.
    pub pma: bool,
    /// Zero-based page index within the parent atlas.
    pub index: u32,
}

impl AtlasPage {
    fn new(name: String) -> Self {
        Self {
            name,
            width: 0,
            height: 0,
            format: Format::Unknown,
            min_filter: TextureFilter::Nearest,
            mag_filter: TextureFilter::Nearest,
            u_wrap: TextureWrap::ClampToEdge,
            v_wrap: TextureWrap::ClampToEdge,
            pma: false,
            index: 0,
        }
    }
}

/// One named sub-rectangle within an [`AtlasPage`]. UV coordinates are
/// precomputed relative to the parent page during parsing.
///
/// When `degrees == 90` (most common rotated case), the region's pixels are
/// laid out rotated 90° CCW on the page — the renderer must account for this
/// when sampling.
#[derive(Debug, Clone, PartialEq)]
pub struct AtlasRegion {
    /// Index into the parent [`Atlas::pages`] vector.
    pub page: u32,
    pub name: String,
    /// Sequence index (e.g. `foo` with `index: 2` for animated attachments).
    /// `-1` means "no index" (un-numbered region).
    pub index: i32,

    // Pixel rect on the page.
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,

    // Pre-crop dimensions of the source image (before the editor trimmed
    // transparent padding), plus the offset from the original top-left to
    // the trimmed rect.
    pub original_width: i32,
    pub original_height: i32,
    pub offset_x: f32,
    pub offset_y: f32,

    /// Rotation degrees: 0, 90, 180, or 270.
    pub degrees: i32,

    /// UV coordinates on the parent page, precomputed from `x/y/width/height`.
    pub u: f32,
    pub v: f32,
    pub u2: f32,
    pub v2: f32,

    /// Extension key/value pairs for any line that wasn't one of the
    /// well-known region fields. Values are parsed as decimal integers.
    pub extras: Vec<(String, Vec<i32>)>,
}

impl AtlasRegion {
    fn new(page: u32, name: String) -> Self {
        Self {
            page,
            name,
            index: -1,
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            original_width: 0,
            original_height: 0,
            offset_x: 0.0,
            offset_y: 0.0,
            degrees: 0,
            u: 0.0,
            v: 0.0,
            u2: 0.0,
            v2: 0.0,
            extras: Vec::new(),
        }
    }

    /// Lookup an extension key; returns its values if present. Linear scan —
    /// the list is always short, so this is fine.
    #[must_use]
    pub fn extra(&self, key: &str) -> Option<&[i32]> {
        self.extras
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_slice())
    }
}

/// Parsed contents of a `.atlas` file: a set of texture pages and the regions
/// that reference them.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Atlas {
    pub pages: Vec<AtlasPage>,
    pub regions: Vec<AtlasRegion>,
}

impl Atlas {
    /// Parse atlas text. Returns a populated [`Atlas`] or an error with
    /// 1-based line numbers for diagnostics.
    ///
    /// # Errors
    /// Returns [`AtlasError`] on malformed content (missing colons in a region
    /// body, unparseable integers, etc.). Unknown values in `format:` /
    /// `filter:` / `repeat:` entries degrade silently to `Unknown` /
    /// `ClampToEdge` rather than error — this matches spine-cpp, which was
    /// designed to tolerate atlases exported from older editor versions.
    pub fn parse(text: &str) -> Result<Self, AtlasError> {
        Parser::new(text).run()
    }

    /// Find the first region by name. Linear scan — callers that need
    /// repeated lookups should build their own hash map.
    #[must_use]
    pub fn find_region(&self, name: &str) -> Option<&AtlasRegion> {
        self.regions.iter().find(|r| r.name == name)
    }
}

/// Errors produced by [`Atlas::parse`]. Line numbers are 1-based.
///
/// The parser is permissive about structural issues that spine-cpp also
/// tolerates (unknown keys, garbage lines that happen to look like region
/// names, unrecognised enum values). These variants flag problems that make a
/// known property unusable.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum AtlasError {
    #[error("line {line}: failed to parse integer in {key:?}: {value:?}")]
    BadInteger {
        line: usize,
        key: String,
        value: String,
    },

    #[error("line {line}: expected {expected} values for {key:?}, got {got}")]
    WrongArity {
        line: usize,
        key: String,
        expected: usize,
        got: usize,
    },
}

// --- parser -----------------------------------------------------------------

/// At most 5 trimmed tokens parsed out of a `key: v1, v2, …, v4` line.
#[derive(Default)]
struct Entry<'a> {
    key: &'a str,
    values: [&'a str; 4],
    value_count: usize,
}

struct Parser<'a> {
    lines: Vec<&'a str>,
    cursor: usize,
    atlas: Atlas,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            // `str::lines()` handles both `\n` and `\r\n` line terminators and
            // omits a trailing empty line from a final newline — the exact
            // behaviour we want.
            lines: text.lines().collect(),
            cursor: 0,
            atlas: Atlas::default(),
        }
    }

    /// Trimmed view of the current line. Returns `None` past EOF.
    fn current(&self) -> Option<&'a str> {
        self.lines.get(self.cursor).map(|l| trim_line(l))
    }

    fn line_no(&self) -> usize {
        self.cursor + 1
    }

    fn advance(&mut self) {
        self.cursor += 1;
    }

    fn run(mut self) -> Result<Atlas, AtlasError> {
        self.skip_blank_lines();

        // Spine 4.1+ atlases may emit file-level entries before the first page
        // name (e.g. a hash header). Consume and discard any run of `key: ...`
        // lines at the top.
        while let Some(line) = self.current() {
            if line.is_empty() || parse_entry(line).is_none() {
                break;
            }
            self.advance();
        }

        while let Some(line) = self.current() {
            if line.is_empty() {
                // Blank lines terminate the current page's region block.
                self.advance();
                continue;
            }
            // Non-blank line at the top level → it's a page name. The page is
            // immediately followed by its property block, then its regions.
            self.parse_page()?;
        }

        Ok(self.atlas)
    }

    fn skip_blank_lines(&mut self) {
        while matches!(self.current(), Some(l) if l.is_empty()) {
            self.advance();
        }
    }

    fn parse_page(&mut self) -> Result<(), AtlasError> {
        let Some(name_line) = self.current() else {
            return Ok(());
        };
        let page_index = u32::try_from(self.atlas.pages.len()).expect("page count fits in u32");
        let mut page = AtlasPage::new(name_line.to_string());
        self.advance();

        // Page properties: a contiguous block of `key: v1, v2, …` lines.
        while let Some(line) = self.current() {
            if line.is_empty() {
                break;
            }
            let Some(entry) = parse_entry(line) else {
                // Non-entry line at property scope means the page block is
                // over and this line starts a region.
                break;
            };
            match entry.key {
                "size" => {
                    page.width = parse_int(&entry, 0, self.line_no())?;
                    page.height = parse_int(&entry, 1, self.line_no())?;
                }
                "format" => page.format = parse_format(entry.values[0]),
                "filter" => {
                    page.min_filter = parse_texture_filter(entry.values[0]);
                    page.mag_filter = parse_texture_filter(entry.values[1]);
                }
                "repeat" => {
                    // Default is ClampToEdge in both axes; presence of 'x' or
                    // 'y' in the value toggles Repeat on that axis.
                    page.u_wrap = TextureWrap::ClampToEdge;
                    page.v_wrap = TextureWrap::ClampToEdge;
                    if entry.values[0].contains('x') {
                        page.u_wrap = TextureWrap::Repeat;
                    }
                    if entry.values[0].contains('y') {
                        page.v_wrap = TextureWrap::Repeat;
                    }
                }
                "pma" => page.pma = entry.values[0].eq_ignore_ascii_case("true"),
                // Ignore unknown page-level keys (e.g. `scale:`, which is an
                // editor-only hint); matches spine-cpp behaviour.
                _ => {}
            }
            self.advance();
        }

        page.index = page_index;
        self.atlas.pages.push(page);

        // Regions of this page until the next blank line.
        while let Some(line) = self.current() {
            if line.is_empty() {
                break;
            }
            self.parse_region(page_index)?;
        }
        Ok(())
    }

    fn parse_region(&mut self, page_index: u32) -> Result<(), AtlasError> {
        let Some(name_line) = self.current() else {
            return Ok(());
        };
        let mut region = AtlasRegion::new(page_index, name_line.to_string());
        self.advance();

        while let Some(line) = self.current() {
            if line.is_empty() {
                break;
            }
            // A non-entry line (no colon) is the signal that this region's
            // property block is finished — the line is the start of the next
            // region (or, at EOF, nothing). spine-cpp handles this the same
            // way via `readEntry` returning 0.
            let Some(entry) = parse_entry(line) else {
                break;
            };

            match entry.key {
                "xy" => {
                    // Legacy pre-4.1 format: xy + size separately.
                    region.x = parse_int(&entry, 0, self.line_no())?;
                    region.y = parse_int(&entry, 1, self.line_no())?;
                }
                "size" => {
                    region.width = parse_int(&entry, 0, self.line_no())?;
                    region.height = parse_int(&entry, 1, self.line_no())?;
                }
                "bounds" => {
                    // 4.1+ compact form: bounds: x, y, w, h.
                    region.x = parse_int(&entry, 0, self.line_no())?;
                    region.y = parse_int(&entry, 1, self.line_no())?;
                    region.width = parse_int(&entry, 2, self.line_no())?;
                    region.height = parse_int(&entry, 3, self.line_no())?;
                }
                "offset" => {
                    // Legacy.
                    region.offset_x = parse_float_from_int(&entry, 0, self.line_no())?;
                    region.offset_y = parse_float_from_int(&entry, 1, self.line_no())?;
                }
                "orig" => {
                    region.original_width = parse_int(&entry, 0, self.line_no())?;
                    region.original_height = parse_int(&entry, 1, self.line_no())?;
                }
                "offsets" => {
                    // 4.1+ compact form: offsets: ox, oy, ow, oh.
                    region.offset_x = parse_float_from_int(&entry, 0, self.line_no())?;
                    region.offset_y = parse_float_from_int(&entry, 1, self.line_no())?;
                    region.original_width = parse_int(&entry, 2, self.line_no())?;
                    region.original_height = parse_int(&entry, 3, self.line_no())?;
                }
                "rotate" => {
                    // Three accepted forms: `true` → 90°, `false` → 0°, or
                    // an explicit integer.
                    let v = entry.values[0];
                    if v.eq_ignore_ascii_case("true") {
                        region.degrees = 90;
                    } else if !v.eq_ignore_ascii_case("false") {
                        region.degrees = parse_int(&entry, 0, self.line_no())?;
                    }
                }
                "index" => {
                    region.index = parse_int(&entry, 0, self.line_no())?;
                }
                other => {
                    // Any unrecognised key is captured verbatim so downstream
                    // extensions (9-slice, hull counts, whatever) can read it
                    // off the region.
                    let mut vals = Vec::with_capacity(entry.value_count);
                    for i in 0..entry.value_count {
                        vals.push(parse_int(&entry, i, self.line_no())?);
                    }
                    region.extras.push((other.to_string(), vals));
                }
            }
            self.advance();
        }

        // If the region had no explicit `orig`/`offsets` entry, fall back to
        // the trimmed size — matches spine-cpp.
        if region.original_width == 0 && region.original_height == 0 {
            region.original_width = region.width;
            region.original_height = region.height;
        }

        // Compute UVs. Rotated regions swap width/height when projecting onto
        // the page.
        let page = &self.atlas.pages[page_index as usize];
        let pw = page.width.max(1) as f32;
        let ph = page.height.max(1) as f32;
        region.u = region.x as f32 / pw;
        region.v = region.y as f32 / ph;
        if region.degrees == 90 {
            region.u2 = (region.x + region.height) as f32 / pw;
            region.v2 = (region.y + region.width) as f32 / ph;
        } else {
            region.u2 = (region.x + region.width) as f32 / pw;
            region.v2 = (region.y + region.height) as f32 / ph;
        }

        self.atlas.regions.push(region);
        Ok(())
    }
}

/// Trim leading/trailing whitespace and any trailing `\r` (Windows line endings).
fn trim_line(line: &str) -> &str {
    line.trim_matches(|c: char| c.is_whitespace())
}

/// Split `"key: v1, v2, v3, v4"` into key + up to 4 values, trimming each.
/// Returns `None` when the line has no `:` — the caller decides whether that's
/// end-of-block or an error.
fn parse_entry(line: &str) -> Option<Entry<'_>> {
    let colon = line.find(':')?;
    let (key, rest) = line.split_at(colon);
    let rest = &rest[1..]; // skip the ':'
    let mut entry = Entry {
        key: key.trim(),
        ..Entry::default()
    };

    // Up to 4 comma-separated values. Extra commas beyond that are silently
    // truncated, matching spine-cpp's behaviour (readEntry returns at most 4).
    let mut remaining = rest;
    for slot in &mut entry.values {
        if remaining.is_empty() {
            break;
        }
        if let Some(idx) = remaining.find(',') {
            *slot = remaining[..idx].trim();
            remaining = &remaining[idx + 1..];
            entry.value_count += 1;
        } else {
            *slot = remaining.trim();
            entry.value_count += 1;
            break;
        }
    }

    Some(entry)
}

fn parse_int(entry: &Entry, idx: usize, line: usize) -> Result<i32, AtlasError> {
    if idx >= entry.value_count {
        return Err(AtlasError::WrongArity {
            line,
            key: entry.key.to_string(),
            expected: idx + 1,
            got: entry.value_count,
        });
    }
    let raw = entry.values[idx];
    raw.parse::<i32>().map_err(|_| AtlasError::BadInteger {
        line,
        key: entry.key.to_string(),
        value: raw.to_string(),
    })
}

/// The editor writes `offset`/`offsets` values as integers but
/// `TextureRegion::offsetX/Y` is declared as `float` in spine-cpp. Parse as
/// integer then widen to f32 to match.
fn parse_float_from_int(entry: &Entry, idx: usize, line: usize) -> Result<f32, AtlasError> {
    parse_int(entry, idx, line).map(|v| v as f32)
}

fn parse_format(s: &str) -> Format {
    // Clean mapping by name. spine-cpp has an off-by-one bug here (its lookup
    // array aligns with the enum by accident only for some values), but the
    // field is only informational — no visible behaviour depends on matching
    // that bug.
    match s {
        "Alpha" => Format::Alpha,
        "Intensity" => Format::Intensity,
        "LuminanceAlpha" => Format::LuminanceAlpha,
        "RGB565" => Format::Rgb565,
        "RGBA4444" => Format::Rgba4444,
        "RGB888" => Format::Rgb888,
        "RGBA8888" => Format::Rgba8888,
        _ => Format::Unknown,
    }
}

fn parse_texture_filter(s: &str) -> TextureFilter {
    match s {
        "Nearest" => TextureFilter::Nearest,
        "Linear" => TextureFilter::Linear,
        "MipMap" => TextureFilter::MipMap,
        "MipMapNearestNearest" => TextureFilter::MipMapNearestNearest,
        "MipMapLinearNearest" => TextureFilter::MipMapLinearNearest,
        "MipMapNearestLinear" => TextureFilter::MipMapNearestLinear,
        "MipMapLinearLinear" => TextureFilter::MipMapLinearLinear,
        _ => TextureFilter::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    const SIMPLE: &str = "page.png
\tsize: 64, 32
\tfilter: Linear, Linear
region-a
\tbounds: 0, 0, 16, 16
region-b
\tbounds: 16, 0, 16, 16
\trotate: 90
";

    #[test]
    fn parses_simple_atlas() {
        let atlas = Atlas::parse(SIMPLE).unwrap();
        assert_eq!(atlas.pages.len(), 1);
        assert_eq!(atlas.regions.len(), 2);

        let page = &atlas.pages[0];
        assert_eq!(page.name, "page.png");
        assert_eq!(page.width, 64);
        assert_eq!(page.height, 32);
        assert_eq!(page.min_filter, TextureFilter::Linear);
        assert_eq!(page.mag_filter, TextureFilter::Linear);
        assert_eq!(page.u_wrap, TextureWrap::ClampToEdge);
        assert!(!page.pma);

        let a = &atlas.regions[0];
        assert_eq!(a.name, "region-a");
        assert_eq!(a.page, 0);
        assert_eq!((a.x, a.y, a.width, a.height), (0, 0, 16, 16));
        assert_abs_diff_eq!(a.u, 0.0);
        assert_abs_diff_eq!(a.v, 0.0);
        assert_abs_diff_eq!(a.u2, 16.0 / 64.0);
        assert_abs_diff_eq!(a.v2, 16.0 / 32.0);
        assert_eq!(a.degrees, 0);
        assert_eq!(a.original_width, 16); // falls back to trimmed size
        assert_eq!(a.original_height, 16);

        let b = &atlas.regions[1];
        assert_eq!(b.degrees, 90);
        // Rotated: u2/v2 swap width/height.
        assert_abs_diff_eq!(b.u2, (16 + 16) as f32 / 64.0);
        assert_abs_diff_eq!(b.v2, 16.0_f32 / 32.0);
    }

    #[test]
    fn handles_pma_and_repeat() {
        let text = "p.png
\tsize: 8, 8
\tpma: true
\trepeat: xy
r
\tbounds: 0, 0, 8, 8
";
        let atlas = Atlas::parse(text).unwrap();
        let page = &atlas.pages[0];
        assert!(page.pma);
        assert_eq!(page.u_wrap, TextureWrap::Repeat);
        assert_eq!(page.v_wrap, TextureWrap::Repeat);
    }

    #[test]
    fn handles_legacy_xy_size_offset_orig() {
        let text = "p.png
\tsize: 32, 32
r
\txy: 2, 3
\tsize: 5, 7
\toffset: 1, 2
\torig: 10, 12
";
        let atlas = Atlas::parse(text).unwrap();
        let r = &atlas.regions[0];
        assert_eq!((r.x, r.y), (2, 3));
        assert_eq!((r.width, r.height), (5, 7));
        assert_abs_diff_eq!(r.offset_x, 1.0);
        assert_abs_diff_eq!(r.offset_y, 2.0);
        assert_eq!(r.original_width, 10);
        assert_eq!(r.original_height, 12);
    }

    #[test]
    fn handles_rotate_true_false_and_degrees() {
        for (raw, expected) in [
            ("\trotate: true", 90),
            ("\trotate: false", 0),
            ("\trotate: 180", 180),
            ("\trotate: 270", 270),
        ] {
            let text = format!(
                "p.png
\tsize: 8, 8
r
\tbounds: 0, 0, 1, 1
{raw}
"
            );
            let atlas = Atlas::parse(&text).unwrap();
            assert_eq!(atlas.regions[0].degrees, expected, "raw={raw}");
        }
    }

    #[test]
    fn captures_unknown_keys_as_extras() {
        let text = "p.png
\tsize: 8, 8
r
\tbounds: 0, 0, 1, 1
\tsplits: 1, 2, 3, 4
\tpads: 5, 6, 7, 8
";
        let atlas = Atlas::parse(text).unwrap();
        let r = &atlas.regions[0];
        assert_eq!(r.extra("splits"), Some([1, 2, 3, 4].as_slice()));
        assert_eq!(r.extra("pads"), Some([5, 6, 7, 8].as_slice()));
        assert_eq!(r.extra("missing"), None);
    }

    #[test]
    fn parses_index_and_defaults_to_minus_one() {
        let text = "p.png
\tsize: 8, 8
r
\tbounds: 0, 0, 1, 1
\tindex: 3
r2
\tbounds: 0, 0, 1, 1
";
        let atlas = Atlas::parse(text).unwrap();
        assert_eq!(atlas.regions[0].index, 3);
        assert_eq!(atlas.regions[1].index, -1);
    }

    #[test]
    fn supports_multiple_pages_separated_by_blank_line() {
        let text = "p1.png
\tsize: 4, 4
r1
\tbounds: 0, 0, 1, 1

p2.png
\tsize: 8, 8
r2
\tbounds: 0, 0, 1, 1
";
        let atlas = Atlas::parse(text).unwrap();
        assert_eq!(atlas.pages.len(), 2);
        assert_eq!(atlas.regions.len(), 2);
        assert_eq!(atlas.regions[0].page, 0);
        assert_eq!(atlas.regions[1].page, 1);
        assert_eq!(atlas.pages[1].width, 8);
    }

    #[test]
    fn tolerates_crlf_line_endings() {
        let text = "p.png\r\n\tsize: 8, 8\r\nr\r\n\tbounds: 0, 0, 1, 1\r\n";
        let atlas = Atlas::parse(text).unwrap();
        assert_eq!(atlas.pages[0].width, 8);
        assert_eq!(atlas.regions[0].name, "r");
    }

    #[test]
    fn tolerates_unknown_format_and_filter_values() {
        let text = "p.png
\tsize: 8, 8
\tformat: HypotheticalFormat
\tfilter: Nearest, SomethingNew
r
\tbounds: 0, 0, 1, 1
";
        let atlas = Atlas::parse(text).unwrap();
        assert_eq!(atlas.pages[0].format, Format::Unknown);
        assert_eq!(atlas.pages[0].min_filter, TextureFilter::Nearest);
        assert_eq!(atlas.pages[0].mag_filter, TextureFilter::Unknown);
    }

    #[test]
    fn find_region_returns_first_match() {
        let atlas = Atlas::parse(SIMPLE).unwrap();
        assert!(atlas.find_region("region-a").is_some());
        assert!(atlas.find_region("missing").is_none());
    }

    #[test]
    fn non_entry_line_starts_a_new_region() {
        // A line without a colon inside a region body is interpreted as the
        // start of the next region, matching spine-cpp. So this parses as
        // two regions named "r" and "more" rather than as an error.
        let text = "p.png
\tsize: 8, 8
r
\tbounds: 0, 0, 1, 1
more
\tbounds: 2, 2, 1, 1
";
        let atlas = Atlas::parse(text).unwrap();
        let names: Vec<_> = atlas.regions.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["r", "more"]);
    }

    #[test]
    fn error_on_bad_integer() {
        let text = "p.png
\tsize: wide, 8
r
\tbounds: 0, 0, 1, 1
";
        let err = Atlas::parse(text).unwrap_err();
        assert!(matches!(err, AtlasError::BadInteger { .. }));
    }

    // Integration: every .atlas shipped with spine-runtimes parses without
    // error and has at least one page and region.
    #[test]
    fn parses_all_example_atlases() {
        let examples =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../spine-runtimes/examples");
        let mut parsed = 0;
        for entry in walk_atlas_files(&examples) {
            let text = std::fs::read_to_string(&entry)
                .unwrap_or_else(|e| panic!("read {}: {e}", entry.display()));
            let atlas =
                Atlas::parse(&text).unwrap_or_else(|e| panic!("parse {}: {e}", entry.display()));
            assert!(!atlas.pages.is_empty(), "{} has no pages", entry.display());
            assert!(
                !atlas.regions.is_empty(),
                "{} has no regions",
                entry.display()
            );
            parsed += 1;
        }
        // Sanity: we expect ~41 atlases in the examples dir. Hard-coding a
        // lower bound means the test still works if new skeletons get added.
        assert!(parsed >= 20, "only parsed {parsed} atlases");
    }

    fn walk_atlas_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return out;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(walk_atlas_files(&path));
            } else if path.extension().and_then(|s| s.to_str()) == Some("atlas") {
                out.push(path);
            }
        }
        out
    }
}
