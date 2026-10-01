use spine_runtime::atlas::Atlas;
use spine_runtime::data::ConstraintData;
use spine_runtime::load::{AtlasAttachmentLoader, SkeletonBinary};
use spine_runtime::skeleton::{Physics, Skeleton};
use std::sync::Arc;
fn main() {
    let dir = std::env::args().nth(1).unwrap();
    let rig = std::env::args().nth(2).unwrap();
    let anim = std::env::args().nth(3).unwrap();
    let mut atlas_text = std::fs::read_to_string(format!("{dir}/{rig}.atlas")).unwrap();
    if let Ok(b) = std::fs::read_to_string(format!("{dir}/{rig}_Body.atlas")) {
        atlas_text.push('\n');
        atlas_text.push_str(&b);
    }
    let atlas = Atlas::parse(&atlas_text).unwrap();
    let mut loader = AtlasAttachmentLoader::new(&atlas);
    let sd = Arc::new(
        SkeletonBinary::with_loader(&mut loader)
            .read(&std::fs::read(format!("{dir}/{rig}.skel")).unwrap())
            .unwrap(),
    );
    let mut kinds = std::collections::BTreeMap::new();
    for c in &sd.constraints {
        *kinds
            .entry(match c {
                ConstraintData::Ik(_) => "ik",
                ConstraintData::Transform(_) => "transform",
                ConstraintData::Path(_) => "path",
                ConstraintData::Physics(_) => "physics",
                ConstraintData::Slider(_) => "slider",
            })
            .or_insert(0) += 1;
    }
    println!(
        "bones {} slots {} constraints {:?} skins {} attachments {} animations {}",
        sd.bones.len(),
        sd.slots.len(),
        kinds,
        sd.skins.len(),
        sd.attachments.len(),
        sd.animations.len()
    );
    let a = sd.animations.iter().find(|a| a.name == anim).unwrap();
    let mut tk = std::collections::BTreeMap::new();
    for t in &a.timelines {
        let n = format!("{t:?}");
        *tk.entry(n.split([' ', '{']).next().unwrap().to_string())
            .or_insert(0) += 1;
    }
    println!("{anim}: {} timelines {:?}", a.timelines.len(), tk);
    let mut sk = Skeleton::new(Arc::clone(&sd));
    sk.set_skin_by_name(&format!("{rig}01")).ok();
    sk.update_world_transform(Physics::None);
    let mut state = spine_runtime::animation::AnimationState::new(Arc::new(
        spine_runtime::animation::AnimationStateData::new(Arc::clone(&sd)),
    ));
    state.set_animation_by_name(0, &anim, true).unwrap();
    state.update(0.3);
    state.apply(&mut sk, &mut Vec::new());
    sk.update_world_transform(Physics::None);
    let (mut active, mut live, mut bone_props) = (0, 0, 0);
    for (i, c) in sk.constraints.iter().enumerate() {
        if let (spine_runtime::skeleton::Constraint::Transform(t), ConstraintData::Transform(d)) =
            (c, &sd.constraints[i])
        {
            if !sk.constraints_active[i] {
                continue;
            }
            active += 1;
            let p = t.posed.applied();
            if p.mix_rotate != 0.0
                || p.mix_x != 0.0
                || p.mix_y != 0.0
                || p.mix_scale_x != 0.0
                || p.mix_scale_y != 0.0
                || p.mix_shear_y != 0.0
            {
                live += 1;
                bone_props +=
                    d.bones.len() * d.properties.iter().map(|f| f.to.len()).sum::<usize>();
            }
        }
    }
    println!(
        "transform: {active} active, {live} with nonzero mix, {bone_props} bone x to-property applications"
    );
    println!(
        "update cache {} entries, reset cache {}",
        sk.update_cache_entries().len(),
        sk.bones.iter().filter(|b| b.posed.is_constrained()).count()
    );
}
