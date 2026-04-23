# dm_spine_runtime

Full-native Rust port of the Spine 4.2 runtime. Two sibling crates:

- **This crate** (`~/deadmoney/dm_spine_runtime/`) — core runtime. Data types, loaders, skeleton pose, animation state, constraints, clipping, bounds, render-command emission. **No GPU or windowing deps.**
- **`~/deadmoney/dm_spine_bevy/`** — Bevy integration. Depends on this crate via `path = "../dm_spine_runtime"`. Owns the plugin, assets, systems, meshes. Use its `examples/` for visual verification.
- **`~/deadmoney/spine-runtimes/`** — upstream reference. **Read-only.** Never edit.

## Reference material

- Canonical C++ port: `~/deadmoney/spine-runtimes/spine-cpp/spine-cpp/{include,src}/spine/`
- Cleaner-to-read TS port: `~/deadmoney/spine-runtimes/spine-ts/spine-core/src/`
- Example skeletons/atlases: `~/deadmoney/spine-runtimes/examples/{spineboy,raptor,stretchyman,celestial-circus,…}/export/`
- Format changes: `~/deadmoney/spine-runtimes/CHANGELOG.md`. **Target Spine 4.2** (physics, `Inherit` timeline, new mix thresholds).

## Architectural invariants

Deviating from these is a design change — raise it before implementing.

- **SoA + typed indices.** `Skeleton` owns `Vec<Bone>`, `Vec<Slot>`, `Vec<IkConstraint>`, etc. Cross-references are `BoneId(u16)` / `SlotId(u16)` / `SkinId`. **No `Rc<RefCell<…>>`** in hot paths.
- **`SkeletonData` is immutable and shared** via `Arc<SkeletonData>`. One load per asset; many `Skeleton` instances reference it.
- **Timelines are a tagged enum**, not `Box<dyn Timeline>`. Closed set, cache-friendly dispatch.
- **Unified update order.** One `Vec<UpdateCacheEntry>` (enum over `Bone(BoneId)` / `IkConstraint(IkConstraintId)` / …) built by `updateCache()`. **Port the C++ algorithm literally** — dependency logic is subtle.
- **No render types in core.** Emit `RenderCommand` with an opaque `TextureId`; downstream maps to GPU handles.
- **Events via out-param.** `AnimationState::apply(skeleton, events: &mut Vec<Event>)`. No listener callbacks in core.
- **Minimal deps.** `thiserror`, `byteorder`/`bytes`, `glam` (feature-gated). `serde_json` only behind a `json` feature.

## License obligation

Every ported source file must retain the **Spine Runtimes License header block** verbatim at the top (copy from any `spine-cpp/spine-cpp/src/spine/*.cpp`). The crate `LICENSE` file must be Esoteric's `LICENSE` verbatim. Downstream users need their own Spine Editor license — call this out in README.

## Port conventions

- Match `spine-cpp` function shape and file ordering 1:1 where feasible. Rust names in `snake_case` but same layout lets a reader diff the two.
- **Don't refactor math during the port.** Port literally first, verify against goldens, then refactor if worth it.
- Binary reader: big-endian, zigzag varint, custom string table. Replicate `SkeletonBinary.cpp` exactly.

## Phase tracker

- [x] 0 — math (`Color`, deg/rad trig helpers), triangulator (ear-clipping + convex decompose). Curves deferred to Phase 3 with timelines.
- [x] 1 — atlas parser (1a), data-type scaffold (1b), binary `.skel` loader (1c). All 25 example skeletons load through `AtlasAttachmentLoader`. JSON loader deferred to Phase 8.
- [x] 2 — `Skeleton` runtime pose: update-cache ordering (2c), bone world transforms with all five `Inherit` modes (2d), skin activation + setup-pose + attachment resolution (2e). All 25 example skeletons match spine-cpp bit-for-bit on setup pose via `tests/golden_pose.rs`; constraints are stubs until Phase 5.
- [ ] 3 — property timelines + single-track `AnimationState`
- [ ] 4 — full `AnimationState` (tracks, mixing, events, queue)
- [ ] 5 — constraints (IK → Transform → Path → Physics)
- [ ] 6 — clipping, bounds, render-command emission
- [ ] 7 — `dm_spine_bevy` plugin + examples

Check the box when that phase's golden tests pass.

## Golden tests

Phases 2–5 are validated by comparison against dumps captured from spine-cpp. Build the capture harness at Phase 2 under `tools/spine_capture/` (small C++ CLI). Commit dumped JSON fixtures under `tests/fixtures/`. Tolerance ~1e-4 for transforms.

## Commands

- `cargo check` — fast type-check
- `cargo test` — unit + golden tests
- `cargo clippy --all-targets` — lint
- `cargo fmt` — format
- Visual (Phase 7+): `cd ../dm_spine_bevy && cargo run --example spineboy_walk`
