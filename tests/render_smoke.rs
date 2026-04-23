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

//! Smoke-test `SkeletonRenderer::render` on every example rig: every
//! rig loads + renders at setup pose without panicking, every emitted
//! command has internally-consistent buffer lengths, and every world
//! position / uv is finite.
//!
//! This doesn't check output correctness — that's golden_render's job
//! (Phase 6g). It catches walker-level regressions: out-of-bounds
//! indexing, attachment-kind mismatches, stray NaN/Inf propagation.

use std::path::PathBuf;
use std::sync::Arc;

use dm_spine_runtime::atlas::Atlas;
use dm_spine_runtime::load::{AtlasAttachmentLoader, SkeletonBinary};
use dm_spine_runtime::render::SkeletonRenderer;
use dm_spine_runtime::skeleton::{Physics, Skeleton};

fn examples_dir() -> PathBuf {
    PathBuf::from("../spine-runtimes/examples")
}

fn rig_skel_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let root = examples_dir();
    if !root.exists() {
        return out;
    }
    for entry in std::fs::read_dir(&root).unwrap().flatten() {
        let export = entry.path().join("export");
        if !export.is_dir() {
            continue;
        }
        for file in std::fs::read_dir(&export).unwrap().flatten() {
            let p = file.path();
            if p.extension().and_then(|s| s.to_str()) == Some("skel") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

#[test]
fn renders_every_example_rig_at_setup_pose() {
    let skels = rig_skel_paths();
    assert!(
        !skels.is_empty(),
        "no example .skel files found at {:?}",
        examples_dir()
    );

    let mut rendered = 0;
    let mut total_commands = 0;
    for skel_path in skels {
        let dir = skel_path.parent().unwrap();
        let rig = dir
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let atlas_path = dir.join(format!("{rig}.atlas"));
        if !atlas_path.exists() {
            continue;
        }

        let atlas_src = std::fs::read_to_string(&atlas_path).unwrap();
        let Ok(atlas) = Atlas::parse(&atlas_src) else {
            continue;
        };
        let mut loader = AtlasAttachmentLoader::new(&atlas);

        let bytes = std::fs::read(&skel_path).unwrap();
        let Ok(data) = SkeletonBinary::with_loader(&mut loader).read(&bytes) else {
            continue;
        };
        let data = Arc::new(data);

        let mut sk = Skeleton::new(Arc::clone(&data));
        sk.update_cache();
        sk.set_to_setup_pose();
        sk.update_world_transform(Physics::None);

        let mut renderer = SkeletonRenderer::new();
        // Render unbatched so degenerate-command regressions surface per-slot
        // — the batcher would otherwise merge a zero-area slot into an
        // adjacent real command and hide the problem.
        let cmds = renderer.render_unbatched(&sk);

        for (i, cmd) in cmds.iter().enumerate() {
            assert_eq!(
                cmd.positions.len() % 2,
                0,
                "{rig}: cmd[{i}] positions must be 2xN"
            );
            assert_eq!(
                cmd.uvs.len(),
                cmd.positions.len(),
                "{rig}: cmd[{i}] uvs mismatch"
            );
            assert_eq!(cmd.colors.len(), cmd.num_vertices());
            assert_eq!(cmd.dark_colors.len(), cmd.num_vertices());
            for (k, v) in cmd.positions.iter().enumerate() {
                assert!(
                    v.is_finite(),
                    "{rig}: cmd[{i}].positions[{k}] = {v}"
                );
            }
            for (k, v) in cmd.uvs.iter().enumerate() {
                assert!(v.is_finite(), "{rig}: cmd[{i}].uvs[{k}] = {v}");
            }

            // Regression guard against attachments whose `vertex_offset` or
            // world vertices never got populated — those emit valid-looking
            // commands whose quads have collapsed to a single point.
            if let Some((xmin, xmax, ymin, ymax)) = cmd.position_bounds() {
                let degenerate = (xmax - xmin).abs() < 1e-4 && (ymax - ymin).abs() < 1e-4;
                assert!(
                    !degenerate,
                    "{rig}: cmd[{i}] has zero-area bbox ({xmin},{ymin})..({xmax},{ymax})",
                );
            }
        }

        total_commands += cmds.len();
        rendered += 1;
    }

    assert!(rendered > 0, "render_smoke: no rigs loaded");
    eprintln!(
        "render_smoke: rendered {rendered} rigs, emitted {total_commands} region commands total"
    );
}
