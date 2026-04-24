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

//! Integration tests: load every example skeleton's `.json` export and assert
//! basic structural invariants, then spot-check against the matching `.skel`
//! load to confirm the two formats produce compatible `SkeletonData`.

use std::path::{Path, PathBuf};

use dm_spine_runtime::atlas::Atlas;
use dm_spine_runtime::data::SkeletonData;
use dm_spine_runtime::load::{AtlasAttachmentLoader, SkeletonBinary, SkeletonJson};

fn examples_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../spine-runtimes/examples")
}

struct Pair {
    json: PathBuf,
    atlas: PathBuf,
}

fn collect_jsons() -> Vec<Pair> {
    let mut out = Vec::new();
    let root = examples_root();
    let Ok(entries) = std::fs::read_dir(&root) else {
        return out;
    };
    for entry in entries.flatten() {
        let export_dir = entry.path().join("export");
        if !export_dir.is_dir() {
            continue;
        }
        let mut jsons = Vec::new();
        let mut atlases = Vec::new();
        let Ok(files) = std::fs::read_dir(&export_dir) else {
            continue;
        };
        for f in files.flatten() {
            let p = f.path();
            match p.extension().and_then(|s| s.to_str()) {
                Some("json") => jsons.push(p),
                Some("atlas") => atlases.push(p),
                _ => {}
            }
        }
        for json in jsons {
            if let Some(atlas) = pick_atlas(&json, &atlases) {
                out.push(Pair { json, atlas });
            }
        }
    }
    out
}

fn pick_atlas(skel: &Path, atlases: &[PathBuf]) -> Option<PathBuf> {
    if atlases.is_empty() {
        return None;
    }
    let stem = skel.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let base = ["-pro", "-ess", "-ios"]
        .into_iter()
        .find_map(|sfx| stem.strip_suffix(sfx))
        .unwrap_or(stem);
    atlases
        .iter()
        .find(|a| a.file_stem().and_then(|s| s.to_str()) == Some(base))
        .or_else(|| {
            atlases.iter().find(|a| {
                !a.file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s.ends_with("-pma"))
            })
        })
        .or(atlases.first())
        .cloned()
}

fn load_json(atlas_path: &Path, json_path: &Path) -> SkeletonData {
    let atlas_text = std::fs::read_to_string(atlas_path)
        .unwrap_or_else(|e| panic!("read atlas {}: {e}", atlas_path.display()));
    let atlas = Atlas::parse(&atlas_text)
        .unwrap_or_else(|e| panic!("parse atlas {}: {e}", atlas_path.display()));
    let bytes = std::fs::read(json_path)
        .unwrap_or_else(|e| panic!("read json {}: {e}", json_path.display()));
    let mut loader = AtlasAttachmentLoader::new(&atlas);
    SkeletonJson::with_loader(&mut loader)
        .read_slice(&bytes)
        .unwrap_or_else(|e| panic!("parse json {}: {e}", json_path.display()))
}

fn load_skel(atlas_path: &Path, skel_path: &Path) -> SkeletonData {
    let atlas_text = std::fs::read_to_string(atlas_path).unwrap();
    let atlas = Atlas::parse(&atlas_text).unwrap();
    let bytes = std::fs::read(skel_path).unwrap();
    let mut loader = AtlasAttachmentLoader::new(&atlas);
    SkeletonBinary::with_loader(&mut loader)
        .read(&bytes)
        .unwrap()
}

#[test]
fn loads_every_example_skeleton_json() {
    let pairs = collect_jsons();
    assert!(
        pairs.len() >= 15,
        "expected >= 15 example JSON skeletons, found {}",
        pairs.len()
    );
    let mut loaded = 0usize;
    let mut failures: Vec<String> = Vec::new();
    for p in &pairs {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            load_json(&p.atlas, &p.json)
        }));
        match result {
            Ok(sd) => {
                if sd.bones.is_empty() || sd.slots.is_empty() || sd.animations.is_empty() {
                    failures.push(format!("{} empty sections", p.json.display()));
                } else {
                    loaded += 1;
                }
            }
            Err(_) => failures.push(format!("{} panicked during parse", p.json.display())),
        }
    }
    println!("Loaded {loaded} / {} json skeletons", pairs.len());
    assert!(
        failures.is_empty(),
        "failures:\n  {}",
        failures.join("\n  ")
    );
}

#[test]
fn spineboy_pro_json_has_expected_structure() {
    let root = examples_root().join("spineboy/export");
    let sd = load_json(
        &root.join("spineboy.atlas"),
        &root.join("spineboy-pro.json"),
    );
    assert!(sd.version.starts_with("4.2"));
    assert!(sd.bones.iter().any(|b| b.name == "root"));
    assert!(sd.bones.iter().any(|b| b.name == "hip"));
    assert!(sd.slots.iter().any(|s| s.name == "head"));
    assert!(sd.animations.iter().any(|a| a.name == "walk"));
    assert!(sd.animations.iter().any(|a| a.name == "run"));
    assert!(sd.animations.iter().any(|a| a.name == "jump"));

    assert!(sd.bones.len() > 50, "bones = {}", sd.bones.len());
    assert!(sd.slots.len() > 20, "slots = {}", sd.slots.len());
    assert!(sd.default_skin.is_some());

    for b in &sd.bones {
        if let Some(parent) = b.parent {
            assert!(
                parent.index() < b.index.index(),
                "bone {:?} parent {:?} is not earlier",
                b.name,
                parent
            );
        }
    }
}

#[test]
fn json_matches_binary_spineboy_pro_shape() {
    // Not a byte-for-byte parity check — the two formats can and do differ in
    // ordering details — but key counts should match.
    let root = examples_root().join("spineboy/export");
    let atlas = root.join("spineboy.atlas");
    let json = load_json(&atlas, &root.join("spineboy-pro.json"));
    let skel = load_skel(&atlas, &root.join("spineboy-pro.skel"));
    assert_eq!(json.bones.len(), skel.bones.len(), "bone count");
    assert_eq!(json.slots.len(), skel.slots.len(), "slot count");
    assert_eq!(json.events.len(), skel.events.len(), "event count");
    assert_eq!(
        json.animations.len(),
        skel.animations.len(),
        "animation count"
    );
    assert_eq!(
        json.ik_constraints.len(),
        skel.ik_constraints.len(),
        "ik count"
    );
    assert_eq!(
        json.transform_constraints.len(),
        skel.transform_constraints.len(),
        "transform count"
    );
    assert_eq!(
        json.path_constraints.len(),
        skel.path_constraints.len(),
        "path count"
    );
    assert_eq!(
        json.physics_constraints.len(),
        skel.physics_constraints.len(),
        "physics count"
    );
    // Skins + attachments are usually order-independent but should have
    // matching totals.
    assert_eq!(json.skins.len(), skel.skins.len(), "skin count");

    // Names in both formats must line up 1-for-1 on ordered collections where
    // the editor export preserves order (bones, slots, events). Skins and
    // animations live in `HashMap`-shaped JSON so order isn't guaranteed —
    // compare as sets instead.
    let json_bone_names: Vec<&str> = json.bones.iter().map(|b| b.name.as_str()).collect();
    let skel_bone_names: Vec<&str> = skel.bones.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(json_bone_names, skel_bone_names, "bone order");

    let json_slot_names: Vec<&str> = json.slots.iter().map(|s| s.name.as_str()).collect();
    let skel_slot_names: Vec<&str> = skel.slots.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(json_slot_names, skel_slot_names, "slot order");

    let mut json_anims: Vec<&str> = json.animations.iter().map(|a| a.name.as_str()).collect();
    let mut skel_anims: Vec<&str> = skel.animations.iter().map(|a| a.name.as_str()).collect();
    json_anims.sort_unstable();
    skel_anims.sort_unstable();
    assert_eq!(json_anims, skel_anims, "animation names");

    let mut json_skins: Vec<&str> = json.skins.iter().map(|s| s.name.as_str()).collect();
    let mut skel_skins: Vec<&str> = skel.skins.iter().map(|s| s.name.as_str()).collect();
    json_skins.sort_unstable();
    skel_skins.sort_unstable();
    assert_eq!(json_skins, skel_skins, "skin names");
}

#[test]
fn rejects_non_42_version() {
    let atlas = Atlas::default();
    let mut loader = AtlasAttachmentLoader::new(&atlas);
    let err = SkeletonJson::with_loader(&mut loader)
        .read_str(r#"{"skeleton":{"spine":"3.8.0"}}"#)
        .unwrap_err();
    assert!(matches!(
        err,
        dm_spine_runtime::load::JsonError::UnsupportedVersion { .. }
    ));
}
