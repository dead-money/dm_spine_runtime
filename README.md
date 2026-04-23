# dm_spine_runtime

A native Rust port of the [Spine](https://esotericsoftware.com/) 4.2 runtime. Renderer-agnostic — loads `.skel` + `.atlas` files, poses skeletons, plays animations, solves constraints, and emits draw commands that any graphics backend can consume.

For Bevy 0.18 integration, see the sibling crate [`dm_spine_bevy`](https://github.com/dead-money/dm_spine_bevy).

> **About this project.** This crate is built for Dead Money's internal game projects and was primarily authored by AI agents (Claude Code) driving a literal port of the upstream [spine-cpp](https://github.com/EsotericSoftware/spine-runtimes) reference runtime, with a human engineer directing scope, reviewing output, and steering architecture. It's published for transparency and for use inside Dead Money, not as a polished third-party library. Interfaces will shift, not everything is battle-tested, and documentation leans toward "what would a maintainer need?" rather than "what would a brand-new user expect?". If you adopt it anyway, expect to file issues and read source occasionally.

## You need a Spine Editor license to use this

This is a derivative work of Esoteric Software's `spine-cpp` reference runtime, translated to Rust while preserving source structure and copyright notices. Distribution is governed by Section 2 of the [Spine Editor License Agreement](https://esotericsoftware.com/spine-editor-license) and the [Spine Runtimes License Agreement](https://esotericsoftware.com/spine-runtimes-license). In practical terms:

- **Every end user of software built with this crate must hold their own [Spine Editor license](https://esotericsoftware.com/spine-purchase).** Same obligation as every official Spine runtime.
- **Copyright and license notices must be preserved.** Every ported source file carries the Esoteric Software copyright block; the `LICENSE` file reproduces the Spine Runtimes License verbatim and must travel with any redistribution.
- **The Spine editor is separately licensed.** This runtime processes data exported by the Spine editor but is not a substitute for it.

If your use case is in doubt, consult the [Spine licensing page](https://esotericsoftware.com/spine-purchase) or contact Esoteric Software directly.

## What's in the box

- **Loaders** — binary `.skel` reader and `.atlas` parser. All 25 rigs that ship under `spine-runtimes/examples/` load cleanly through `AtlasAttachmentLoader`.
- **Skeleton + animation state** — full pose pipeline, multi-track `AnimationState` with crossfade mixing, event queue, empty animations, and all five `Inherit` modes.
- **Constraint solvers** — IK (1-bone + 2-bone with bend/softness/stretch), Transform (absolute/relative × world/local), Path (all spacing + rotate modes), and Physics (damped spring, fixed timestep).
- **Clipping + bounds** — `SkeletonClipping` (Sutherland-Hodgman + convex decomposition) and `SkeletonBounds` (AABB, point-in-polygon, segment-polygon hit tests).
- **Render-command emission** — `SkeletonRenderer::render` walks the draw order, handles `RegionAttachment` / `MeshAttachment`, runs the clipper, and merges adjacent same-(texture, blend, color) runs into one batched command.

## What's explicitly out of scope

- **GPU work.** The core crate has no GPU, windowing, or shader dependency. Draw commands carry plain `Vec<f32>` / `Vec<u16>` buffers and a `TextureId(u32)` (atlas page index); integration crates map that onto their backend.
- **JSON skeletons.** Only the binary `.skel` format is supported. Add a JSON loader if you need one — the data types are public.
- **Spine versions before 4.2.** The binary format introduced new fields and physics constraints in 4.2. Older exports won't parse.

## Quick start

```toml
[dependencies]
dm_spine_runtime = { git = "https://github.com/dead-money/dm_spine_runtime" }
```

```rust
use std::sync::Arc;
use dm_spine_runtime::atlas::Atlas;
use dm_spine_runtime::load::{AtlasAttachmentLoader, SkeletonBinary};
use dm_spine_runtime::animation::{AnimationState, AnimationStateData};
use dm_spine_runtime::skeleton::{Physics, Skeleton};
use dm_spine_runtime::render::SkeletonRenderer;

// 1. Parse the atlas (text) and skeleton (binary).
let atlas_src = std::fs::read_to_string("spineboy.atlas")?;
let atlas = Atlas::parse(&atlas_src)?;
let mut attachment_loader = AtlasAttachmentLoader::new(&atlas);

let bytes = std::fs::read("spineboy-pro.skel")?;
let data = Arc::new(SkeletonBinary::with_loader(&mut attachment_loader).read(&bytes)?);

// 2. Build a skeleton + animation state sharing the immutable data.
let mut skeleton = Skeleton::new(Arc::clone(&data));
skeleton.update_cache();
skeleton.set_to_setup_pose();
skeleton.update_world_transform(Physics::None);

let state_data = Arc::new(AnimationStateData::new(Arc::clone(&data)));
let mut animation = AnimationState::new(state_data);
animation.set_animation_by_name(0, "walk", true)?;

// 3. Tick + render every frame.
let mut renderer = SkeletonRenderer::new();
for dt in frame_deltas { // your game loop
    animation.update(dt);
    let mut events = Vec::new();
    animation.apply(&mut skeleton, &mut events);
    skeleton.update_world_transform(Physics::Update);
    let commands = renderer.render(&skeleton);
    // Upload commands to your renderer. See dm_spine_bevy for one way.
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Design notes

- **Struct-of-arrays with typed indices.** `Skeleton` owns `Vec<Bone>`, `Vec<Slot>`, `Vec<IkConstraint>`, etc., and cross-references are `BoneId(u16)` / `SlotId(u16)` / `AnimationId(u16)`. No `Rc<RefCell<…>>` in hot paths.
- **Immutable shared data.** `SkeletonData` is `Arc`-shared across `Skeleton` instances — one load per asset, many instances cloning the `Arc` cheaply.
- **Tagged-enum timelines.** Timelines dispatch through a closed `enum Timeline` rather than `Box<dyn Timeline>`, keeping the inner-loop apply code cache-friendly.
- **No renderer types in the core.** `SkeletonRenderer::render` emits a `&[RenderCommand]`, each carrying plain vertex/uv/color/index buffers and a `TextureId(u32)`. Downstream code resolves the texture id to whatever GPU handle it owns.
- **Events via out-parameter.** `AnimationState::apply(&mut skeleton, &mut events: Vec<Event>)` — no listener callbacks, no allocation per event.

## Examples

Run any of these from the crate root. They expect the upstream [`spine-runtimes`](https://github.com/EsotericSoftware/spine-runtimes) repo to live as a sibling directory (`../spine-runtimes`) so they can load the canonical example rigs.

- `cargo run --example software_render` — pure-CPU rasterizer. Consumes the runtime's `RenderCommand` stream and writes a PNG. Useful as a reference implementation, and as a diagnostic when something visual goes wrong and you want to bisect runtime vs. renderer. Configurable via `SPINE_RIG`, `SPINE_ANIM`, `SPINE_TIME`, `SPINE_OUT` environment variables (see the file header).
- `cargo run --example dump_slots` — walks a rig's draw order and prints each drawable slot's attachment kind and world-space bounds. Handy when investigating per-slot regressions.

## Testing

```sh
cargo test          # unit + integration + goldens
cargo clippy --all-targets
cargo fmt
```

Goldens diff against dumps captured from `spine-cpp` via the small C++ harness under [`tools/spine_capture/`](tools/spine_capture/). Tolerances:

- Setup-pose bone transforms: 1e-4 (25/25 rigs).
- Animation samples: 1e-3 (34/35 samples match; one sub-0.05° applied-rotation drift on raptor-pro is a known numerical follow-up).
- Render-command headers (texture, blend, vertex count, color): exact (25/25 rigs).

If you regenerate fixtures, rebuild the capture harness with `make` inside its directory.

## Further reading

- [`docs/BINARY_FORMAT.md`](docs/BINARY_FORMAT.md) — reference for the Spine 4.2 binary `.skel` wire format, including the non-obvious encoding tricks (DrawOrder sign-via-wraparound, dual `Inherit` encoding, mesh triangle-count unit mixing, sequence path resolution). Written during the port to save the next implementer a debugging round trip.

## License

Distributed under the [Spine Runtimes License Agreement](https://esotericsoftware.com/spine-runtimes-license). See [`LICENSE`](./LICENSE) for the full text.

Copyright © 2013-2025 Esoteric Software LLC. Rust port © Dead Money, published under the same license.

## Acknowledgements

Built by porting [Esoteric Software](https://esotericsoftware.com/)'s C++ reference runtime. The upstream repository at [EsotericSoftware/spine-runtimes](https://github.com/EsotericSoftware/spine-runtimes) remains the source of truth — protocol or behaviour bugs in the underlying runtime should be reported there; port bugs should be filed here.
