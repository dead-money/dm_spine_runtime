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

//! Every example `.skel` under `SPINE_EXAMPLES` loads with non-empty bones,
//! slots and animations; spineboy is spot-checked in more detail.

mod common;

use std::path::{Path, PathBuf};

use spine_runtime::atlas::Atlas;
use spine_runtime::load::{AtlasAttachmentLoader, SkeletonBinary};

fn examples_root() -> PathBuf {
    common::examples_root()
}

/// Every `examples/<rig>/export/*.skel` paired with an atlas from the same
/// directory (see [`pick_atlas`]). Rigs without an atlas are left out.
fn collect_skels() -> Vec<(PathBuf, PathBuf)> {
    let mut out = Vec::new();
    let root = examples_root();
    let Ok(entries) = std::fs::read_dir(&root) else {
        return out;
    };
    for entry in entries.flatten() {
        let rig = entry.path();
        let export_dir = rig.join("export");
        if !export_dir.is_dir() {
            continue;
        }
        let mut skels = Vec::new();
        let mut atlases = Vec::new();
        let Ok(exp_entries) = std::fs::read_dir(&export_dir) else {
            continue;
        };
        for f in exp_entries.flatten() {
            let p = f.path();
            match p.extension().and_then(|s| s.to_str()) {
                Some("skel") => skels.push(p),
                Some("atlas") => atlases.push(p),
                _ => {}
            }
        }
        for skel in skels {
            let atlas = pick_atlas(&skel, &atlases);
            if let Some(atlas) = atlas {
                out.push((skel, atlas));
            }
        }
    }
    out
}

fn pick_atlas(skel: &Path, atlases: &[PathBuf]) -> Option<PathBuf> {
    if atlases.is_empty() {
        return None;
    }
    let skel_stem = skel.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    // `spineboy-pro.skel` pairs with `spineboy.atlas`.
    let base = ["-pro", "-ess", "-ios"]
        .into_iter()
        .find_map(|sfx| skel_stem.strip_suffix(sfx))
        .unwrap_or(skel_stem);
    // Exact `<base>.atlas` first: some rigs also ship subset atlases such as
    // `<base>-run.atlas`. Then any non-PMA atlas, then anything.
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

fn load(atlas_path: &Path, skel_path: &Path) -> spine_runtime::data::SkeletonData {
    let atlas_text = std::fs::read_to_string(atlas_path)
        .unwrap_or_else(|e| panic!("read atlas {}: {e}", atlas_path.display()));
    let atlas = Atlas::parse(&atlas_text)
        .unwrap_or_else(|e| panic!("parse atlas {}: {e}", atlas_path.display()));
    let bytes = std::fs::read(skel_path)
        .unwrap_or_else(|e| panic!("read skel {}: {e}", skel_path.display()));
    let mut loader = AtlasAttachmentLoader::new(&atlas);
    SkeletonBinary::with_loader(&mut loader)
        .read(&bytes)
        .unwrap_or_else(|e| panic!("parse skel {}: {e}", skel_path.display()))
}

#[test]
fn loads_every_example_skeleton() {
    let pairs = collect_skels();
    assert!(
        pairs.len() >= 15,
        "expected >= 15 example skeletons, found {}",
        pairs.len()
    );
    let mut loaded = 0usize;
    let mut failures: Vec<String> = Vec::new();
    for (skel, atlas) in &pairs {
        let atlas_text = match std::fs::read_to_string(atlas) {
            Ok(t) => t,
            Err(e) => {
                failures.push(format!("read atlas {}: {e}", atlas.display()));
                continue;
            }
        };
        let parsed_atlas = match Atlas::parse(&atlas_text) {
            Ok(a) => a,
            Err(e) => {
                failures.push(format!("parse atlas {}: {e}", atlas.display()));
                continue;
            }
        };
        let bytes = match std::fs::read(skel) {
            Ok(b) => b,
            Err(e) => {
                failures.push(format!("read skel {}: {e}", skel.display()));
                continue;
            }
        };
        let mut loader = AtlasAttachmentLoader::new(&parsed_atlas);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            SkeletonBinary::with_loader(&mut loader).read(&bytes)
        }));
        match result {
            Ok(Ok(sd)) => {
                if sd.bones.is_empty() || sd.slots.is_empty() || sd.animations.is_empty() {
                    failures.push(format!("{} empty sections", skel.display()));
                } else {
                    loaded += 1;
                }
            }
            Ok(Err(e)) => failures.push(format!("{}: {e}", skel.display())),
            Err(_) => failures.push(format!("{} panicked during parse", skel.display())),
        }
    }
    println!("Loaded {loaded} / {} skeletons", pairs.len());
    assert!(
        failures.is_empty(),
        "failures:\n  {}",
        failures.join("\n  ")
    );
}

#[test]
fn spineboy_pro_has_expected_structure() {
    let root = examples_root().join("spineboy/export");
    let sd = load(
        &root.join("spineboy.atlas"),
        &root.join("spineboy-pro.skel"),
    );

    assert!(sd.version.starts_with("4.3"));
    assert!(sd.bones.iter().any(|b| b.name == "root"));
    assert!(sd.bones.iter().any(|b| b.name == "hip"));
    assert!(sd.slots.iter().any(|s| s.name == "head"));
    assert!(sd.animations.iter().any(|a| a.name == "walk"));
    assert!(sd.animations.iter().any(|a| a.name == "run"));
    assert!(sd.animations.iter().any(|a| a.name == "jump"));

    assert!(
        sd.bones.len() > 50,
        "spineboy has >50 bones, got {}",
        sd.bones.len()
    );
    assert!(
        sd.slots.len() > 20,
        "spineboy has >20 slots, got {}",
        sd.slots.len()
    );
    assert!(sd.default_skin.is_some(), "spineboy has a default skin");

    // Bones are stored parent-first.
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
fn rejects_other_versions() {
    // A 4.2 header must bounce.
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&[0, 0, 0, 0]); // low hash
    bytes.extend_from_slice(&[0, 0, 0, 0]); // high hash
    bytes.push(7);
    bytes.extend_from_slice(b"4.2.43");
    let atlas = Atlas::default();
    let mut loader = AtlasAttachmentLoader::new(&atlas);
    let err = SkeletonBinary::with_loader(&mut loader)
        .read(&bytes)
        .unwrap_err();
    assert!(
        matches!(
            err,
            spine_runtime::load::BinaryError::UnsupportedVersion { .. }
        ),
        "unexpected error: {err}"
    );
}

/// hommlet's creature rigs (`HOMMLET_SPINE_ASSETS` = its `Assets/Spine`),
/// each with its rig atlas plus body atlas when present. Skipped when unset.
#[test]
fn loads_hommlet_rigs() {
    let Some(root) = std::env::var_os("HOMMLET_SPINE_ASSETS").map(PathBuf::from) else {
        return;
    };
    let mut loaded = 0;
    for rig in ["Human", "Goblin", "Orc", "Ogre", "Deer", "Stele"] {
        let dir = root.join(rig);
        let mut atlas_text = std::fs::read_to_string(dir.join(format!("{rig}.atlas"))).unwrap();
        if let Ok(body) = std::fs::read_to_string(dir.join(format!("{rig}_Body.atlas"))) {
            atlas_text.push('\n');
            atlas_text.push_str(&body);
        }
        let atlas = Atlas::parse(&atlas_text).unwrap();
        let bytes = std::fs::read(dir.join(format!("{rig}.skel"))).unwrap();
        let mut loader = AtlasAttachmentLoader::new(&atlas);
        let sd = SkeletonBinary::with_loader(&mut loader)
            .read(&bytes)
            .unwrap_or_else(|e| panic!("{rig}: {e}"));
        assert!(!sd.bones.is_empty(), "{rig}: no bones");
        loaded += 1;
    }
    assert_eq!(loaded, 6);
}
