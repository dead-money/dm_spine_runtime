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

//! Skins assembled at runtime from owned copies must pose and render exactly
//! like the skin they were copied from, on the example rigs and, with
//! `HOMMLET_SPINE_ASSETS`, on hommlet's creatures.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use spine_runtime::animation::{AnimationState, AnimationStateData};
use spine_runtime::atlas::Atlas;
use spine_runtime::data::{AttachmentRef, SkeletonData, Skin};
use spine_runtime::load::{AtlasAttachmentLoader, SkeletonBinary};
use spine_runtime::render::{RenderCommand, RenderOptions, SkeletonRenderer};
use spine_runtime::skeleton::{Physics, Skeleton};

fn load(atlases: &[PathBuf], skel: &Path) -> Arc<SkeletonData> {
    let mut text = String::new();
    for a in atlases {
        text.push_str(&std::fs::read_to_string(a).unwrap());
        text.push('\n');
    }
    let atlas = Atlas::parse(&text).unwrap();
    let mut loader = AtlasAttachmentLoader::new(&atlas);
    Arc::new(
        SkeletonBinary::with_loader(&mut loader)
            .read(&std::fs::read(skel).unwrap())
            .unwrap(),
    )
}

/// The template's entries as tagged owned copies, each pointed back at the
/// region it already used, the way hommlet binds body parts.
fn copied_skin(sd: &SkeletonData, template: &Skin) -> Skin {
    let mut skin = Skin::new("custom");
    skin.add_skin(template);
    for (i, (key, attachment)) in template.entries().enumerate() {
        let original = template.resolve(&sd.attachments, attachment);
        let mut copy = original.copy(attachment);
        if let Some(sequence) = original.sequence()
            && sequence.count() == 1
            && let Some(region) = sequence.region(0)
        {
            assert!(copy.set_region(*region));
        }
        copy.set_tag(i as u32 + 1);
        let owned = skin.add_owned(copy);
        skin.set(key, owned);
    }
    skin
}

fn check(label: &str, sd: &Arc<SkeletonData>, template: &str, animation: &str) {
    let template = sd
        .find_skin(template)
        .unwrap_or_else(|| panic!("{label}: no {template}"));
    let custom = Arc::new(copied_skin(sd, template));

    let pose = |skin: Arc<Skin>| {
        let mut sk = Skeleton::new(Arc::clone(sd));
        sk.set_skin(Some(skin));
        sk.setup_pose_slots();
        let mut state = AnimationState::new(Arc::new(AnimationStateData::new(Arc::clone(sd))));
        state.set_animation_by_name(0, animation, true).unwrap();
        (sk, state)
    };
    let (mut expected, mut expected_state) = pose(Arc::clone(template));
    let (mut actual, mut actual_state) = pose(Arc::clone(&custom));
    let (mut expected_renderer, mut actual_renderer) =
        (SkeletonRenderer::new(), SkeletonRenderer::new());
    let mut merged_renderer = SkeletonRenderer::with_options(RenderOptions {
        vertex_ids: true,
        merge_colors: true,
    });
    let (mut plain_commands, mut merged_commands) = (0, 0);
    let mut events = Vec::new();
    let mut owned_shown = 0;
    for frame in 0..90 {
        for (sk, state) in [
            (&mut expected, &mut expected_state),
            (&mut actual, &mut actual_state),
        ] {
            state.update(1.0 / 30.0);
            state.apply(sk, &mut events);
            sk.update(1.0 / 30.0);
            sk.update_world_transform(Physics::Update);
        }
        for slot in &actual.slots {
            if let Some(r @ AttachmentRef::Owned(_)) = slot.applied().attachment {
                owned_shown += 1;
                assert_ne!(actual.attachment(r).tag(), 0, "{label}: untagged copy");
            }
        }
        assert!(
            expected_renderer.render(&expected) == actual_renderer.render(&actual),
            "{label}: frame {frame} renders differently"
        );

        let plain = actual_renderer.commands();
        let merged = merged_renderer.render(&actual);
        let flat = |cmds: &[RenderCommand], f: fn(&RenderCommand) -> &[f32]| -> Vec<f32> {
            cmds.iter().flat_map(|c| f(c).iter().copied()).collect()
        };
        assert_eq!(
            flat(plain, |c| &c.positions),
            flat(merged, |c| &c.positions)
        );
        assert_eq!(flat(plain, |c| &c.uvs), flat(merged, |c| &c.uvs));
        for cmd in merged {
            assert_eq!(cmd.slots.len(), cmd.num_vertices());
            for (&slot, &tag) in cmd.slots.iter().zip(&cmd.tags) {
                let shown = actual.slots[usize::from(slot)]
                    .applied()
                    .attachment
                    .unwrap();
                assert_eq!(actual.attachment(shown).tag(), tag, "{label}: slot {slot}");
            }
        }
        plain_commands += plain.len();
        merged_commands += merged.len();
    }
    assert!(merged_commands <= plain_commands);
    assert!(owned_shown > 0, "{label}: no copies shown");
}

#[test]
fn copied_skins_render_like_their_templates() {
    // Goblins walk keys deform and diamond rotation keys sequences, so
    // copies must keep the timelines of the attachments they came from.
    for (rig, skel, skin, animation) in [
        (
            "mix-and-match",
            "mix-and-match-pro",
            "full-skins/girl",
            "dance",
        ),
        ("goblins", "goblins-pro", "goblin", "walk"),
        ("diamond", "diamond-pro", "default", "rotation"),
    ] {
        let dir = common::examples_root().join(format!("{rig}/export"));
        let sd = load(
            &[dir.join(format!("{rig}.atlas"))],
            &dir.join(format!("{skel}.skel")),
        );
        check(rig, &sd, skin, animation);
    }

    let Some(root) = std::env::var_os("HOMMLET_SPINE_ASSETS").map(PathBuf::from) else {
        return;
    };
    for rig in ["Human", "Goblin"] {
        let dir = root.join(rig);
        let sd = load(
            &[
                dir.join(format!("{rig}.atlas")),
                dir.join(format!("{rig}_Body.atlas")),
            ],
            &dir.join(format!("{rig}.skel")),
        );
        check(
            rig,
            &sd,
            &format!("{rig}01"),
            "Combat/Locomotion/WR_walkCombat_F",
        );
    }
}

#[test]
fn skin_changes_and_compaction_keep_slots_valid() {
    let dir = common::examples_root().join("mix-and-match/export");
    let sd = load(
        &[dir.join("mix-and-match.atlas")],
        &dir.join("mix-and-match-pro.skel"),
    );
    let template = sd.find_skin("full-skins/girl").unwrap();
    let mut sk = Skeleton::new(Arc::clone(&sd));
    sk.set_skin(Some(Arc::new(copied_skin(&sd, template))));
    sk.setup_pose_slots();
    let names = |sk: &Skeleton| -> Vec<Option<String>> {
        sk.slots
            .iter()
            .map(|s| {
                s.applied()
                    .attachment
                    .map(|a| sk.attachment(a).name().to_owned())
            })
            .collect()
    };
    let shown = names(&sk);
    assert!(shown.iter().any(Option::is_some));

    // Replacing every entry with a fresh copy strands the old copies; slots
    // still show them until compaction proves they survive it.
    let before = sk.skin().unwrap().owned().len();
    let entries: Vec<_> = sk.skin().unwrap().entries().collect();
    let skin = sk.skin_mut().unwrap();
    for (key, attachment) in entries {
        let copy = skin.resolve(&sd.attachments, attachment).copy(attachment);
        let owned = skin.add_owned(copy);
        skin.set(key, owned);
    }
    assert_eq!(skin.owned().len(), before * 2);
    sk.compact_skin();
    assert_eq!(names(&sk), shown);
    sk.setup_pose_slots();
    assert_eq!(names(&sk), shown);
    sk.compact_skin();
    assert_eq!(sk.skin().unwrap().owned().len(), before);

    // Swapping to a data skin moves slots onto its attachments; dropping the
    // skin clears what the old skin owned.
    sk.set_skin(Some(Arc::clone(template)));
    assert_eq!(names(&sk), shown);
    assert!(
        sk.slots
            .iter()
            .all(|s| !matches!(s.applied().attachment, Some(AttachmentRef::Owned(_))))
    );
    sk.set_skin(Some(Arc::new(copied_skin(&sd, template))));
    sk.set_skin(None);
    assert!(
        sk.slots
            .iter()
            .all(|s| !matches!(s.applied().attachment, Some(AttachmentRef::Owned(_))))
    );
}
