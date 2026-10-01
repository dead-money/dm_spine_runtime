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

//! Every example `.json` export loads and matches its `.skel` export: bones,
//! slots, constraints and skins within the JSON's precision, and the same
//! timeline kinds and targets per animation.

mod common;

use std::path::{Path, PathBuf};

use spine_runtime::atlas::Atlas;
use spine_runtime::data::SkeletonData;
use spine_runtime::load::{AtlasAttachmentLoader, SkeletonBinary, SkeletonJson};

/// A skin's name and its `(slot, placeholder, attachment)` entries.
type SkinEntries = (String, Vec<(u16, String, String)>);

fn examples_root() -> PathBuf {
    common::examples_root()
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

/// alien-pro's death animation ends on a deform key, so its duration comes
/// from the deform timeline (one float per frame).
#[test]
fn duration_includes_last_deform_key() {
    let root = examples_root().join("alien/export");
    let atlas = root.join("alien.atlas");
    for sd in [
        load_json(&atlas, &root.join("alien-pro.json")),
        load_skel(&atlas, &root.join("alien-pro.skel")),
    ] {
        let death = sd.animations.iter().find(|a| a.name == "death").unwrap();
        assert!(
            (death.duration - 2.166_666_7).abs() < 1e-5,
            "{}",
            death.duration
        );
    }
}

#[test]
fn spineboy_pro_json_has_expected_structure() {
    let root = examples_root().join("spineboy/export");
    let sd = load_json(
        &root.join("spineboy.atlas"),
        &root.join("spineboy-pro.json"),
    );
    assert!(sd.version.starts_with("4.3"));
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

/// Splits Debug output into text and numbers.
fn tokens(v: &impl std::fmt::Debug) -> Vec<Result<f64, String>> {
    let s = format!("{v:?}");
    let mut out = Vec::new();
    let mut text = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        let starts_number =
            c.is_ascii_digit() || (c == '-' && chars.peek().is_some_and(char::is_ascii_digit));
        if starts_number && !text.ends_with(|t: char| t.is_ascii_alphanumeric() || t == '_') {
            let mut num = String::from(c);
            while let Some(&n) = chars.peek() {
                if n.is_ascii_digit() || n == '.' || n == 'e' || (n == '-' && num.ends_with('e')) {
                    num.push(n);
                    chars.next();
                } else {
                    break;
                }
            }
            out.push(Err(std::mem::take(&mut text)));
            out.push(Ok(num.parse().unwrap()));
        } else {
            text.push(c);
        }
    }
    out.push(Err(text));
    out
}

/// JSON exports carry about two decimals, so numbers match within 0.01 or
/// 0.1% and everything else exactly.
fn assert_close(a: &impl std::fmt::Debug, b: &impl std::fmt::Debug, label: &str) {
    let (ta, tb) = (tokens(a), tokens(b));
    let same = ta.len() == tb.len()
        && ta.iter().zip(&tb).all(|(x, y)| match (x, y) {
            (Ok(x), Ok(y)) => (x - y).abs() <= 0.01 + 1e-3 * x.abs().max(y.abs()),
            (Err(x), Err(y)) => x == y,
            _ => false,
        });
    assert!(same, "{label}\n  json: {a:?}\n  skel: {b:?}");
}

/// Order-independent summary of an animation's timelines: kind and target.
fn timeline_signature(sd: &SkeletonData, name: &str) -> Vec<String> {
    let anim = sd.animations.iter().find(|a| a.name == name).unwrap();
    let mut sig: Vec<String> = anim
        .timelines
        .iter()
        .map(|t| {
            let s = format!("{t:?}");
            let kind = s.split([' ', '{']).next().unwrap_or("").to_string();
            let target = s
                .split_once('{')
                .and_then(|(_, rest)| rest.split(',').next())
                .unwrap_or("")
                .trim();
            let target = if target.starts_with("frames") {
                ""
            } else {
                target
            };
            format!("{kind} {target}")
        })
        .collect();
    sig.sort();
    sig
}

#[test]
fn json_matches_binary_on_every_example() {
    let mut compared = 0;
    for pair in collect_jsons() {
        let skel_path = pair.json.with_extension("skel");
        if !skel_path.is_file() {
            continue;
        }
        let label = pair.json.display().to_string();
        let json = load_json(&pair.atlas, &pair.json);
        let skel = load_skel(&pair.atlas, &skel_path);

        for (j, b) in json.bones.iter().zip(&skel.bones) {
            assert_eq!(j.name, b.name, "{label}: bone order");
            assert_close(&j.setup, &b.setup, &format!("{label}: bone {}", j.name));
        }
        assert_eq!(json.bones.len(), skel.bones.len(), "{label}: bone count");
        let slots = |sd: &SkeletonData| {
            sd.slots
                .iter()
                .map(|s| {
                    (
                        s.name.clone(),
                        s.bone,
                        s.attachment_name.clone(),
                        s.blend_mode,
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(slots(&json), slots(&skel), "{label}: slots");

        // One-bone IK ignores bend direction, and the exports disagree on it.
        let constraints = |sd: &SkeletonData| {
            sd.constraints
                .iter()
                .cloned()
                .map(|mut c| {
                    match &mut c {
                        spine_runtime::data::ConstraintData::Ik(ik) if ik.bones.len() == 1 => {
                            ik.setup.bend_direction = 0;
                        }
                        // Nonessential; only the binary export carries it.
                        spine_runtime::data::ConstraintData::Slider(slider) => slider.max = 0.0,
                        _ => {}
                    }
                    c
                })
                .collect::<Vec<_>>()
        };
        assert_close(
            &constraints(&json),
            &constraints(&skel),
            &format!("{label}: constraints"),
        );

        let skins = |sd: &SkeletonData| {
            let mut v: Vec<SkinEntries> = sd
                .skins
                .iter()
                .map(|skin| {
                    let mut entries: Vec<_> = skin
                        .entries()
                        .map(|(key, attachment)| {
                            let att = skin.resolve(&sd.attachments, attachment);
                            (
                                sd.skin_keys.slot(key).0,
                                sd.skin_keys.placeholder(key).to_string(),
                                format!("{:?} {}", att.kind(), att.name()),
                            )
                        })
                        .collect();
                    entries.sort();
                    (skin.name.clone(), entries)
                })
                .collect();
            v.sort();
            v
        };
        assert_eq!(skins(&json), skins(&skel), "{label}: skins");

        let mut names: Vec<&str> = skel.animations.iter().map(|a| a.name.as_str()).collect();
        names.sort_unstable();
        let mut json_names: Vec<&str> = json.animations.iter().map(|a| a.name.as_str()).collect();
        json_names.sort_unstable();
        assert_eq!(names, json_names, "{label}: animation names");
        for name in names {
            assert_eq!(
                timeline_signature(&json, name),
                timeline_signature(&skel, name),
                "{label}: animation {name} timelines"
            );
        }
        compared += 1;
    }
    assert!(compared >= 20, "only compared {compared} JSON/binary pairs");
}

#[test]
fn rejects_other_versions() {
    let atlas = Atlas::default();
    let mut loader = AtlasAttachmentLoader::new(&atlas);
    let err = SkeletonJson::with_loader(&mut loader)
        .read_str(r#"{"skeleton":{"spine":"4.2.43"}}"#)
        .unwrap_err();
    assert!(matches!(
        err,
        spine_runtime::load::JsonError::UnsupportedVersion { .. }
    ));
}
