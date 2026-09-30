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

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use dm_spine_runtime::atlas::Atlas;
use dm_spine_runtime::load::{AtlasAttachmentLoader, SkeletonBinary};
use dm_spine_runtime::render::SkeletonRenderer;
use dm_spine_runtime::skeleton::{Physics, Skeleton};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct RenderFixture {
    source_skel: String,
    source_atlas: String,
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
    first_pos: [f32; 2],
    last_pos: [f32; 2],
    first_uv: [f32; 2],
}

fn fixtures_root() -> PathBuf {
    PathBuf::from("tests/fixtures/render")
}

/// `(label, fixture_path, atlas_path, skel_path)` for every render fixture.
fn render_samples() -> Vec<(String, PathBuf, PathBuf, PathBuf)> {
    let root = fixtures_root();
    common::json_files(&root)
        .into_iter()
        .map(|p| {
            let fx: RenderFixture =
                serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
            let label = p
                .strip_prefix(&root)
                .unwrap()
                .with_extension("")
                .display()
                .to_string();
            let atlas = common::example_path(&fx.source_atlas);
            let skel = common::example_path(&fx.source_skel);
            (label, p, atlas, skel)
        })
        .collect()
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

    sk.setup_pose();
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
    assert!(!samples.is_empty(), "no fixtures at {:?}", fixtures_root());

    let mut rigs_checked = 0;
    let mut rigs_matched = 0;

    for (label, fixture_path, atlas_path, skel_path) in samples {
        let fixture: RenderFixture =
            serde_json::from_str(&std::fs::read_to_string(&fixture_path).unwrap()).unwrap();
        let got = render_rig(&atlas_path, &skel_path);
        rigs_checked += 1;

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
                    w.texture,
                    w.blend,
                    w.num_vertices,
                    w.num_indices,
                    w.color,
                    w.dark_color,
                    g.texture,
                    g.blend,
                    g.num_vertices,
                    g.num_indices,
                    g.color,
                    g.dark_color
                );
                matched = false;
                break;
            }
            // The capture prints 6 significant digits.
            let near = |a: [f32; 2], b: [f32; 2]| {
                a.iter()
                    .zip(&b)
                    .all(|(x, y)| (x - y).abs() <= 1e-3 * x.abs().max(1.0))
            };
            if !near(w.first_pos, g.first_pos)
                || !near(w.last_pos, g.last_pos)
                || !near(w.first_uv, g.first_uv)
            {
                eprintln!(
                    "  {label}: cmd[{i}] geometry mismatch — want pos {:?}..{:?} uv {:?}, got pos {:?}..{:?} uv {:?}",
                    w.first_pos, w.last_pos, w.first_uv, g.first_pos, g.last_pos, g.first_uv
                );
                matched = false;
                break;
            }
        }
        if matched {
            rigs_matched += 1;
        }
    }

    eprintln!("golden_render: {rigs_matched} of {rigs_checked} rigs match");
    assert!(rigs_checked > 0, "no render fixtures");
    assert_eq!(rigs_matched, rigs_checked, "every rig must match");
}
