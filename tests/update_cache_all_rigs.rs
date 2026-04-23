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

//! Smoke-test `Skeleton::update_cache` against every example skeleton that
//! Phase 1 knows how to load. This doesn't verify ordering correctness
//! (that's Phase 2d's job, comparing computed bone world transforms
//! against the capture fixtures) — it catches panics and asserts simple
//! invariants like "every active bone appears at least once in the cache".

use std::path::PathBuf;
use std::sync::Arc;

use dm_spine_runtime::atlas::Atlas;
use dm_spine_runtime::load::{AtlasAttachmentLoader, SkeletonBinary};
use dm_spine_runtime::skeleton::{Skeleton, UpdateCacheEntry};

fn examples_dir() -> PathBuf {
    // `cargo test` runs with CWD set to the crate root.
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
fn update_cache_runs_on_every_example_rig() {
    let skels = rig_skel_paths();
    assert!(
        !skels.is_empty(),
        "no example .skel files found at {:?}",
        examples_dir()
    );

    let mut exercised = 0usize;
    for skel_path in skels {
        // Find the matching non-PMA atlas. Skip if none (spinosaurus).
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
        let atlas = Atlas::parse(&atlas_src)
            .unwrap_or_else(|e| panic!("atlas parse failed for {atlas_path:?}: {e}"));
        let mut loader = AtlasAttachmentLoader::new(&atlas);

        let bytes = std::fs::read(&skel_path).unwrap();
        let data = SkeletonBinary::with_loader(&mut loader)
            .read(&bytes)
            .unwrap_or_else(|e| panic!("skeleton load failed for {skel_path:?}: {e}"));
        let data = Arc::new(data);

        let mut sk = Skeleton::new(Arc::clone(&data));
        sk.update_cache();

        // Every active bone appears at least once. (A bone may appear more
        // than once — see sort_reset commentary in skeleton.rs — but each
        // active bone must be in the cache.)
        for (bone_idx, bone) in sk.bones.iter().enumerate() {
            if !bone.active {
                continue;
            }
            let entry = UpdateCacheEntry::Bone(bone.data_index);
            assert!(
                sk.update_cache.contains(&entry),
                "rig {rig}: active bone #{bone_idx} ({:?}) missing from cache",
                data.bones[bone_idx].name
            );
        }

        exercised += 1;
    }

    assert!(
        exercised >= 20,
        "only exercised {exercised} rigs — expected most of 25"
    );
}
