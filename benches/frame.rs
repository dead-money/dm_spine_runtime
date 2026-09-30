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

//! Per-frame cost of `AnimationState` update + apply, world transform and
//! render, in the same JSON-line shape as `spine_capture --bench` so
//! `tools/spine_capture/bench_compare.sh` can pair the two runtimes.
//!
//! Rigs: a few `SPINE_EXAMPLES` rigs, plus hommlet's creature rigs under
//! `HOMMLET_SPINE_ASSETS` (its `Assets/Spine`) when set, each with a walk
//! cycle and a real skin. `SPINE_BENCH_FRAMES` overrides the frame count.
//! An atlas spec `a.atlas+b.atlas` concatenates pages, which is how hommlet
//! pairs a rig atlas with its body atlas.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use dm_spine_runtime::animation::{AnimationState, AnimationStateData};
use dm_spine_runtime::atlas::Atlas;
use dm_spine_runtime::load::{AtlasAttachmentLoader, SkeletonBinary};
use dm_spine_runtime::render::SkeletonRenderer;
use dm_spine_runtime::skeleton::{Physics, Skeleton};

fn examples_root() -> PathBuf {
    std::env::var_os("SPINE_EXAMPLES").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../spine-runtimes/examples"),
        PathBuf::from,
    )
}

struct Rig {
    atlas: String,
    skel: PathBuf,
    anim: &'static str,
    skin: Option<String>,
}

fn rigs() -> Vec<Rig> {
    let ex = examples_root();
    let example = |rig: &str, skel: &str, anim: &'static str| Rig {
        atlas: ex
            .join(format!("{rig}/export/{rig}.atlas"))
            .display()
            .to_string(),
        skel: ex.join(format!("{rig}/export/{skel}.skel")),
        anim,
        skin: None,
    };
    let mut out = vec![
        example("spineboy", "spineboy-pro", "run"),
        example("raptor", "raptor-pro", "walk"),
        example("diamond", "diamond-pro", "rotation"),
    ];
    if let Some(root) = std::env::var_os("HOMMLET_SPINE_ASSETS").map(PathBuf::from) {
        for name in ["Human", "Goblin", "Orc", "Ogre"] {
            let dir = root.join(name);
            out.push(Rig {
                atlas: format!(
                    "{}+{}",
                    dir.join(format!("{name}.atlas")).display(),
                    dir.join(format!("{name}_Body.atlas")).display()
                ),
                skel: dir.join(format!("{name}.skel")),
                anim: "Combat/Locomotion/WR_walkCombat_F",
                skin: Some(format!("{name}01")),
            });
        }
        out.push(Rig {
            atlas: root.join("Deer/Deer.atlas").display().to_string(),
            skel: root.join("Deer/Deer.skel"),
            anim: "Locomotion/walk_F",
            skin: Some("Doe".into()),
        });
    }
    out
}

fn read_atlas(spec: &str) -> Result<String, String> {
    let mut text = String::new();
    for path in spec.split('+') {
        text.push_str(&std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?);
        text.push('\n');
    }
    Ok(text)
}

fn bench_rig(rig: &Rig, frames: u32) -> Result<(), String> {
    let atlas_src = read_atlas(&rig.atlas)?;
    let atlas = Atlas::parse(&atlas_src).map_err(|e| e.to_string())?;
    let mut loader = AtlasAttachmentLoader::new(&atlas);
    let bytes = std::fs::read(&rig.skel).map_err(|e| e.to_string())?;
    let data = Arc::new(
        SkeletonBinary::with_loader(&mut loader)
            .read(&bytes)
            .map_err(|e| e.to_string())?,
    );
    let mut skeleton = Skeleton::new(Arc::clone(&data));
    if let Some(skin) = &rig.skin {
        skeleton.set_skin_by_name(skin).map_err(|e| e.to_string())?;
    }
    skeleton.setup_pose();
    let mut state = AnimationState::new(Arc::new(AnimationStateData::new(Arc::clone(&data))));
    state
        .set_animation_by_name(0, rig.anim, true)
        .map_err(|e| e.to_string())?;
    let mut renderer = SkeletonRenderer::new();
    let mut events = Vec::new();

    let dt = 1.0 / 60.0;
    let (mut anim_ns, mut world_ns, mut render_ns) = (0u128, 0u128, 0u128);
    let mut vertices = 0usize;
    for _ in 0..frames {
        let t0 = Instant::now();
        state.update(dt);
        events.clear();
        state.apply(&mut skeleton, &mut events);
        let t1 = Instant::now();
        skeleton.update_world_transform(Physics::None);
        let t2 = Instant::now();
        vertices += renderer
            .render(&skeleton)
            .iter()
            .map(dm_spine_runtime::render::RenderCommand::num_vertices)
            .sum::<usize>();
        let t3 = Instant::now();
        anim_ns += (t1 - t0).as_nanos();
        world_ns += (t2 - t1).as_nanos();
        render_ns += (t3 - t2).as_nanos();
    }
    let f = f64::from(frames.max(1));
    println!(
        "{{\"runtime\": \"dm_spine_runtime\", \"atlas\": {:?}, \"skel\": {:?}, \"animation\": {:?}, \"skin\": {:?}, \
         \"frames\": {frames}, \"anim_ns\": {:.1}, \"world_ns\": {:.1}, \"render_ns\": {:.1}, \"vertices\": {}}}",
        rig.atlas,
        rig.skel.display().to_string(),
        rig.anim,
        rig.skin.as_deref().unwrap_or(""),
        anim_ns as f64 / f,
        world_ns as f64 / f,
        render_ns as f64 / f,
        vertices / frames.max(1) as usize,
    );
    Ok(())
}

fn main() {
    // `cargo test --all-targets` runs this without `--bench`: one frame as a smoke test.
    let benching = std::env::args().any(|a| a == "--bench");
    let frames = if benching {
        std::env::var("SPINE_BENCH_FRAMES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(2000)
    } else {
        1
    };
    for rig in rigs() {
        if let Err(e) = bench_rig(&rig, frames) {
            eprintln!("skip {}: {e}", rig.skel.display());
        }
    }
}
