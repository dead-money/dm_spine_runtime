// Trace the rear-foot-target through the pipeline to pinpoint the
// Transform-constraint divergence.

use std::sync::Arc;

use dm_spine_runtime::animation::{AnimationState, AnimationStateData};
use dm_spine_runtime::atlas::Atlas;
use dm_spine_runtime::data::{BoneId, SlotId};
use dm_spine_runtime::load::{AtlasAttachmentLoader, SkeletonBinary};
use dm_spine_runtime::skeleton::{Physics, Skeleton, UpdateCacheEntry};

fn main() {
    let atlas_src = std::fs::read_to_string(
        "../spine-runtimes/examples/spineboy/export/spineboy.atlas",
    )
    .unwrap();
    let atlas = Atlas::parse(&atlas_src).unwrap();
    let mut loader = AtlasAttachmentLoader::new(&atlas);
    let bytes =
        std::fs::read("../spine-runtimes/examples/spineboy/export/spineboy-pro.skel").unwrap();
    let data = Arc::new(SkeletonBinary::with_loader(&mut loader).read(&bytes).unwrap());

    // Find rear-foot-target + hoverboard-controller by name.
    let rft_idx = data.bones.iter().position(|b| b.name == "rear-foot-target").unwrap();
    let hbc_idx = data.bones.iter().position(|b| b.name == "hoverboard-controller").unwrap();
    println!("rear-foot-target bone index: {rft_idx}");
    println!("hoverboard-controller bone index: {hbc_idx}");

    // Find the transform constraint targeting rear-foot-target.
    for (i, tc) in data.transform_constraints.iter().enumerate() {
        if tc.bones.contains(&BoneId(rft_idx as u16)) {
            println!("transform constraint #{i}: {:?}", tc);
        }
    }

    let mut sk = Skeleton::new(Arc::clone(&data));
    sk.update_cache();
    sk.set_to_setup_pose();

    // Dump the full update cache to see what touches rear-foot-target.
    for (i, entry) in sk.update_cache.iter().enumerate() {
        match entry {
            UpdateCacheEntry::Bone(id) => {
                let name = &data.bones[id.0 as usize].name;
                if id.0 as usize == rft_idx || id.0 as usize == hbc_idx {
                    println!("cache[{i}]: Bone({name})");
                }
            }
            UpdateCacheEntry::TransformConstraint(id) => {
                let tc_idx = id.0 as usize;
                let tc = &data.transform_constraints[tc_idx];
                if tc.bones.contains(&BoneId(rft_idx as u16)) || tc.target == BoneId(rft_idx as u16) {
                    println!("cache[{i}]: TransformConstraint '{}' bones={:?} target={:?}", tc.name, tc.bones, tc.target);
                }
            }
            UpdateCacheEntry::IkConstraint(id) => {
                let ik = &data.ik_constraints[id.0 as usize];
                if ik.bones.contains(&BoneId(rft_idx as u16)) || ik.target == BoneId(rft_idx as u16) {
                    println!("cache[{i}]: IkConstraint '{}' bones={:?} target={:?}", ik.name, ik.bones, ik.target);
                }
            }
            UpdateCacheEntry::PathConstraint(id) => {
                let pc = &data.path_constraints[id.0 as usize];
                if pc.bones.contains(&BoneId(rft_idx as u16)) {
                    println!("cache[{i}]: PathConstraint '{}' bones={:?}", pc.name, pc.bones);
                }
            }
            UpdateCacheEntry::PhysicsConstraint(_) => {}
        }
    }

    // No animation — pure setup pose, matching golden_pose test.
    let _ = AnimationState::new(Arc::new(AnimationStateData::new(Arc::clone(&data))));

    // Pre-update_world_transform state.
    let b = &sk.bones[rft_idx];
    println!("\nSetup data x,y for rear-foot-target: {}, {}", data.bones[rft_idx].x, data.bones[rft_idx].y);
    println!("\nBEFORE update_world_transform:");
    println!("  rear-foot-target local: x={}, y={}, rotation={}, scale=({}, {})", b.x, b.y, b.rotation, b.scale_x, b.scale_y);
    println!("  rear-foot-target applied: ax={}, ay={}, a_rotation={}", b.ax, b.ay, b.a_rotation);
    println!("  rear-foot-target world: a={}, b={}, c={}, d={}, wx={}, wy={}", b.a, b.b, b.c, b.d, b.world_x, b.world_y);

    sk.update_world_transform(Physics::None);

    let b = &sk.bones[rft_idx];
    println!("\nAFTER update_world_transform:");
    println!("  rear-foot-target world: a={}, b={}, c={}, d={}, wx={}, wy={}", b.a, b.b, b.c, b.d, b.world_x, b.world_y);
    println!("  rear-foot-target applied: ax={}, ay={}, a_rotation={}, a_scale_y={}", b.ax, b.ay, b.a_rotation, b.a_scale_y);

    let hbc = &sk.bones[hbc_idx];
    println!("  hoverboard-controller world: a={}, b={}, c={}, d={}, wx={}, wy={}", hbc.a, hbc.b, hbc.c, hbc.d, hbc.world_x, hbc.world_y);

    // Expected from fixture.
    println!("\nEXPECTED (from spine-cpp fixture):");
    println!("  rear-foot-target.b = -0.00083336025  got {}", b.b);

    let _ = SlotId;
}
