// Spine Runtimes License Agreement
// Last updated April 5, 2025. Replaces all prior versions.
//
// Copyright (c) 2013-2025, Esoteric Software LLC
//
// See LICENSE for full terms.

//! Walk a skeleton's draw order and print each drawable slot's attachment
//! kind + name + world-space bounds. Used to investigate per-slot
//! regressions.
//!
//! ```
//! SPINE_RIG=dragon SPINE_SKEL=dragon-ess SPINE_ATLAS=dragon-pma \
//!     cargo run --example dump_slots
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use dm_spine_runtime::atlas::Atlas;
use dm_spine_runtime::data::{Attachment, BoneData};
use dm_spine_runtime::load::{AtlasAttachmentLoader, SkeletonBinary};
use dm_spine_runtime::render::SkeletonRenderer;
use dm_spine_runtime::skeleton::{Physics, Skeleton};

fn env_str(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn main() {
    let rig = env_str("SPINE_RIG", "spineboy");
    let skel = env_str("SPINE_SKEL", "spineboy-pro");
    let atlas_stem = env_str("SPINE_ATLAS", "spineboy-pma");

    let dir: PathBuf = ["..", "spine-runtimes", "examples", &rig, "export"]
        .iter()
        .collect();
    let atlas_src = std::fs::read_to_string(dir.join(format!("{atlas_stem}.atlas"))).unwrap();
    let atlas = Atlas::parse(&atlas_src).unwrap();
    let mut loader = AtlasAttachmentLoader::new(&atlas);
    let bytes = std::fs::read(dir.join(format!("{skel}.skel"))).unwrap();
    let data = SkeletonBinary::with_loader(&mut loader)
        .read(&bytes)
        .unwrap();
    let data = Arc::new(data);

    let mut sk = Skeleton::new(Arc::clone(&data));
    sk.update_cache();
    sk.set_to_setup_pose();
    sk.update_world_transform(Physics::None);

    // Build a parallel list of (drawable slot name, attachment kind) to
    // label the render commands.
    let mut labels: Vec<(String, &'static str, String)> = Vec::new();
    for &slot_id in &sk.draw_order {
        let slot = &sk.slots[slot_id.index()];
        let slot_data = &data.slots[slot_id.index()];
        let _bone_data: &BoneData = &data.bones[slot_data.bone.index()];
        let bone_active = sk.bones[slot_data.bone.index()].active;
        let Some(att_id) = slot.attachment else {
            continue;
        };
        if slot.color.a == 0.0 || !bone_active {
            continue;
        }
        let att = &data.attachments[att_id.index()];
        let (kind, name) = match att {
            Attachment::Region(r) => ("region", r.name.clone()),
            Attachment::Mesh(m) => ("mesh", m.name.clone()),
            Attachment::Clipping(c) => ("clipping", c.name.clone()),
            Attachment::BoundingBox(b) => ("bbox", b.name.clone()),
            Attachment::Path(p) => ("path", p.name.clone()),
            Attachment::Point(p) => ("point", p.name.clone()),
        };
        if matches!(kind, "region" | "mesh") {
            labels.push((slot_data.name.clone(), kind, name));
        }
    }

    let mut renderer = SkeletonRenderer::new();
    let cmds = renderer.render_unbatched(&sk);
    println!("rig={rig} skel={skel}: {} drawable commands", cmds.len());
    for (i, c) in cmds.iter().enumerate() {
        let (xmin, xmax, ymin, ymax) = c.position_bounds().unwrap_or((0.0, 0.0, 0.0, 0.0));
        let (slot_name, kind, att_name) = labels
            .get(i)
            .cloned()
            .unwrap_or_else(|| ("?".to_string(), "?", "?".to_string()));
        let degenerate = (xmax - xmin).abs() < 1e-4 && (ymax - ymin).abs() < 1e-4;
        println!(
            "  cmd[{i:2}] {flag} verts={:3} tris={:3} x=[{xmin:7.1}..{xmax:7.1}] y=[{ymin:7.1}..{ymax:7.1}] slot={slot_name} ({kind}:{att_name})",
            c.num_vertices(),
            c.indices.len() / 3,
            flag = if degenerate { "!!" } else { "  " },
        );
    }
}
