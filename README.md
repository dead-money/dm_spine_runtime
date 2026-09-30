# dm_spine_runtime

[![CI](https://github.com/dead-money/dm_spine_runtime/actions/workflows/ci.yml/badge.svg)](https://github.com/dead-money/dm_spine_runtime/actions/workflows/ci.yml)

A native Rust port of the [Spine](https://esotericsoftware.com/) 4.3 runtime. You load `.skel` or `.json` skeletons with their `.atlas`, pose them, play and mix animations, solve constraints, and get back draw commands your own renderer can consume.

The crate is renderer-agnostic. It's built for Dead Money's own game projects and was mostly written by AI agents under human direction, as a literal port of Esoteric Software's [spine-cpp](https://github.com/EsotericSoftware/spine-runtimes) reference runtime. For Bevy 0.18, see the sibling crate [`dm_spine_bevy`](https://github.com/dead-money/dm_spine_bevy).

## You need a Spine Editor license

This crate is a derivative of `spine-cpp`, translated to Rust with its source structure and copyright notices kept. Distribution is governed by Section 2 of the [Spine Editor License Agreement](https://esotericsoftware.com/spine-editor-license) and by the [Spine Runtimes License Agreement](https://esotericsoftware.com/spine-runtimes-license). That's the same obligation every official Spine runtime carries:

- **Every end user of software built with this crate needs their own [Spine Editor license](https://esotericsoftware.com/spine-purchase).**
- **Keep the notices.** Every ported source file carries Esoteric Software's copyright block, and `LICENSE` reproduces the Spine Runtimes License verbatim. Both travel with any redistribution.

The Spine editor is licensed separately. This runtime reads what the editor exports; it doesn't replace it. If your use case is in doubt, check the [Spine licensing page](https://esotericsoftware.com/spine-purchase) or ask Esoteric Software.

`main` targets **Spine 4.3** exports, binary or JSON. 4.3 changed both formats in most sections, so older exports won't parse. For Spine 4.2, use the `v0.1.0` tag. The rest of the upgrade, including the hooks hommlet needs, is tracked in [`docs/SPINE_4_3_UPGRADE.md`](docs/SPINE_4_3_UPGRADE.md).

## Quick start

```toml
[dependencies]
dm_spine_runtime = { git = "https://github.com/dead-money/dm_spine_runtime" }
```

The crate isn't on crates.io yet.

```rust
use std::sync::Arc;
use dm_spine_runtime::atlas::Atlas;
use dm_spine_runtime::load::{AtlasAttachmentLoader, SkeletonBinary, SkeletonJson};
use dm_spine_runtime::animation::{AnimationState, AnimationStateData};
use dm_spine_runtime::skeleton::{Physics, Skeleton};
use dm_spine_runtime::render::SkeletonRenderer;

// Parse the atlas and skeleton. Both loaders produce the same SkeletonData.
let atlas = Atlas::parse(&std::fs::read_to_string("spineboy.atlas")?)?;
let mut attachment_loader = AtlasAttachmentLoader::new(&atlas);

let bytes = std::fs::read("spineboy-pro.skel")?;
let data = Arc::new(SkeletonBinary::with_loader(&mut attachment_loader).read(&bytes)?);
// Or JSON:
// let json = std::fs::read("spineboy-pro.json")?;
// let data = Arc::new(SkeletonJson::with_loader(&mut attachment_loader).read_slice(&json)?);

// Skeletons and animation state share the immutable data.
let mut skeleton = Skeleton::new(Arc::clone(&data));
skeleton.update_world_transform(Physics::None);

let state_data = Arc::new(AnimationStateData::new(Arc::clone(&data)));
let mut animation = AnimationState::new(state_data);
animation.set_animation_by_name(0, "walk", true)?;

// Drive it from your frame loop.
let mut renderer = SkeletonRenderer::new();
let mut events = Vec::new();
for dt in frame_deltas {
    animation.update(dt);
    events.clear();
    animation.apply(&mut skeleton, &mut events);
    skeleton.update_world_transform(Physics::Update);
    let commands = renderer.render(&skeleton);
    // Upload `commands` to your renderer. dm_spine_bevy shows one way.
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

## What it does

- **Loaders.** Binary `.skel`, JSON `.json`, and the `.atlas` text format. All 43 example rigs in `spine-runtimes/examples/` load in either format, and the two loaders produce the same data.
- **Skeleton and animation state.** 4.3's pose system (setup, local, and constrained poses per bone and slot) with all five `Inherit` modes, and a multi-track `AnimationState` with crossfade mixing, additive tracks, hold modes, queuing, empty animations, and events.
- **Constraints.** IK (one- and two-bone, with bend, softness, and stretch), Transform (4.3's from/to property mapping), Path (every spacing and rotate mode), Physics (damped spring on a fixed timestep), and Slider (drives an animation from a bone property or a timeline).
- **Clipping and bounds.** `SkeletonClipping` (Sutherland-Hodgman plus convex decomposition, with 4.3's convex and inverse modes) and `SkeletonBounds` (AABB, point-in-polygon, segment-polygon hit tests).
- **Render commands.** `SkeletonRenderer::render` walks the draw order, emits region and mesh attachments through the clipper, and merges adjacent runs that share texture, blend mode, and color into one command.

## How it works

- **A literal port.** Files, functions, and update order follow `spine-cpp` closely enough to diff the two side by side. Math wasn't refactored on the way over; correctness is checked against dumps from `spine-cpp` itself (see [Testing](#testing)).
- **Struct-of-arrays with typed indices.** `Skeleton` owns flat `Vec<Bone>`, `Vec<Slot>`, and one ordered `Vec<Constraint>` enum. Cross-references are `BoneId(u16)`, `SlotId(u16)`, `ConstraintId(u16)`, and friends, not `Rc<RefCell<…>>`. The update cache is one `Vec` of an enum over bones and constraints, built with `spine-cpp`'s own sort.
- **Immutable shared data.** `SkeletonData` sits behind an `Arc`. Load an asset once and share it across every instance.
- **Tagged-enum timelines.** Timelines are a closed `enum`, not `Box<dyn Timeline>`, so the apply loop dispatches without a vtable.
- **No renderer types.** Each `RenderCommand` carries plain vertex, UV, color, and index buffers plus a `TextureId(u32)` (the atlas page index). Mapping that to a GPU handle is your side's job.
- **Events through an out-parameter.** `AnimationState::apply` pushes into a `&mut Vec<Event>` you own. There are no listener callbacks.

- **No per-frame allocation.** Once a skeleton has played through its animations, `AnimationState` update and apply, the world transform, and rendering reuse their buffers. `tests/alloc.rs` holds that at zero.

The crate has no GPU, windowing, or shader dependency, and it doesn't plan to grow one.

On hommlet's creature rigs, a full frame (animation update and apply, world transform, render) takes 0.51–0.63× the time `spine-cpp` 4.3 takes on the same rig and animation. `cargo bench --bench frame` and `tools/spine_capture/bench_compare.sh` measure it.

## Building

The tests and examples load the canonical rigs from a sibling clone of [`spine-runtimes`](https://github.com/EsotericSoftware/spine-runtimes), or from wherever `SPINE_EXAMPLES` points. The fixtures are captured from upstream `4.3` at the commit pinned in CI. For the 4.2 runtime, check out the `v0.1.0` tag with the `4.2` branch of `spine-runtimes`.

```sh
git clone -b 4.3 https://github.com/EsotericSoftware/spine-runtimes ../spine-runtimes
cargo test
```

Two examples ship with the crate:

- `cargo run --example software_render` rasterizes the `RenderCommand` stream on the CPU and writes a PNG. It's a reference consumer, and a quick way to tell whether a visual bug is in the runtime or in your renderer. Set `SPINE_RIG`, `SPINE_ANIM`, `SPINE_TIME`, and `SPINE_OUT` to pick what it draws (see the file header).
- `cargo run --example dump_slots` prints each drawable slot's attachment kind and world-space bounds.

## Testing

```sh
cargo test
cargo clippy --all-targets
cargo fmt --check
```

The golden tests diff against JSON captured from `spine-cpp` by the small C++ harness in [`tools/spine_capture/`](tools/spine_capture/). Current parity on the 4.3 example rigs:

- Setup-pose bone transforms match at 1e-4 on 43/43 rigs.
- Animation samples match at 1e-3 on 45/45 samples. One raptor-pro IK bone is allowed a documented drift, because IK softness runs through `acos` near 1 and amplifies float rounding.
- Scripted `AnimationState` scenarios (crossfades, queuing, additive tracks, empty animations, events) match on all 8.
- Render commands (texture, blend, color, vertex positions, and UVs) match on 43/43 rigs.

To regenerate fixtures, run `make` in the harness directory, then its `capture_*.sh` scripts. The harness, the fixtures, and the example exports have to be the same Spine version.

For the binary `.skel` wire format, including the tricks that aren't obvious from the reader (draw-order sign by u16 wraparound, the dual `Inherit` encoding, mesh triangle-count units, sequence path resolution), see [`docs/BINARY_FORMAT.md`](docs/BINARY_FORMAT.md).

## Licensing

Distributed under the [Spine Runtimes License Agreement](https://esotericsoftware.com/spine-runtimes-license). See [`LICENSE`](./LICENSE) for the full text.

Copyright © 2013-2025 Esoteric Software LLC. Rust port © Dead Money LLC, published under the same license.

## Acknowledgements

Built by porting [Esoteric Software](https://esotericsoftware.com/)'s C++ reference runtime. The upstream [spine-runtimes](https://github.com/EsotericSoftware/spine-runtimes) repository is the source of truth for runtime behavior. Report bugs in the runtime itself there; report bugs in this port here.
