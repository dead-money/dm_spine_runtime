// Dump a bone's transform + its parent chain for a given animation sample.
// Intended for side-by-side comparison with a spine-cpp fixture.

use std::sync::Arc;

use dm_spine_runtime::animation::{AnimationState, AnimationStateData};
use dm_spine_runtime::atlas::Atlas;
use dm_spine_runtime::data::AnimationId;
use dm_spine_runtime::load::{AtlasAttachmentLoader, SkeletonBinary};
use dm_spine_runtime::skeleton::{Physics, Skeleton};

const SKEL_ATLAS: &str = "../spine-runtimes/examples/stretchyman/export/stretchyman.atlas";
const SKEL_FILE: &str = "../spine-runtimes/examples/stretchyman/export/stretchyman-pro.skel";
const ANIM: &str = "sneak";
const TIME: f32 = 0.449999988;
const BONE_NAME: &str = "back-leg1";

fn main() {
    let atlas_src = std::fs::read_to_string(SKEL_ATLAS).unwrap();
    let atlas = Atlas::parse(&atlas_src).unwrap();
    let mut loader = AtlasAttachmentLoader::new(&atlas);
    let bytes = std::fs::read(SKEL_FILE).unwrap();
    let data = Arc::new(SkeletonBinary::with_loader(&mut loader).read(&bytes).unwrap());

    let mut sk = Skeleton::new(Arc::clone(&data));
    sk.update_cache();
    sk.set_to_setup_pose();

    let state_data = Arc::new(AnimationStateData::new(Arc::clone(&data)));
    let mut state = AnimationState::new(state_data);
    let anim_id = data
        .animations
        .iter()
        .position(|a| a.name == ANIM)
        .map(|i| AnimationId(i as u16))
        .unwrap();
    state.set_animation(0, anim_id, false);
    state.update(TIME);
    let mut events = Vec::new();

    // Dump full update cache with bone entries that = back-shoulder highlighted.
    let bone_probe = data.bones.iter().position(|b| b.name == BONE_NAME).unwrap();
    use dm_spine_runtime::skeleton::UpdateCacheEntry;
    println!("=== full update cache (back-shoulder=#{bone_probe}) ===");
    for (i, entry) in sk.update_cache.iter().enumerate() {
        let marker = match entry {
            UpdateCacheEntry::Bone(id) => {
                if id.0 as usize == bone_probe {
                    format!("*** Bone({}) [back-shoulder] ***", data.bones[id.index()].name)
                } else {
                    format!("Bone({})", data.bones[id.index()].name)
                }
            }
            UpdateCacheEntry::TransformConstraint(id) => {
                let tc = &data.transform_constraints[id.index()];
                let hits = tc.bones.iter().any(|b| b.index() == bone_probe);
                let tag = if hits { " [touches back-shoulder]" } else { "" };
                format!("TC({}){tag}", tc.name)
            }
            UpdateCacheEntry::IkConstraint(id) => format!("IK({})", data.ik_constraints[id.index()].name),
            UpdateCacheEntry::PathConstraint(id) => format!("PC({})", data.path_constraints[id.index()].name),
            UpdateCacheEntry::PhysicsConstraint(id) => format!("Phys({})", data.physics_constraints[id.index()].name),
        };
        println!("  [{i}] {marker}");
    }

    // Dump ALL translate timelines in this animation with bone name.
    use dm_spine_runtime::data::Timeline as Tl;
    for (ti, t) in data.animations[anim_id.0 as usize].timelines.iter().enumerate() {
        match t {
            Tl::Translate { bone, curves } if data.bones[bone.index()].name == BONE_NAME => {
                println!("Timeline #{ti}: Translate on {BONE_NAME}: frames={:?}", curves.frames);
            }
            Tl::Rotate { bone, curves } if data.bones[bone.index()].name == BONE_NAME => {
                println!("Timeline #{ti}: Rotate on {BONE_NAME}: frames={:?}", curves.frames);
                println!("  curves={:?}", curves.curves);
            }
            _ => {}
        }
    }
    let snap = |sk: &Skeleton, tag: &str| {
        let b = &sk.bones[bone_probe];
        println!(
            "{tag}: x={:.4} rot={:.4} | ax={:.4} ar={:.7} | wa={:.7} wb={:.7} wc={:.7} wd={:.7}",
            b.x, b.rotation,
            b.ax, b.a_rotation,
            b.a, b.b, b.c, b.d,
        );
    };
    snap(&sk, "Pre-apply");
    state.apply(&mut sk, &mut events);
    snap(&sk, "Post-apply");
    sk.update_world_transform(Physics::None);
    snap(&sk, "Post-update");

    let bone_idx = data.bones.iter().position(|b| b.name == BONE_NAME).unwrap();
    let mut chain: Vec<usize> = Vec::new();
    let mut cur = Some(bone_idx);
    while let Some(ci) = cur {
        chain.push(ci);
        cur = data.bones[ci].parent.map(|p| p.index());
    }
    chain.reverse();

    println!("=== {ANIM} @ {TIME}s — chain ending at {BONE_NAME} ===\n");
    for idx in &chain {
        let b = &sk.bones[*idx];
        let name = &data.bones[*idx].name;
        println!("#{idx} {name} (inherit={:?})", b.inherit);
        println!("  world a={:.7} b={:.7} c={:.7} d={:.7}", b.a, b.b, b.c, b.d);
        println!("  world wx={:.7} wy={:.7}", b.world_x, b.world_y);
        println!(
            "  applied ax={:.7} ay={:.7} ar={:.7}",
            b.ax, b.ay, b.a_rotation
        );
        println!(
            "  applied asx={:.7} asy={:.7} ashx={:.7} ashy={:.7}",
            b.a_scale_x, b.a_scale_y, b.a_shear_x, b.a_shear_y
        );
    }

    // Dump animation timelines that affect this bone or its chain.
    use dm_spine_runtime::data::Timeline;
    let anim = &data.animations[anim_id.0 as usize];
    println!("\n=== {ANIM} timelines touching chain bones or any TC/IK ===");
    for t in &anim.timelines {
        let (kind, bone_opt, extra) = match t {
            Timeline::Rotate { bone, .. } => ("Rotate", Some(*bone), String::new()),
            Timeline::Translate { bone, .. } => ("Translate", Some(*bone), String::new()),
            Timeline::TranslateX { bone, .. } => ("TranslateX", Some(*bone), String::new()),
            Timeline::TranslateY { bone, .. } => ("TranslateY", Some(*bone), String::new()),
            Timeline::Scale { bone, .. } => ("Scale", Some(*bone), String::new()),
            Timeline::ScaleX { bone, .. } => ("ScaleX", Some(*bone), String::new()),
            Timeline::ScaleY { bone, .. } => ("ScaleY", Some(*bone), String::new()),
            Timeline::Shear { bone, .. } => ("Shear", Some(*bone), String::new()),
            Timeline::ShearX { bone, .. } => ("ShearX", Some(*bone), String::new()),
            Timeline::ShearY { bone, .. } => ("ShearY", Some(*bone), String::new()),
            Timeline::TransformConstraint { constraint, .. } => {
                let n = &data.transform_constraints[constraint.index()].name;
                println!("  TC timeline: {n}");
                continue;
            }
            Timeline::IkConstraint { constraint, .. } => {
                let n = &data.ik_constraints[constraint.index()].name;
                println!("  IK timeline: {n}");
                continue;
            }
            _ => continue,
        };
        if let Some(b) = bone_opt {
            if chain.contains(&b.index()) {
                println!("  {kind} timeline on {}{extra}", data.bones[b.index()].name);
            }
        }
    }

    // Dump any path constraint that touches a chain bone.
    println!("\n=== path constraints touching this chain ===");
    for (pi, pc) in sk.path_constraints.iter().enumerate() {
        let pd = &data.path_constraints[pc.data_index.index()];
        if pd.bones.iter().any(|b| chain.contains(&b.index())) {
            println!(
                "  PC#{pi} '{}' active={} mix: R={} X={} Y={} pos={} spac={} bones={:?}",
                pd.name, pc.active, pc.mix_rotate, pc.mix_x, pc.mix_y,
                pc.position, pc.spacing, pd.bones
            );
        }
    }

    // Also dump the active transform constraints that touch any bone in the chain.
    println!("\n=== transform constraints touching this chain ===");
    for (ci, tc) in sk.transform_constraints.iter().enumerate() {
        let target_in_chain = chain.contains(&tc.target.index());
        let bone_in_chain = tc.bones.iter().any(|b| chain.contains(&b.index()));
        if target_in_chain || bone_in_chain {
            let d = &data.transform_constraints[tc.data_index.index()];
            println!(
                "  TC#{ci} '{}' active={} local={} relative={}",
                d.name, tc.active, d.local, d.relative
            );
            println!(
                "    mix: R={} X={} Y={} SX={} SY={} ShY={}",
                tc.mix_rotate, tc.mix_x, tc.mix_y, tc.mix_scale_x, tc.mix_scale_y, tc.mix_shear_y
            );
            println!(
                "    target={} ({:?}) bones={:?}",
                data.bones[tc.target.index()].name, tc.target, tc.bones
            );
        }
    }
}
