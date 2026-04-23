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

//! Integration tests: load every example skeleton shipped in
//! `~/deadmoney/spine-runtimes/examples/` and assert basic structural
//! invariants. Also spot-checks spineboy against known counts.

use std::path::{Path, PathBuf};

use dm_spine_runtime::atlas::Atlas;
use dm_spine_runtime::load::{AtlasAttachmentLoader, SkeletonBinary};

fn examples_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../spine-runtimes/examples")
}

/// Walk `examples/<name>/export/` directories and yield every .skel path
/// along with its sibling .atlas. Some rigs have multiple skels (ess/pro
/// variants); each pairs with the single shared .atlas for the rig.
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
        // Gather .skel and .atlas files in export/.
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
        // Pair each skel with an atlas. If there's only one atlas, every
        // skel shares it. If multiple, prefer a non-pma atlas whose stem
        // shares a prefix with the skel's stem.
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
    // Prefer a non-pma atlas with a matching prefix to the skel's stem.
    let skel_stem = skel.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    atlases
        .iter()
        .find(|a| {
            let stem = a.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            !stem.ends_with("-pma")
                && skel_stem
                    .split(['-', '_'])
                    .next()
                    .is_some_and(|p| stem.starts_with(p))
        })
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

fn load(atlas_path: &Path, skel_path: &Path) -> dm_spine_runtime::data::SkeletonData {
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

    // Spot-check values cross-referenced against the corresponding
    // .json export (which is much easier to read by eye).
    assert!(sd.version.starts_with("4.2"));
    assert!(sd.bones.iter().any(|b| b.name == "root"));
    assert!(sd.bones.iter().any(|b| b.name == "hip"));
    assert!(sd.slots.iter().any(|s| s.name == "head"));
    assert!(sd.animations.iter().any(|a| a.name == "walk"));
    assert!(sd.animations.iter().any(|a| a.name == "run"));
    assert!(sd.animations.iter().any(|a| a.name == "jump"));

    // Skeleton-wide invariants.
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

    // Bones are sorted parent-first: every non-root bone's parent index is
    // strictly less than its own.
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
fn rejects_non_42_version_bytes() {
    // Hand-craft a minimal prefix with version "3.8.0" → should bounce.
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&[0, 0, 0, 0]); // low hash
    bytes.extend_from_slice(&[0, 0, 0, 0]); // high hash
    // String length prefix: "3.8.0" is 5 bytes + 1 = 6 → varint 6 is a single byte.
    bytes.push(6);
    bytes.extend_from_slice(b"3.8.0");
    let atlas = Atlas::default();
    let mut loader = AtlasAttachmentLoader::new(&atlas);
    let err = SkeletonBinary::with_loader(&mut loader)
        .read(&bytes)
        .unwrap_err();
    assert!(
        matches!(
            err,
            dm_spine_runtime::load::BinaryError::UnsupportedVersion { .. }
        ),
        "unexpected error: {err}"
    );
}
