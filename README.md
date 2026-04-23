# dm_spine_runtime

Native Rust port of the [Spine](https://esotericsoftware.com/) 4.2 runtime. Renderer-agnostic.

A Bevy integration layer lives in the sibling crate `dm_spine_bevy`. This crate has no GPU or windowing dependency and can in principle be used from any Rust rendering stack.

**Status:** The full pose-to-render pipeline is landed — data loader, skeleton pose, animation state, all four constraint solvers, clipping, bounds, and render-command emission. A Bevy integration crate is the next phase. Golden tests diff against spine-cpp captures at 1e-4 for setup pose (25/25 rigs), 1e-3 for animations (34/35 samples), and header-level for render commands (25/25 rigs). Not yet considered production-ready pending the Bevy integration and visual-parity verification. See [Phase tracker](#phase-tracker) below.

## Important: Spine Editor license required

This is **not** a cleanroom implementation. It is a derivative work of the official [spine-runtimes](https://github.com/EsotericSoftware/spine-runtimes) C++ runtime (spine-cpp), translated to Rust while preserving the structure of the original source and its copyright notices.

Because this is a derivative of the Spine Runtimes, distribution is governed by Section 2 of the [Spine Editor License Agreement](https://esotericsoftware.com/spine-editor-license) and the [Spine Runtimes License Agreement](https://esotericsoftware.com/spine-runtimes-license). In practical terms this means:

- **End users need a Spine Editor license.** If you ship software that uses this crate (directly or transitively), each of your users must hold their own [Spine Editor license](https://esotericsoftware.com/spine-purchase). This is the same obligation that applies to every official Spine runtime.
- **Copyright and license notices must be preserved.** Every source file ported from spine-cpp carries the original Esoteric Software copyright block and must retain it when redistributed. The `LICENSE` file at the root of this repository reproduces the Spine Runtimes License verbatim and must travel with any redistribution.
- **The Spine editor is separately licensed.** This runtime processes data exported by the Spine editor but is not a substitute for it.

If you are uncertain whether your use case complies, consult the official [Spine licensing page](https://esotericsoftware.com/spine-purchase) or contact Esoteric Software directly.

## Scope

Targets **Spine 4.2** data (binary `.skel` and atlas `.atlas`). This version introduces physics constraints, the `Inherit` timeline, and new mix thresholds. Earlier format versions are not currently supported.

## Architecture

- `dm_spine_runtime` (this crate) — data types, loaders, skeleton pose, animation state, constraints (IK / Transform / Path / Physics), clipping, bounds, render-command emission. No GPU or windowing dependency.
- `dm_spine_bevy` (sibling crate) — Bevy plugin, asset loaders, rendering systems. Consumes the `RenderCommand` stream from this crate.

Core design choices:

- SoA + typed indices (`BoneId(u16)`, `SlotId(u16)`, etc.) instead of pointer graphs. `Skeleton` owns `Vec<Bone>`, `Vec<Slot>`, etc.
- `SkeletonData` is immutable and shared across `Skeleton` instances via `Arc`.
- Timelines are a single tagged enum for cache-friendly dispatch.
- No GPU types in the core — `SkeletonRenderer::render` emits a `Vec<RenderCommand>` where each command carries plain `Vec<f32>` / `Vec<u16>` buffers and a `TextureId(u32)` (the atlas page index). Downstream renderers resolve that to whatever texture handle they own.

## Phase tracker

- [x] 0 — math primitives (`Color`, deg/rad trig helpers) and triangulator (ear-clipping + convex decompose)
- [x] 1 — atlas parser, data-type scaffold, binary `.skel` loader. Loads every rig shipped in `spine-runtimes/examples/` end-to-end through the `AtlasAttachmentLoader`. JSON loader deferred to Phase 8.
- [x] 2 — `Skeleton` runtime pose: update-cache ordering, bone world transforms with all five `Inherit` modes, skin activation + setup pose + attachment resolution. All 25 example skeletons match spine-cpp bit-for-bit on setup pose; constraint solvers are stubs until Phase 5.
- [x] 3 — property timelines + single-track `AnimationState`. Bone/slot/skeleton-wide/constraint timelines apply against setup pose with curves, MixBlend, MixDirection, and loop handling; animation goldens diff against spine-cpp for 7 animations across 3 rigs within 1e-3. Deform and Sequence timelines are no-op fallthroughs (need mesh-attachment plumbing); constraint solvers remain stubbed until Phase 5.
- [x] 4 — full `AnimationState`: multi-track + queuing (`set_animation` / `add_animation`), crossfade mixing with proper per-timeline classification (Subsequent / First / HoldSubsequent / HoldFirst / HoldMix) via `compute_hold`, event queue (Start / Interrupt / End / Complete / Dispose / Event), and empty animations (`set_empty_animation` for fading back to setup pose). Shortest-rotation rotate mixing and the `unkeyedState` attachment state-machine are visual-polish follow-ups.
- [x] 5 — constraint solvers: IK (1-bone + 2-bone with bend/softness/stretch), Transform (absolute/relative × world/local), Path (all spacing + rotate modes, including constant-speed bezier arc-length), Physics (damped-spring simulator). All four dispatch through `Skeleton::update_world_transform`; capture harness + goldens now run the full constraint pipeline. Post-phase parity pass (5f) zeroed constraint-data mix defaults to match spine-cpp (the `1.0` defaults were silently activating setup-disabled constraints) and fixed `Bone::update` to read applied TRS so a second cache run on a constrained bone preserves the constraint's effect. 25/25 setup-pose fixtures match spine-cpp at 1e-4.
- [x] 6 — clipping, bounds, render-command emission. `RenderCommand` + `TextureId` types in `src/render/` with a shared `Skeleton::compute_world_vertices` helper (6a). `DeformTimeline` / `SequenceTimeline` apply (6b) — previously no-op stubs; loader fixed to pre-add setup vertices for unweighted meshes and to emit bezier curves in spine-cpp's in-memory form. Draw-order walker + `RegionAttachment` / `MeshAttachment` emission with `MeshAttachment::update_region` covering all four `degrees` cases (6c, 6d). `SkeletonClipping` port (Sutherland-Hodgman, convex decomposition via the Phase 0 triangulator) wired into the walker (6e). `SkeletonBounds` port — AABB + point-in-polygon + segment-polygon hit tests (6f). Linked-list-equivalent command batcher (adjacent same-(texture, blend, color) runs merge) + render goldens (6g). `golden_render` headers-only diff (texture, blend, num_vertices, num_indices, color, dark_color) at 25/25. `golden_animation` at 34/35 (the remaining drift is a <0.05° applied-rotation extraction on raptor-pro). `render_smoke` exercises all 25 rigs end-to-end, 170 batched commands, no non-finite coordinates.
- [ ] 7 — `dm_spine_bevy` plugin and examples

## Documentation

- [`docs/BINARY_FORMAT.md`](docs/BINARY_FORMAT.md) — full reference for the Spine 4.2 binary `.skel` wire format. Written during the port to save the next implementer the debugging round-trip; includes a gotchas section for the non-obvious encoding tricks (DrawOrder sign-via-wraparound, Inherit's dual encoding, mesh triangle-count unit mixing, sequence path resolution).
- [`tools/spine_capture/`](tools/spine_capture/) — small C++ harness that links spine-cpp and dumps reference fixtures as JSON. Three modes: setup-pose bone state (`./capture_all.sh`, Phase 2 goldens), animation samples (`./capture_animations.sh`, Phase 3+ goldens), and `SkeletonRenderer` command summaries (`./capture_render.sh`, Phase 6 goldens). Rebuild with `make` in that directory.

## Development

```sh
cargo check
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt
```

Dev dependencies: `approx` (float comparisons), `proptest` (property tests), `pretty_assertions` (diff output). Runtime dependency: `glam`.

## License

Licensed under the [Spine Runtimes License Agreement](https://esotericsoftware.com/spine-runtimes-license). See `LICENSE` for the full text.

Copyright © 2013-2025 Esoteric Software LLC. Rust port by Brandon Reinhart / Dead Money, published under the same license.

## Acknowledgements

Built by carefully porting [Esoteric Software](https://esotericsoftware.com/)'s C++ reference runtime (spine-cpp). The upstream repository at [EsotericSoftware/spine-runtimes](https://github.com/EsotericSoftware/spine-runtimes) remains the source of truth; bugs in this port should be filed here, and protocol or behaviour bugs in the underlying runtime should be reported upstream.
