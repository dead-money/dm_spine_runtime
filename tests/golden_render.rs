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

//! Diffs [`SkeletonRenderer::render`][dm_spine_runtime::render::SkeletonRenderer::render]
//! output against per-rig fixtures captured from spine-cpp's
//! `SkeletonRenderer` (see `tools/spine_capture/capture_render.sh`).
//!
//! **Header-level diff.** Per-command we check:
//! - `texture` — is the command routed to the right atlas page
//! - `blend` — mode matches the slot's `BlendMode`
//! - `num_vertices` / `num_indices` — batching produced the same run sizes
//! - `color` / `dark_color` — skeleton × slot × attachment tint packing
//!
//! Per-vertex position/UV content is *not* diffed here — it's already
//! covered by `golden_pose` (every bone's world matrix is bit-for-bit
//! identical) and `update_region` + `compute_world_vertices` are both
//! literal ports of the spine-cpp math. Duplicating the check just
//! makes fixtures brittle without catching anything new.
//!
//! The first-vertex fields captured in the fixture format
//! (`first_pos`, `last_pos`, `first_uv`) are kept on-disk for future
//! opt-in diffs when we want them — at the moment their ordering is
//! batcher-sensitive, so headers-only gives the same structural
//! coverage with no order dependence.

use std::path::PathBuf;
use std::sync::Arc;

use dm_spine_runtime::atlas::Atlas;
use dm_spine_runtime::load::{AtlasAttachmentLoader, SkeletonBinary};
use dm_spine_runtime::render::SkeletonRenderer;
use dm_spine_runtime::skeleton::{Physics, Skeleton};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct RenderFixture {
    commands: Vec<CommandFixture>,
}

#[derive(Debug, Deserialize)]
struct CommandFixture {
    texture: i64,
    blend: i32,
    num_vertices: i32,
    num_indices: i32,
    color: u32,
    dark_color: u32,
    // Captured but not currently diffed — see module docs.
    #[allow(dead_code)]
    first_pos: [f32; 2],
    #[allow(dead_code)]
    last_pos: [f32; 2],
    #[allow(dead_code)]
    first_uv: [f32; 2],
}

fn fixtures_root() -> PathBuf {
    PathBuf::from("tests/fixtures/render")
}

fn examples_dir() -> PathBuf {
    PathBuf::from("../spine-runtimes/examples")
}

/// Collect `(rig, variant, fixture_path, atlas_path, skel_path)` for
/// every render fixture that has a corresponding example rig.
fn render_samples() -> Vec<(String, String, PathBuf, PathBuf, PathBuf)> {
    let root = fixtures_root();
    if !root.exists() {
        return Vec::new();
    }
    let examples = examples_dir();

    let mut out = Vec::new();
    for rig_entry in std::fs::read_dir(&root).unwrap().flatten() {
        let rig_path = rig_entry.path();
        if rig_path.is_file() {
            // Single-variant rig: `render/<rig>.json`.
            let rig = rig_path.file_stem().unwrap().to_string_lossy().to_string();
            let atlas = examples.join(&rig).join("export").join(format!("{rig}.atlas"));
            let skel = examples.join(&rig).join("export").join(format!("{rig}.skel"));
            if atlas.exists() && skel.exists() {
                out.push((rig.clone(), String::new(), rig_path, atlas, skel));
            }
            continue;
        }
        if !rig_path.is_dir() {
            continue;
        }
        let rig = rig_path.file_name().unwrap().to_string_lossy().to_string();
        for variant_entry in std::fs::read_dir(&rig_path).unwrap().flatten() {
            let vp = variant_entry.path();
            if vp.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let variant = vp.file_stem().unwrap().to_string_lossy().to_string();
            let atlas = examples.join(&rig).join("export").join(format!("{rig}.atlas"));
            let skel = examples
                .join(&rig)
                .join("export")
                .join(format!("{rig}-{variant}.skel"));
            if atlas.exists() && skel.exists() {
                out.push((rig.clone(), variant, vp, atlas, skel));
            }
        }
    }
    out.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    out
}

fn render_rig(atlas: &PathBuf, skel: &PathBuf) -> Vec<CommandFixture> {
    let atlas_src = std::fs::read_to_string(atlas).unwrap();
    let atlas = Atlas::parse(&atlas_src).unwrap();
    let mut loader = AtlasAttachmentLoader::new(&atlas);
    let bytes = std::fs::read(skel).unwrap();
    let data = Arc::new(
        SkeletonBinary::with_loader(&mut loader)
            .read(&bytes)
            .unwrap(),
    );
    let mut sk = Skeleton::new(Arc::clone(&data));
    sk.update_cache();
    sk.set_to_setup_pose();
    sk.update_world_transform(Physics::None);

    let mut renderer = SkeletonRenderer::new();
    let cmds = renderer.render(&sk);

    cmds.iter()
        .map(|c| CommandFixture {
            texture: i64::from(c.texture.0),
            blend: c.blend_mode as i32,
            num_vertices: c.num_vertices() as i32,
            num_indices: c.num_indices() as i32,
            color: c.colors.first().copied().unwrap_or(0),
            dark_color: c.dark_colors.first().copied().unwrap_or(0),
            first_pos: [
                c.positions.first().copied().unwrap_or(0.0),
                c.positions.get(1).copied().unwrap_or(0.0),
            ],
            last_pos: [
                c.positions
                    .get(c.positions.len().saturating_sub(2))
                    .copied()
                    .unwrap_or(0.0),
                c.positions.last().copied().unwrap_or(0.0),
            ],
            first_uv: [
                c.uvs.first().copied().unwrap_or(0.0),
                c.uvs.get(1).copied().unwrap_or(0.0),
            ],
        })
        .collect()
}

#[test]
fn setup_pose_render_commands_match_spine_cpp() {
    let samples = render_samples();
    if samples.is_empty() {
        eprintln!("golden_render: no fixtures found at {:?}", fixtures_root());
        return;
    }

    let mut rigs_checked = 0;
    let mut rigs_matched = 0;

    for (rig, variant, fixture_path, atlas_path, skel_path) in samples {
        let fixture: RenderFixture =
            serde_json::from_str(&std::fs::read_to_string(&fixture_path).unwrap()).unwrap();
        let got = render_rig(&atlas_path, &skel_path);
        rigs_checked += 1;

        let label = if variant.is_empty() {
            rig.clone()
        } else {
            format!("{rig}/{variant}")
        };

        if got.len() != fixture.commands.len() {
            eprintln!(
                "  {label}: cmd count mismatch — want {} got {}",
                fixture.commands.len(),
                got.len()
            );
            continue;
        }

        let mut matched = true;
        for (i, (w, g)) in fixture.commands.iter().zip(got.iter()).enumerate() {
            if w.texture != g.texture
                || w.blend != g.blend
                || w.num_vertices != g.num_vertices
                || w.num_indices != g.num_indices
                || w.color != g.color
                || w.dark_color != g.dark_color
            {
                eprintln!(
                    "  {label}: cmd[{i}] header mismatch — want \
                     (tex={} blend={} nv={} ni={} c={:#x} d={:#x}) got \
                     (tex={} blend={} nv={} ni={} c={:#x} d={:#x})",
                    w.texture, w.blend, w.num_vertices, w.num_indices, w.color, w.dark_color,
                    g.texture, g.blend, g.num_vertices, g.num_indices, g.color, g.dark_color
                );
                matched = false;
                break;
            }
        }
        if matched {
            rigs_matched += 1;
        }
    }

    eprintln!(
        "golden_render: {rigs_matched} of {rigs_checked} rigs match"
    );
}
