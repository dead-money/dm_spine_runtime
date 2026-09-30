# dm_spine_runtime

Full-native Rust port of the Spine 4.3 runtime. The upgrade from 4.2 is tracked in `docs/SPINE_4_3_UPGRADE.md`; the last 4.2 state is tagged `v0.1.0`. The goal is to replace spine-godot's spine-cpp in `~/deadmoney/hommlet`, where it's too slow.

- **This crate** (`~/deadmoney/dm_spine_runtime/`) is the core runtime: data types, loaders, skeleton pose, animation state, constraints, clipping, bounds, render-command emission. **No GPU or windowing deps.**
- **`~/deadmoney/hommlet/`** is the primary consumer, reached through a C-ABI crate under its `Native/`, the same pattern as `Native/sorting`. The engine-facing work lives there; this crate stays engine-agnostic.
- **`~/deadmoney/dm_spine_bevy/`** is the Bevy integration and a secondary consumer. It depends on this crate via `path = "../dm_spine_runtime"` and stays on the 4.2 API until hommlet ships on 4.3; don't block runtime work on it.
- **`~/deadmoney/spine-runtimes/`** is the upstream reference. **Read-only.** Never edit.

`main` + `git log --first-parent` is truth for what landed.

The crate is pre-1.0 and nothing depends on its API. Don't preserve backward compatibility for APIs or formats: no deprecated aliases, dual loaders, or version shims. Support only the current Spine version.

## Attribution

Do not add `Co-Authored-By: Claude` trailers to commits or "Generated with Claude Code" footers to PR bodies. Author lines and PR bodies stay clean. `scripts/git-hooks/commit-msg` strips these defensively; activate per clone with `git config core.hooksPath scripts/git-hooks`. Do not work around the hook.

## Reference material

The `spine-runtimes` checkout is on a Dead Money branch (`deadmoney/4.7`, hommlet's spine-godot patches) built on an older 4.3. Port from `upstream/4.3`, not the working tree. Use a worktree:

```sh
git -C ~/deadmoney/spine-runtimes worktree add ../spine-runtimes-4.3 upstream/4.3
```

- Canonical C++ port: `spine-cpp/{include,src}/spine/` (4.2 used `spine-cpp/spine-cpp/…`).
- Cleaner-to-read TS port: `spine-ts/spine-core/src/`.
- Example skeletons/atlases: `examples/{spineboy,raptor,stretchyman,celestial-circus,…}/export/`. Re-exported per Spine version; fixtures, exports, and the harness's spine-cpp must match. hommlet's rigs (`hommlet/Assets/Spine/`) are the real workload.
- Format and API changes: `CHANGELOG.md`. It lags the code; the 4.3 timeline apply signature changed after its entry was written.

## Architectural invariants

Deviating from these is a design change. Raise it before implementing.

- **SoA + typed indices.** `Skeleton` owns `Vec<Bone>`, `Vec<Slot>`, and one `Vec<Constraint>` enum (4.3's unified, ordered constraint list). Cross-references are `BoneId(u16)` / `SlotId(u16)` / `ConstraintId(u16)` / `SkinId`. 4.3's pose pointers (`appliedPose`) become per-object pose fields plus a constrained flag. **No `Rc<RefCell<…>>`** in hot paths.
- **`SkeletonData` is immutable and shared** via `Arc<SkeletonData>`. One load per asset; many `Skeleton` instances reference it.
- **Runtime skins sit beside the data.** A `Skin` is a dense `(SlotId, placeholder)` table of `AttachmentRef::{Data(AttachmentId), Owned(u32)}`. Copied or remapped attachments live in the skin's own arena. Skeletons hold `Arc<Skin>`, so identical loadouts share one assembled skin.
- **Timelines are a tagged enum**, not `Box<dyn Timeline>`. Closed set, cache-friendly dispatch.
- **Unified update order.** One `Vec<UpdateCacheEntry>` (enum over `Bone(BoneId)` / `Constraint(ConstraintId)`) built by `updateCache()`. **Port the C++ algorithm literally**; the dependency logic is subtle.
- **No render types in core.** Emit `RenderCommand` with an opaque `TextureId`; downstream maps to GPU handles.
- **Events via out-param.** `AnimationState::apply(skeleton, events: &mut Vec<Event>)`. No listener callbacks in core.
- **Minimal deps.** `thiserror`, `glam`, `serde_json`. Anything new needs a reason.

## License obligation

Every ported source file must retain the **Spine Runtimes License header block** verbatim at the top (copy from any `spine-cpp` `src/spine/*.cpp`). The crate `LICENSE` file must be Esoteric's `LICENSE` verbatim. Downstream users need their own Spine Editor license; the README says so and must keep saying so.

## Port conventions

- Match `spine-cpp` function shape and file ordering 1:1 where feasible. Rust names in `snake_case`, but the same layout lets a reader diff the two.
- **Don't refactor math during a port.** Port literally first, verify against goldens, then refactor if it's worth it.
- **spine-cpp is the behavioral reference, not the quality bar.** Math, algorithms, and order of operations are ported literally. Its containers and allocation patterns are not: its `HashMap` is a linked list with O(n) lookups. Use `Vec` indexed by typed id, real maps, and reused scratch buffers. Steady-state frames should not allocate.
- **Performance and efficiency are the goals.** The runtime has to beat spine-cpp on hommlet's rigs. Between parity-equivalent designs, pick the cheaper one in time, memory, and per-frame work. Benchmark hot-path changes (`cargo bench`) rather than guessing.
- Binary reader: big-endian, zigzag varint, custom string table. Replicate `SkeletonBinary.cpp` exactly. Wire-format surprises go in `docs/BINARY_FORMAT.md`.

## Comments

Comments serve future readers, not the author of the diff. **Default to writing no comment.** A comment earns its place by saying something the code cannot: a non-obvious why, a hidden invariant, a workaround with a citation, or a subtle correctness anchor ("sign carried by u16 wraparound, matches `SkeletonBinary.cpp`").

- No restate-the-code, no narrated decision process, no banner dividers, no commented-out code.
- No task or bookkeeping references: "Phase 5c", "added for the Bevy loader", "fixes #12". They rot. That belongs in the commit message.
- Don't annotate individual items with "mirrors `spine::Foo`" or "port of `Foo.cpp:123`". The whole crate is a port. Cite upstream only when the citation anchors a non-obvious correctness point.
- `///` doc comments state the contract (preconditions, units, error modes) in a line or two. Don't restate the signature.
- Existing code predates this rule and is comment-heavy. Prune what you touch; don't do drive-by comment sweeps in unrelated diffs.

## Testing

Tests protect **parity with `spine-cpp`** and durable API contracts. The goldens are the contract; everything else is supporting.

**Default to no new tests for routine changes.** Running the existing suite doesn't imply adding to it, and a small fix doesn't automatically warrant a regression test.

- **Good candidates:** new golden coverage when a port adds behavior (a new constraint, timeline, or attachment path), loader regressions on real exports, crashes or non-finite output, and invariants the goldens can't see (update-cache ordering, track-entry lifecycle).
- **Generally don't test:** internal helper shapes, exact comment or error-message wording, or anything that just freezes current layout.
- Prefer extending an existing golden or smoke test over adding a new file. When the benefit is marginal, lean on `golden_*` and a `software_render` check instead.
- Don't loosen golden tolerances to make a diff pass. A new mismatch is either a port bug or a documented known drift. Say which in the PR.

### Goldens

Goldens diff against JSON dumps captured from `spine-cpp` by `tools/spine_capture/` (small C++ CLI; `make SPINE_RUNTIMES=<upstream/4.3 checkout at CI's pinned commit>`, then `capture_all.sh` / `capture_animations.sh` / `capture_render.sh`). The harness forces `Bone::setYDown(false)`: spine-cpp 4.3 defaults to y-down, while spine-ts, libgdx, and this crate are y-up. Fixtures are committed under `tests/fixtures/`. Tolerance: 1e-4 for setup-pose transforms, 1e-3 for animation samples, exact for render-command headers. Fixtures, example exports, and the spine-cpp the harness links must all be the same Spine version.

## Process

- **Merge PRs with a merge commit, never squash or rebase.** Full commit history is the record. Separate ideas land as separate commits and stay that way on `main`. `gh pr merge --merge --delete-branch`.
- **Use targeted verification.** `cargo check` while iterating, `cargo test --test golden_pose` (or the relevant suite) for the affected area. Run the full `cargo test` + `cargo clippy --all-targets` before opening a PR. Flag pre-existing failures by name; don't silence them.
- **PR bodies and commit messages are terse.** A sentence or two on what changed and why. No "## Summary" / "## Test plan" / "## Changes" scaffolding, no bulleted self-recaps, no boilerplate checklists. Same discipline as comments: say what the diff can't say, then stop.

## Status

The 4.2 port is tagged `v0.1.0`. The 4.3 port has reached parity and passed its performance gate (upgrade phases 0–6); phases 7–9 (hommlet runtime surface, hommlet adoption, release) remain. Update this section as they land. 4.3 parity:

- Setup pose: 43/43 rigs at 1e-4.
- Animation samples: 45/45 at 1e-3, with raptor-pro/roar front-bracer `a_rotation` in `KNOWN_DRIFT` (IK softness `acos` amplifies rounding).
- `AnimationState` scenarios (`golden_state`): 8/8.
- Render commands, including vertex positions and UVs: 43/43 rigs.
- Steady-state frames allocate nothing (`tests/alloc.rs`).
- hommlet rigs run at 0.51–0.63× spine-cpp's frame time. World transform on humanoid rigs is still about 1.1× spine-cpp; the difference is in transform constraints, which hommlet's rigs carry in the hundreds.


## Commands

- `cargo check`: fast type-check.
- `cargo test`: unit + golden tests. Needs `../spine-runtimes/examples` (or `SPINE_EXAMPLES`) to hold the matching Spine version's exports.
- `cargo clippy --all-targets`: lint.
- `cargo fmt`: format.
- `cargo bench --bench frame`: per-frame anim / world / render ns per rig. `HOMMLET_SPINE_ASSETS=../hommlet/Assets/Spine` adds hommlet's creature rigs.
- `tools/spine_capture/bench_compare.sh`: the same rigs, timed on both this crate and spine-cpp, side by side. `make -C tools/spine_capture` first.
- Visual: `cargo run --example software_render` (CPU rasterizer to PNG). `dm_spine_bevy`'s examples work only against the 4.2 tag until it's updated.
