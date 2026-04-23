# dm_spine_runtime

Native Rust port of the [Spine](https://esotericsoftware.com/) 4.2 runtime. Renderer-agnostic.

A Bevy integration layer lives in the sibling crate `dm_spine_bevy`. This crate has no GPU or windowing dependency and can in principle be used from any Rust rendering stack.

**Status:** In active development and not yet usable for shipping work. See [Phase tracker](#phase-tracker) below.

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

- `dm_spine_runtime` (this crate) — data types, loaders, skeleton pose, animation state, constraints (IK / Transform / Path / Physics), clipping, bounds, render-command emission. No rendering.
- `dm_spine_bevy` (sibling crate) — Bevy plugin, asset loaders, rendering systems.

Core design choices:

- SoA + typed indices (`BoneId(u16)`, `SlotId(u16)`, etc.) instead of pointer graphs. `Skeleton` owns `Vec<Bone>`, `Vec<Slot>`, etc.
- `SkeletonData` is immutable and shared across `Skeleton` instances via `Arc`.
- Timelines are a single tagged enum for cache-friendly dispatch.
- No render types in the core — the renderer receives `RenderCommand`s with opaque texture ids.

## Phase tracker

- [x] 0 — math primitives (`Color`, deg/rad trig helpers) and triangulator (ear-clipping + convex decompose)
- [x] 1 — atlas parser, data-type scaffold, binary `.skel` loader. Loads every rig shipped in `spine-runtimes/examples/` end-to-end through the `AtlasAttachmentLoader`. JSON loader deferred to Phase 8.
- [x] 2 — `Skeleton` runtime pose: update-cache ordering, bone world transforms with all five `Inherit` modes, skin activation + setup pose + attachment resolution. All 25 example skeletons match spine-cpp bit-for-bit on setup pose; constraint solvers are stubs until Phase 5.
- [ ] 3 — property timelines and single-track `AnimationState`
- [ ] 4 — full `AnimationState` (tracks, mixing, events, queue)
- [ ] 5 — constraints (IK → Transform → Path → Physics)
- [ ] 6 — clipping, bounds, render-command emission
- [ ] 7 — `dm_spine_bevy` plugin and examples

## Documentation

- [`docs/BINARY_FORMAT.md`](docs/BINARY_FORMAT.md) — full reference for the Spine 4.2 binary `.skel` wire format. Written during the port to save the next implementer the debugging round-trip; includes a gotchas section for the non-obvious encoding tricks (DrawOrder sign-via-wraparound, Inherit's dual encoding, mesh triangle-count unit mixing, sequence path resolution).
- [`tools/spine_capture/`](tools/spine_capture/) — small C++ harness that links spine-cpp and dumps setup-pose bone state as JSON fixtures for the Phase 2 golden tests. Rebuild with `make` in that directory and run `./capture_all.sh` to regenerate fixtures.

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
