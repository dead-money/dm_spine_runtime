# spine_runtime

[![CI](https://github.com/dead-money/spine_runtime/actions/workflows/ci.yml/badge.svg)](https://github.com/dead-money/spine_runtime/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/spine_runtime.svg)](https://crates.io/crates/spine_runtime)
[![docs.rs](https://docs.rs/spine_runtime/badge.svg)](https://docs.rs/spine_runtime)

An unofficial Rust port of the [Spine](https://esotericsoftware.com/) 4.3 runtime. You load skeletons exported from the Spine editor, play and blend their animations, and get back triangles your own renderer can draw.

The crate doesn't draw anything itself, so it works with any engine. It's built for Dead Money's own game projects and was mostly written by AI agents under human direction, as a close translation of Esoteric Software's official C++ runtime. For Bevy, see [`spine_bevy`](https://github.com/dead-money/spine_bevy).

## You need a Spine Editor license

This crate is a translation of Esoteric Software's [`spine-cpp`](https://github.com/EsotericSoftware/spine-runtimes), under the [Spine Runtimes License Agreement](https://esotericsoftware.com/spine-runtimes-license). Under Section 2 of the [Spine Editor License Agreement](https://esotericsoftware.com/spine-editor-license):

- **Every developer who builds software with this crate needs their own [Spine Editor license](https://esotericsoftware.com/spine-purchase),** including to build and run the examples. Players of a game you ship don't need one.
- **Ship the license text.** Include the Spine Runtimes License Agreement in the documentation or other materials that come with your product. If you redistribute this crate's source, keep the copyright header in each file and the `LICENSE` file.

If you're unsure whether your use is covered, ask Esoteric Software.

This release reads **Spine 4.3** exports, binary (`.skel`) or JSON, each paired with its `.atlas`. 4.2 exports won't load; re-export them from a 4.3 editor, or use the `v0.1.0` tag, the last 4.2 version.

## Quick start

```toml
[dependencies]
spine_runtime = "0.2"
```

It needs Rust 1.99 or newer.

```rust
use std::sync::Arc;
use spine_runtime::atlas::Atlas;
use spine_runtime::load::{AtlasAttachmentLoader, SkeletonBinary};
use spine_runtime::animation::{AnimationState, AnimationStateData};
use spine_runtime::skeleton::{Physics, Skeleton};
use spine_runtime::render::SkeletonRenderer;

// Load the atlas and skeleton once. For JSON exports, use SkeletonJson.
let atlas = Atlas::parse(&std::fs::read_to_string("spineboy.atlas")?)?;
let mut loader = AtlasAttachmentLoader::new(&atlas);
let bytes = std::fs::read("spineboy-pro.skel")?;
let data = Arc::new(SkeletonBinary::with_loader(&mut loader).read(&bytes)?);

// Any number of skeletons can share the loaded data.
let mut skeleton = Skeleton::new(Arc::clone(&data));
let mut animation = AnimationState::new(Arc::new(AnimationStateData::new(Arc::clone(&data))));
animation.set_animation_by_name(0, "walk", true)?;

let mut renderer = SkeletonRenderer::new();
let mut events = Vec::new();
for dt in frame_deltas {
    animation.update(dt);
    events.clear();
    animation.apply(&mut skeleton, &mut events);
    skeleton.update(dt); // advances physics
    skeleton.update_world_transform(Physics::Update);
    for command in renderer.render(&skeleton) {
        // Draw command.positions / uvs / colors / indices with
        // command.texture and command.blend_mode.
    }
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Blending and queuing animations

```rust
// Crossfade times between animations, in seconds.
let mut mixes = AnimationStateData::new(Arc::clone(&data));
mixes.set_default_mix(0.1);
mixes.set_mix_by_name("walk", "jump", 0.2)?;
let mut animation = AnimationState::new(Arc::new(mixes));

animation.set_animation_by_name(0, "walk", true)?;
animation.add_animation_by_name(0, "jump", false, 2.0)?; // after 2 s of walking
animation.add_animation_by_name(0, "run", true, 0.0)?;   // when the jump ends

// A second track plays on top of the first, e.g. aiming while running.
animation.set_animation_by_name(1, "aim", true)?;
```

Events keyed in the editor (footsteps, hit frames) land in the `events` buffer you pass to `apply`:

```rust
for event in &events {
    let name = &data.events[event.data.index()].name;
    if name == "footstep" {
        // play a sound at event.volume
    }
}
```

### Skins

Switch to a skin from the editor, or build one by combining several, the way character customization usually works:

```rust
use spine_runtime::data::Skin;

skeleton.set_skin_by_name("full-skins/girl")?;

let mut outfit = Skin::new("outfit");
for name in ["skin-base", "clothes/dress-blue", "accessories/hat-red-yellow"] {
    outfit.add_skin(data.find_skin(name).expect("skin exists"));
}
skeleton.set_skin(Some(Arc::new(outfit)));
skeleton.setup_pose_slots();
```

Skeletons can share one `Arc<Skin>`, so a crowd wearing the same outfit builds it once.

### Drawing

Each `RenderCommand` is one draw call: vertex positions, UVs, packed colors, 16-bit indices, a blend mode, and a `TextureId`. The `TextureId` is the index of the atlas page, so load `atlas.pages[id]` as a texture and look it up by that index. Consecutive attachments that share a texture and blend mode come back merged into one command.

`examples/software_render.rs` draws the commands on the CPU and writes a PNG. It's the shortest complete renderer, and a quick way to tell whether a visual bug is in the runtime or in your renderer.

### Hit testing

Bounding-box attachments drawn in the editor work as hit areas:

```rust
use spine_runtime::skeleton::SkeletonBounds;

let mut bounds = SkeletonBounds::new();
bounds.update(&skeleton, true);
if bounds.contains_point(mouse_x, mouse_y).is_some() {
    // the click landed on the skeleton
}
```

## What's supported

Everything the 4.3 editor exports: bones, slots, meshes and weights, all animation and timeline types, IK, transform, path, physics, and slider constraints, clipping, sequences, and events. The example rigs from Esoteric's repository load and animate the same as in `spine-cpp`, checked automatically against output from `spine-cpp` itself.

It's also fast. On our game's creature rigs, a full frame (animation, posing, and building draw commands) takes about 0.5–0.6× the time `spine-cpp` needs, and once animations have played through, frames don't allocate memory.

## Versions

| Spine | spine_runtime |
|-------|---------------|
| 4.3   | `main`        |
| 4.2   | `v0.1.0`      |

## Building

The tests and examples use the example rigs from Esoteric's [`spine-runtimes`](https://github.com/EsotericSoftware/spine-runtimes) repository, cloned next to this one (or wherever `SPINE_EXAMPLES` points):

```sh
git clone -b 4.3 https://github.com/EsotericSoftware/spine-runtimes ../spine-runtimes
cargo test
```

CI pins a specific `4.3` commit; see `.github/workflows/ci.yml`.

Three examples come with the crate:

- `cargo run --example software_render` draws a rig to a PNG. `SPINE_RIG`, `SPINE_ANIM`, `SPINE_TIME`, and `SPINE_OUT` pick what it draws; the file header lists the rest.
- `cargo run --example dump_slots` prints each visible slot's attachment and on-screen bounds.
- `cargo run --example rig_info -- <dir> <rig> <animation>` counts a binary rig's bones, constraints, and an animation's timelines.

Contributors: [`docs/BINARY_FORMAT.md`](docs/BINARY_FORMAT.md) covers the `.skel` format, and `tools/spine_capture/` regenerates the comparison data from `spine-cpp`.

## Licensing

Distributed under the [Spine Runtimes License Agreement](https://esotericsoftware.com/spine-runtimes-license). See [`LICENSE`](./LICENSE) for the full text.

Copyright © 2013-2025 Esoteric Software LLC. Rust port © Dead Money LLC, published under the same license.

## Acknowledgements

Built by porting [Esoteric Software](https://esotericsoftware.com/)'s C++ runtime. The upstream [spine-runtimes](https://github.com/EsotericSoftware/spine-runtimes) repository is the source of truth for runtime behavior. Report bugs in the runtime itself there; report bugs in this port here.
