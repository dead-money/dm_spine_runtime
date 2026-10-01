# Spine 4.3 upgrade plan

Upgrade `spine_runtime` from Spine 4.2 to 4.3. Target is `upstream/4.3` at the tip (`ba17cf88b` at time of writing), not the `deadmoney/4.7` branch in the local `spine-runtimes` checkout, which is 232 commits behind it. Example exports in both are `4.3.75-beta`.

4.3 is not an incremental release for a port. It replaces the bone/slot/constraint model with a pose system, unifies constraints into one ordered list, rewrites the Transform constraint, replaces `MixBlend`/`MixDirection` in every timeline, reworks `AnimationState` hold/additive logic, moves UVs into `Sequence`, adds a Slider constraint, and changes the binary and JSON formats in most sections. 4.3 loaders reject 4.2 files. Expect to touch most of the crate. Rough size is 5–7k lines changed out of about 21.6k for parity, plus the hommlet-facing work in Phases 6–8.

## Current breakage (fix first, independent of the upgrade)

The local `spine-runtimes` working tree and upstream's default branch now carry only 4.3 exports. On `main` today:

- `cargo test --lib` fails in `renderer::tests::renders_spineboy_setup_pose` and `bounds::tests::update_on_example_rig`. The integration suites that walk `../spine-runtimes/examples` also fail.
- CI checks out `EsotericSoftware/spine-runtimes` with no `ref`, so it gets 4.3.
- `tools/spine_capture/Makefile` points at `spine-runtimes/spine-cpp/spine-cpp`, which no longer exists.

Fix: pin CI's spine-runtimes checkout to `ref: 4.2`, and run local 4.2 tests against a worktree (`git -C ../spine-runtimes worktree add ../spine-runtimes-4.2 upstream/4.2`). Better still, have the tests read an `SPINE_EXAMPLES` env var that falls back to `../spine-runtimes/examples`, so the 4.2 and 4.3 trees can coexist during the upgrade. This is a small PR to `main`.

## Decisions

Settled 2026-09-30.

1. **4.2 support is dropped going forward.** Tag the last 4.2 commit on `main` (`v0.1.0`, "Spine 4.2") and leave it on GitHub as the 4.2 version. Everything after it is 4.3-only, with no dual loader and no compatibility shims. Nothing depends on the 4.2 API.
2. **Target the upstream 4.3 tip**, re-synced once before the upgrade is declared done. The exports are still `-beta`, but hommlet's assets are already on 4.3.
3. **One enum constraint list.** `Vec<ConstraintData>` / `Vec<Constraint>` enums indexed by `ConstraintId(u16)`, with enum dispatch replacing virtual `sort` / `update` / `isSourceActive`. This replaces the per-type-`Vec` invariant in CLAUDE.md.
4. **Pose representation without pointers:**
   - `Bone { pose: BoneLocal, constrained: BoneLocal, is_constrained: bool, world: BoneWorld, world_stamp: u32, local_stamp: u32 }`. Only the applied pose ever carries a world matrix.
   - Slots and constraints follow the same pattern.
   - `applied()` / `applied_mut()` accessors select the pose.
   - The reset cache is a `Vec<ResetEntry>` enum plus a draw-order flag.
5. **Mirror the 4.3 API renames** (`setup_pose`, `setup_pose_bones`, `setup_pose_slots`, `AnimationState::track` / `set_track`, `source_mesh`, skin `placeholder`) with no aliases.
6. **Two oracles.** Upstream's HeadlessTest and its generated `SkeletonSerializer` check loader and data parity, via a Rust serializer with the same schema. `tools/spine_capture` stays the oracle for pose, animation, and render goldens.
7. **Runtime skins are separate from the shared data.**
   - Data attachments stay in `Arc<SkeletonData>` and are addressed by `AttachmentId(u32)`.
   - A runtime `Skin` is a dense table keyed by `(SlotId, placeholder)`. Each entry is `AttachmentRef::{Data(AttachmentId), Owned(u32)}`, so the placeholder lookup is an index, not a string hash.
   - Copied or remapped attachments live in an owned arena on the `Skin`.
   - A `Skeleton` holds its active skin as `Arc<Skin>`, so creatures with the same loadout share one assembled skin, including its precomputed sequence UVs.
   - Assembly is the only place that allocates, and it happens off the frame path.
8. **Performance and efficiency are the goals.** Given two parity-equivalent designs, pick the cheaper one in time, memory, and per-frame work. Every phase checks steady-state allocations. The spine-cpp comparison benchmarks exist from Phase 0 onward, not only in Phase 6.

## Goal

**Primary goal: replace spine-godot's spine-cpp in `../hommlet` with this runtime.** That makes hommlet the primary consumer and makes performance a gated requirement. `spine_bevy` is a secondary goal: it gets updated after hommlet ships on the new runtime, and it doesn't block any phase here.

**spine-cpp is the behavioral reference, not the quality bar.** Its `HashMap` is a linked list with O(n) lookups; replacing it in our fork was a measurable win. Port math, algorithms, and order of operations literally, because the goldens depend on them. Containers, lookups, and allocation strategy are ours to do properly: `Vec` indexing by typed id, real hash maps or dense tables, reused scratch buffers, and no per-frame allocation in the hot path.

hommlet's baseline (2026-09-27 bench, "war" save) is 2.71 ms/frame total. Spine takes about 0.36 ms of that, with `AnimationState::computeHold` alone at about 0.12 ms. Canvas, RenderingDevice, and driver take about 0.43 ms, and the SpineSprite path draws one canvas item per slot. Skeleton evaluation is only part of the cost, so the integration phase has to cut draw submission as well as evaluation.

## Branching

Phase 0 landed on `main` as its own PR. Phases 1–6 landed together as one PR from `spine-4.3/core`, one commit per phase. The 4.3 data model breaks every layer above it at once, so there was no intermediate state that compiled. Phases 7 onward land as separate PRs, merged with merge commits. The crate must compile and pass clippy at every merge.

## Progress

- **0–6 done.** Setup pose 43/43, animation samples 45/45, `AnimationState` scenarios 8/8, render commands (including positions and UVs) 43/43. Steady-state frames don't allocate. On hommlet's rigs a full frame takes 0.51–0.63× spine-cpp's time (Human: 7.8 µs against 13.0 µs). World transform on humanoids is still 1.1× spine-cpp; that's the next performance target once hommlet profiles it in place.
- **7 done.** Skins are flat tables over interned `(slot, placeholder)` keys, worn as `Arc<Skin>`, with owned attachment copies (`copy`, `set_region`, `tag`), `add_skin`, `copy_skin`, in-place edits through `Skeleton::skin_mut`, and `compact_skin`. Placeholders the data never mentions are kept per skin and reached only by name. Changing skins clears slots left showing an owned attachment of the old skin, where spine-cpp would keep a dangling pointer. `RenderOptions` adds per-vertex slot and tag streams and merging across colors. `RegionGeometry` maps region points for weapon trails. Track save and restore needs no new API; a test covers hommlet's round trip. `get_slot_attachment_indices` was dropped: it only fed slot colors that `creature.gdshader` never reads, and the per-vertex streams replace it.
- **8–9 open.**

## Phases

### 0. Harness and fixtures

- `tools/spine_capture`: point it at 4.3 spine-cpp (`spine-cpp/{include,src}/spine`, sourced from an `upstream/4.3` worktree). Port `main.cpp` and `synthetic.cpp` to the 4.3 API:
  - `Array` replaces `Vector`.
  - `Skeleton(data&)`, `setupPose()`.
  - `getAppliedPose()` replaces `getAX` / `getAppliedRotation` / etc.
  - `Animation::apply(..., MixFrom, add, out, appliedPose)`.
  - `getConstraints()` + RTTI.
- Build HeadlessTest and the serializer from the same worktree.
- Add a spine-cpp timing mode to the harness, and a criterion bench skeleton in `benches/`, so every phase can compare against spine-cpp. Phase 6 is the dedicated tuning pass; measurement starts here.
- Regenerate every fixture under `tests/fixtures/` from 4.3 exports. Keep the current 25-rig set (`sack` moved to `examples/7-anticipation/export/`) and add `diamond-pro`, the only example that uses a Slider. Add the numbered `1-…`–`8-…` principle rigs, `spinosaurus`, and `food-app` to load and smoke coverage only.
- No example uses inverse or convex clipping or `drawOrderFolder`. Cover those with `synthetic.cpp` cases.
- Tests read examples through the `SPINE_EXAMPLES` override.

Gate: harness builds against 4.3, and fixtures are committed. Rust goldens are expected to fail.

**Done.**
- The harness builds against `upstream/4.3` at `ba17cf88b`, which is the commit CI pins.
- Fixtures cover 43 rigs: setup pose (now including update-cache order), render headers, and 45 animation samples, including `diamond-pro`.
- Tests resolve rigs through `SPINE_EXAMPLES` and the fixtures' own `source_*` paths.
- Tests that need unported code are `#[ignore = "Spine 4.3 phase N"]`.

Notes:
- spine-cpp 4.3 defaults `Bone::yDown` to true; 4.2 and spine-ts default to false. The harness forces y-up, and the Rust default stays y-up.
- `synthetic.cpp` was ported but not yet extended with inverse or convex clipping cases. Those belong with Phase 5.
- HeadlessTest and the serializer oracle move to Phase 1, where they're first needed.

**spine-cpp baseline** (`-O2`, one skeleton, 60 Hz, `Physics_None`, ns/frame; `spine_capture --bench`):

| rig (skin, animation) | anim | world | render | total |
|---|---|---|---|---|
| Human (Human01, `WR_walkCombat_F`) | 7456 | 2713 | 3033 | 13202 |
| Goblin (Goblin01, same) | 5810 | 2822 | 3347 | 11979 |
| Orc (Orc01, same) | 7709 | 2323 | 2834 | 12866 |
| Ogre (Ogre01, same) | 6344 | 2922 | 2959 | 12225 |
| Deer (Doe, `walk_F`) | 870 | 2241 | 4038 | 7149 |

Animation apply dominates on the humanoid rigs. They carry 206–209 animations; the cost is per-track timeline work, which matches hommlet's own profile showing `computeHold` as the top Spine cost.

### 1. Data model and loaders

Port `SkeletonBinary.cpp` / `SkeletonJson.cpp` at the tip, plus the data types they populate. No runtime behavior yet beyond what loading needs.

- **Version:** `TARGET_VERSION` becomes `"4.3"`. Reject anything else.
- **Bones:** field order is now `inherit` before `length`. Add nonessential `icon_size` / `icon_rotation`. Setup values become a `BoneLocal` setup pose.
- **Constraints:** one list with a type byte (IK / path / transform / physics / slider) and no `order`.
  - IK: `scale_y_mode` replaces `uniform`, and the bend-direction bit is inverted.
  - Path: flags shift by one bit, and `skinRequired` moves into the flags.
  - Physics: `scale_y_mode` is encoded in the sign of `scaleX`.
  - Transform: full data rewrite. Add `source`, `local_source`, `local_target`, `additive`, `clamp`, and `offsets[6]`, plus `properties: Vec<FromProperty { kind, offset, to: Vec<ToProperty { kind, offset, max, scale }> }>` as closed enums.
  - Slider data: the animation index is read after the animations section.
- **Skins:** one constraint-index list, and entries keyed by `placeholder`.
- **Attachments:**
  - Weighted vertices carry a leading total-length varint.
  - Mesh gains `timeline_slots`.
  - Linked mesh gains `source_index`, read before the skin index; `parent_mesh` becomes `source_mesh`, and the source can sit in another slot.
  - Clipping gains `convex` / `inverse` flags.
  - `timeline_attachment` moves to the attachment level so regions have one too.
- **Sequence refactor:** every Region and Mesh has a non-optional `Sequence` (count 1, `path_suffix = false` when absent). Per-frame `uvs` / `offsets` are precomputed by `update_sequence()`, which uses static `compute_uvs` ports. `update_region` and the attachment-level `region` / `uvs` / `vertex_offset` fields are deleted. Missing atlas regions load as `None` instead of erroring, matching cpp.
- **AttachmentLoader:** the trait takes `(skin, placeholder, name, path, &mut Sequence)`.
- **Events:** `EventData.setup_pose: Event` replaces the flat defaults.
- **Animations:**
  - Add `bones: Vec<BoneId>` (deduplicated from bone timelines) and nonessential `color`.
  - New timeline data for `Slider`, `SliderMix`, and `DrawOrderFolder`.
  - The `DrawOrder` property id becomes `ordinal << 53`, and the Sequence property id uses `sequence.id`.
  - Constraint timeline indices point into the unified list.
- **Atlas:** no format change. Optionally mirror the `packedWidth`/`packedHeight` swap for rotated regions if the UV goldens need it.
- **Skins:** the `AttachmentId` / `AttachmentRef` / `Skin` layout from decision 7.
- `docs/BINARY_FORMAT.md` is rewritten for 4.3 in this phase, while the reader is fresh.

Gate: every `.skel` and `.json` under `examples/*/export` loads, binary and JSON produce equal `SkeletonData`, and the Rust serializer matches HeadlessTest's `SkeletonData` dump for the fixture rigs.

### 2. Pose system and update cache

The core of the upgrade. Everything downstream reads the applied pose.

- **2a. Poses.** `BoneLocal` / `BonePose` / `SlotPose` / `DrawOrder` with pose, constrained, and applied selection per decision 4.
  - `Skeleton` gains the `update` counter, the reset cache, and `constrained()`.
  - Bone world update reads the parent's applied pose and skips bones whose world stamp is current.
  - Rename `update_applied_transform` to `update_local_transform`, and port `validate_local_transform`, `modify_local`, `modify_world`, and the recursive `reset_world` literally. The stamps decide when bones recompute, so an approximation will drift the goldens.
- **2b. `update_cache` rewrite.**
  1. Reset everything to unconstrained.
  2. Set bone `active` from the skin.
  3. Per constraint, set `active = is_source_active && skin check`, then `sort` via enum dispatch.
  4. `sort_bone` all bones.
  5. Cache entries run the applied pose.
  - `update_world_transform`: bump `update`, reset the constrained draw order, run `reset_constrained` over the reset cache, then run the cache.
  - Keep `UpdateCacheEntry` as an enum, and add `Slider`.
- **2c. IK, Path, Physics on poses.**
  - Mixes and settings move to per-constraint pose structs.
  - IK gains `ScaleYMode` Volume, and its sort simplifies.
  - Physics gains `scale_y_mode`, the `x/y/rotate/scale` lag terms, `reference_scale` applied to wind and gravity, and `modify_world` before writes.
  - The two-bone IK needs a split borrow over `bones`, and `reset_world` is a free function over `&mut [Bone]`.
- **2d. Transform constraint rewrite** against the new From/To property model.
- Update `compute_world_vertices`, `SkeletonBounds`, and the path solver to read applied slot and bone poses.

Gate: `golden_pose` passes on all fixture rigs at 1e-4, and `update_cache_all_rigs` passes.

### 3. Timelines and AnimationState

Port from the tip, not the CHANGELOG. The CHANGELOG's `fromSetup` / `add` / `out` signature was superseded by `MixFrom { Current, Setup, First }` (cpp `33992df1f`).

- **3a. Timeline apply.**
  - Delete `MixBlend` / `MixDirection`. Every `apply_*` becomes `(…, alpha, from: MixFrom, add, out, applied_pose)`.
  - Add the per-timeline `additive` / `instant` flags and the `First` before-first-key branches.
  - Rewrite the `CurveTimeline1` helpers (`relative` / `absolute` / `scale` values).
  - Deform and Sequence apply to the keyed slot plus `timeline_slots`, gated on `is_timeline_active`.
  - Add `DrawOrderFolder`.
  - `Animation::apply` takes the same parameters.
- **3b. AnimationState.**
  - `TrackEntry`: remove `mix_blend`, `hold_previous`, and `interrupt_alpha`. Add `additive`, `keep_hold`, and `mix_interpolation`. `Interpolation` is a closed enum: linear, smooth, slowFast, fastSlow, circle, pow, powOut.
  - `timeline_mode` becomes a `u8` bitmask (`CURRENT`/`SETUP`/`FIRST`, `MODE` mask, `HOLD`).
  - Rewrite `animations_changed` with a property → owning-track map and oldest-first iteration.
  - Rewrite `compute_hold`, `apply_mixing_from` (new alpha formulas, draw-order-out rule, events queued only when `to.mix_duration > 0`), and the `apply` loop (fast path only for track 0 at alpha 1).
  - Rewrite the attachment-timeline helper (`ATTACH_SETUP` / `ATTACH_RETAIN`, hidden-setup fix).
  - Add reverse events.
  - `set_animation` replaces a never-applied entry only when it is the same animation.
  - Survives mostly intact: the entry slab, queueing, empty animations, `update`, and `update_mixing_from` (which gains `keep_hold` propagation).
  - Closes the 4.2 follow-ups: shortest rotation is now forced for additive, and the `unkeyedState` attachment handling is replaced upstream.
- **3c. Captures.** Extend `capture_animations.sh` with multi-track and additive samples; 4.2 goldens were single-track only.

Gate: `golden_animation` passes at 1e-3, and `multi_track` passes, updated to the 4.3 semantics.

### 4. Slider constraint

- Add `Slider` / `SliderData` / `SliderPose` (`time`, `mix`). Bone-driven time uses a `FromProperty` with loop wrap or clamp.
- `update` calls `modify_local` on every bone in `animation.bones`, then applies the animation to the applied pose.
- `sort` calls `constrained()` on everything the animation's timelines touch.
- The slider applies `&Animation` from `SkeletonData` while mutating the skeleton. Clone the `Arc<SkeletonData>` at the top of `update_world_transform`, which costs one refcount bump per frame, or split the borrow.
- Constrained slots copy `deform` in `reset_constrained`. Reuse the buffer so this doesn't allocate every frame.

Gate: `diamond-pro` pose and animation goldens, plus synthetic slider cases.

### 5. Render path

- `SkeletonRenderer` walks the applied draw order and reads the attachment, color, dark color, and sequence index from the applied slot pose. It picks the region and UVs through `sequence.resolve_index` / `uvs(i)` / `offsets(i)`. Delete `resolve_region_texture` / `region_uvs_for`. Meshes now get per-frame sequence UVs, which the 4.2 renderer never wired.
- `RegionAttachment::compute_world_vertices` takes explicit offsets.
- **`SkeletonClipping`:**
  - Add convex and inverse clipping: `make_clockwise` returns convexity, add a `make_convex` monotone-chain hull, and add `clip_inverse` with lazy barycentric UVs.
  - Rewrite `clip`.
  - `clip_triangles` returns `bool`.
  - Triangulator `decompose` closes its polygons.
- `SkeletonBounds`: pose reads, plus `min_x` / `min_y` / `max_x` / `max_y`.
- `RenderCommand` and the batcher are unchanged.

Gate: `golden_render` headers match exactly on all fixture rigs, plus synthetic inverse and convex clip cases. `render_smoke` runs over every example rig.

### 6. Performance pass

Performance work comes after parity so that optimizations are checked against the goldens and don't hide port bugs.

- **Benchmarks.** Add `benches/` (criterion) covering load, `AnimationState::update` + `apply`, `update_world_transform`, and `render`, per rig. The bench reads hommlet's rigs from `HOMMLET_SPINE_ASSETS` (Human.skel is 8.8 MB) plus the example rigs. It uses the spine-cpp timing mode from Phase 0, so both runtimes are measured on the same rig, animation, and frame count.
- **Audit spine-cpp-shaped hot paths:**
  - property-id and timeline lookups in `AnimationState`
  - `compute_hold`
  - skin attachment lookup
  - `update_cache` rebuilds
  - per-frame `Vec` allocation in the renderer and clipper
- Keep an allocation-count check (a counting global allocator in a bench or test) at zero for steady-state frames.

Gate: every hommlet rig is faster than spine-cpp on update + apply + world transform + render, with the numbers recorded in the PR, and there are no steady-state allocations.

### 7. hommlet integration runtime surface

What the core crate must provide for hommlet, beyond upstream parity. Each item is engine-agnostic.

- **Runtime skin assembly.** hommlet builds per-creature skins: `new_skin`, `add_skin(template)`, copy a template attachment, remap its region to another atlas, then set or remove it. Today skins and attachments live in the immutable `Arc<SkeletonData>`. Runtime skins need owned attachments.
  - Design per decision 7. The `AttachmentRef` / `Skin` layout lands in Phase 1 with the loaders. The assembly API (`new_skin`, `add_skin`, copy, remap, set, remove) lands here.
- **Region remap and synthetic regions.** Rebuild a copied attachment's `Sequence` against a different atlas region, recomputing UVs and offsets. hommlet's fork does the same with `Sequence::update`. Also support a synthetic region over an arbitrary `TextureId` (hommlet's `SpineTextureRegion`, used for hauled-item icons).
- **Per-attachment user tag.** hommlet's fork adds `MaskIndex` to `Attachment`. Add a generic `u32` user tag on attachments, preserved by copy.
- **Per-vertex render streams.** `RenderCommand` gains optional per-vertex `(slot_index, attachment_tag)`. `creature.gdshader` reads these from `CUSTOM0` for LUT recolouring and wading masks.
  - With these streams the batcher can merge slots into one command per texture and blend run. That replaces hommlet's one-canvas-item-per-slot, which is the draw-submission win.
- **Queries.** hommlet needs:
  - bone applied world position and rotation
  - slot attachment name
  - `get_slot_attachment_indices` into a caller span
  - `SkeletonBounds` AABB, point containment, and allocation-free polygon count/point getters
  - `map_region_points`: region-local UV through the posed region or mesh triangles into skeleton space, for weapon trails
- **Events-only tick.** `AnimationState::update` + `apply` with no world transform or render, for off-screen creatures. The API already supports this; it needs a bench and documentation.
- **TrackEntry save/load surface.** hommlet serializes the track-0 queue: track time, animation last, time scale, mix duration, loop, delay, next, mixing-from, and empty-animation flags.

Gate: each item has a unit or golden-backed test, and the render-stream and skin-assembly items are demonstrated on hommlet's Human and Goblin rigs.

### 8. hommlet adoption (lands in hommlet)

The Godot and C# work is tracked in hommlet, in a research memo that supersedes its stale `Docs/dm_spine_godot.txt`. Outline:

- **`Native/spine` cdylib wrapping this crate.** It follows the `Native/sorting` template: C ABI, opaque handles, `#[repr(C)]` structs, native-owned output buffers valid until the next call, `[LibraryImport]` via `NativeLibraryResolver`, and `just build-native` for Linux, MinGW, and sniper. It depends on this crate by git revision, not a path.
  - Batch entry points are preferred, e.g. updating N skeletons per call and emitting all their render streams at once, so hundreds of creatures don't cost hundreds of P/Invokes.
- **Swap at the `CreatureSpineInstance` seam.** It already fronts most SpineSprite usage. The Rust loader reads `.skel` / `.atlas` directly, so the Godot Spine importers are no longer needed.
- **Rendering.** One canvas item per creature carries the engine sort key and a per-creature `ShaderMaterial`. Meshes are built from the render streams with `CUSTOM0` (slot, mask). The contract to keep:
  - PMA
  - MRT outputs
  - flip via negative `scale.x`
  - feet as the node origin
  - paperdolls in MRT SubViewports
- **Parity check.** A/B screenshots and `PerfBench` on the "war" save and `--bench-stress` / `--bench-battle`. Keep the spine-godot path selectable until parity is signed off, then remove the module from the engine fork.

### 9. Release

- Update the README, the CLAUDE.md status section, and the `Cargo.toml` description to say 4.3. Update the `LICENSE` header year to match upstream's 4.3 header.
- CI checks out `spine-runtimes` at `ref: 4.3`.
- Tag `v0.2.0`.
- `spine_bevy` (secondary, after hommlet ships):
  - setup-pose and `AnimationState::track` renames, and apply-signature fallout
  - expose `TrackEntry::additive` / `mix_interpolation`
  - use the per-vertex streams if they're useful
  - a `spine_browser` visual pass
  - README says 4.3

## Risks

- **Update stamps and `reset_world`.** Lazy local/world recompute is the most likely source of small, hard-to-bisect drift. Port it literally and compare per-bone applied poses from HeadlessTest when a golden fails.
- **Upstream is still moving.** 4.3 has had signature changes after its CHANGELOG entries. Pin the capture harness to a commit and re-sync once, deliberately, before tagging `v0.2.0`.
- **hommlet exports are `4.3.26`, the examples are `4.3.75-beta`.** Both pass the `4.3` prefix check. Add hommlet's six rigs to the Phase 1 load gate via `HOMMLET_SPINE_ASSETS` to catch format drift between the two editor builds.
- **Coverage gaps.** Only one example rig exercises Sliders, and none use inverse clipping or draw-order folders. Synthetic cases carry those.
- **The known 4.2 drift** (raptor-pro front-bracer, <0.05°) sits in `update_applied_transform`, which 4.3 rewrites. Re-check it rather than assume it carries over or vanishes.
