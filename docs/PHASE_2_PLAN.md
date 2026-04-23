# Phase 2 plan — Skeleton pose pipeline

*This document is planning scaffolding for the next session. Delete or
archive it when Phase 2 is complete.*

## What Phase 2 delivers

The runtime **pose pipeline**: given an `Arc<SkeletonData>`, an active skin
name, and no animations, compute every bone's world-space transform. This is
the minimum viable runtime — enough for Phase 7 (Bevy) to render a T-pose.

Concretely, at the end of Phase 2 this should work:

```rust
let data: Arc<SkeletonData> = /* Phase 1 binary load */;
let mut skeleton = Skeleton::new(Arc::clone(&data));
skeleton.set_skin_by_name("default")?;
skeleton.set_to_setup_pose();
skeleton.update_world_transform(Physics::None);

for bone in skeleton.bones() {
    println!("{}: world=({}, {}) a={}, b={}, c={}, d={}",
        bone.data().name, bone.world_x(), bone.world_y(),
        bone.a(), bone.b(), bone.c(), bone.d());
}
```

…and the values match spine-cpp bit-for-bit within a small float epsilon.

## Non-goals for Phase 2

- **No animation evaluation.** `Timeline::apply` is Phase 3.
- **No `AnimationState`.** Phase 4.
- **No real constraint solvers.** Constraint instance structs exist and get
  referenced by `update_cache`, but their `update()` methods are no-ops.
  Phase 5 fills them in.
- **No clipping / render commands.** Phase 6.
- **No rendering.** Phase 7 (Bevy) consumes the world transforms this phase
  produces.

## Sub-phases (run in order)

### 2a — Golden-capture harness *(do this first)*

See the `project_capture_harness_phase2` memory for the rule, and below for
the rationale. Build `tools/spine_capture/`:

- CMake project vendoring or fetching `spine-cpp`.
- `main.cpp` that takes `atlas_path`, `skel_path`, and an output path; loads
  the skeleton, calls `setToSetupPose()` + `updateWorldTransform(SP_PHYSICS_NONE)`,
  dumps every bone's world state as JSON.
- Runner script that captures every rig in `spine-runtimes/examples/` into
  `tests/fixtures/{rig}/setup_pose.json`.

**Output JSON shape** (one array entry per bone, in `data.bones` order):

```json
{
  "index": 3,
  "name": "head",
  "a": 0.9876, "b": -0.0342, "c": 0.0342, "d": 0.9876,
  "world_x": 123.45, "world_y": -67.89,
  "ax": 10.0, "ay": 5.0,
  "a_rotation": 5.0,
  "a_scale_x": 1.0, "a_scale_y": 1.0,
  "a_shear_x": 0.0, "a_shear_y": 0.0
}
```

**Fallback if CMake is painful:** Node script using `spine-ts` instead.
spine-ts produces the same floats as spine-cpp on the platforms we care
about, so it's a clean substitute.

### 2b — Runtime types scaffolding

New module: `src/skeleton/` (sibling to `data/`).

- `src/skeleton/mod.rs` — re-exports.
- `src/skeleton/bone.rs` — runtime `Bone` with mutable local TRS + computed
  world-space `a,b,c,d,world_x,world_y`. Also `applied_valid` bool and the
  "applied" local copies Spine uses for inherit math. Holds an index back
  into `data.bones`.
- `src/skeleton/slot.rs` — runtime `Slot` with mutable color / dark color /
  attachment reference / scratch deform vertex buffer.
- `src/skeleton/constraint.rs` — stub `IkConstraint`, `TransformConstraint`,
  `PathConstraint`, `PhysicsConstraint` each with a no-op `update()`. They
  must hold enough data for Phase 5 to fill in the solvers without changing
  the `Skeleton` public API.
- `src/skeleton/skeleton.rs` — the main `Skeleton` struct. Owns the vecs;
  holds `Arc<SkeletonData>`; tracks active skin (`Option<SkinId>`), `time`
  (for Phase 5 physics), and the update cache.

At this sub-phase, **every method is a stub** except constructors. This
gives us a clean compile checkpoint before writing the pose math.

### 2c — `update_cache` (port of `Skeleton::updateCache`)

`spine-cpp/src/spine/Skeleton.cpp` — the `updateCache` function and its
helpers (`sortBone`, `sortIkConstraint`, `sortTransformConstraint`,
`sortPathConstraint`, `sortPhysicsConstraint`, `sortReset`).

Produce a `Vec<UpdateCacheEntry>` where `UpdateCacheEntry` is a small enum:

```rust
pub enum UpdateCacheEntry {
    Bone(BoneId),
    IkConstraint(IkConstraintId),
    TransformConstraint(TransformConstraintId),
    PathConstraint(PathConstraintId),
    PhysicsConstraint(PhysicsConstraintId),
}
```

**This is the subtle one.** Dependency ordering between constraints that
affect overlapping bone sets is non-obvious. Rule: **port the algorithm
verbatim**, including the `sorted` / `visited` boolean state per bone.
Matches the existing memory rule. Do not try to invent a simpler algorithm.

Unit-test with small hand-built `SkeletonData` instances where we know the
expected cache order.

### 2d — `Bone::update_world_transform`

`spine-cpp/src/spine/Bone.cpp` — `Bone::updateWorldTransform` and its
overload. ~100 lines of trig but every line matters.

**Five `Inherit` modes to implement** (from the `Inherit` enum):

- `Normal` — straight `M_parent * M_local`.
- `OnlyTranslation` — apply parent translation only; use local rotation /
  scale / shear directly.
- `NoRotationOrReflection` — subtract parent rotation from local rotation
  while still inheriting scale. The reflection check (`(pa * pd - pb * pc < 0)`)
  is where reflection handling lives.
- `NoScale` — inherit rotation but not parent scale; introduces per-axis
  sign handling to preserve reflection.
- `NoScaleOrReflection` — as above, but also drop reflection.

Glam's `Affine2` doesn't map 1:1 here because Spine uses explicit
`a,b,c,d,world_x,world_y` fields in several places. I recommend keeping
those as bare f32 fields on `Bone` (matching spine-cpp) and only converting
to `Affine2` at the Bevy boundary. No wrapping of fundamentals — port
verbatim.

### 2e — Skin activation and setup pose

- `set_skin(Option<SkinId>)` — activate the skin, re-run `update_cache`
  (skin-required bones/constraints change).
- `set_skin_by_name(&str)` — find + call above.
- `set_to_setup_pose()` — reset all bones, slots, constraints.
- `set_bones_to_setup_pose()`, `set_slots_to_setup_pose()` (separate for
  parity with spine-cpp API — sometimes animations want just one).
- Attachment resolution via the active skin + default skin fallback.

### 2f — Golden pose tests

- `tests/golden_pose.rs` — integration test that loads every
  `examples/*/export/*.skel`, runs `set_to_setup_pose` +
  `update_world_transform(Physics::None)`, and compares every bone's
  state against `tests/fixtures/{rig}/setup_pose.json` with
  `assert_abs_diff_eq!` at tolerance `1e-4`.
- Hand-picked rigs to spot-check specific things:
  - spineboy-pro: large bone count, standard Inherit::Normal everywhere.
  - stretchyman: exercises NoScale / NoScaleOrReflection (it's literally
    designed to stretch, which uses non-standard inherit modes).
  - celestial-circus: has physics constraints → forces us to
    include them in the update cache as stubs, but shouldn't affect
    setup pose.

## Reference files to read first (in order)

1. `spine-cpp/spine-cpp/include/spine/Skeleton.h` — top-level shape.
2. `spine-cpp/spine-cpp/src/spine/Skeleton.cpp` — especially
   `updateCache()` (lines ~95–400) and `updateWorldTransform` (lines
   ~400–500).
3. `spine-cpp/spine-cpp/include/spine/Bone.h` — field list, in/out
   distinction.
4. `spine-cpp/spine-cpp/src/spine/Bone.cpp` — `updateWorldTransform`
   (lines ~100–300) — the core trig.
5. `spine-cpp/spine-cpp/include/spine/Slot.h` and `Slot.cpp` — simple.

As a secondary reference when the C++ is dense,
`spine-runtimes/spine-ts/spine-core/src/Bone.ts` and `Skeleton.ts` are
often easier to follow.

## Design decisions carried forward from earlier phases

Do not revisit without discussion:

- **SoA + typed indices.** `Skeleton` owns `Vec<Bone>`, `Vec<Slot>`,
  `Vec<IkConstraint>`, etc. No `Rc<RefCell>` anywhere. Cross-references via
  `BoneId` / `SlotId` etc.
- **`Arc<SkeletonData>`.** `Skeleton::new` takes an Arc; many `Skeleton`
  instances can share one `SkeletonData`.
- **Bare f32 world transform fields on Bone**, not `Affine2`. Match
  spine-cpp exactly; convert at the Bevy boundary only.
- **Constraints are stubs in Phase 2.** They exist in the data model and
  the update cache, but their `update()` is a no-op until Phase 5.
- **Events** continue to flow through an out-param `&mut Vec<Event>` once
  animations exist (Phase 3). Not needed in Phase 2.

## Known traps (things to be careful about)

1. **Inherit math reflection handling.** spine-cpp uses
   `parent.a * parent.d - parent.b * parent.c < 0` as a "parent is
   reflected" check. If you paraphrase this check into something that
   looks cleaner, you will introduce bugs. Port verbatim.

2. **Order of operations in `set_to_setup_pose`.** `bones_to_setup_pose`
   must run before `slots_to_setup_pose` (slot attachment names reference
   the setup-pose skin). Constraints reset to their default mix values
   last. spine-cpp has the exact sequence — copy it.

3. **Skin application invalidates the update cache.** `set_skin` should
   trigger `update_cache` because skin-required bones/constraints are
   skin-dependent. Forgetting to invalidate makes skin swaps silently
   use the wrong cache.

4. **`applied_valid` and the "applied local" fields on Bone.** spine-cpp
   keeps `ax, ay, a_rotation, a_scale_x, a_scale_y, a_shear_x, a_shear_y`
   on Bone — these are the local values *as actually applied* after
   animation/constraint passes modify them. The `applied_valid` flag
   tracks whether those cached "applied" values are still fresh. They're
   used by `world_to_local`, `local_to_world`, and by constraints that
   need to read the bone's "truly applied" state mid-frame. Port these
   fields even though they feel redundant; later phases depend on them.

5. **`Skeleton::time` field**. Exists in spine-cpp for physics; should be
   present in Phase 2 even though physics is deferred. Defaults to 0.

## Open questions to resolve at session start

Settle these in the first 10 minutes of Phase 2 before writing any code:

1. **Golden capture: C++ tool or Node + spine-ts?** See memory entry for
   the preferred order (C++ first, Node as fallback). Pick one before
   building 2a.
2. **Scale field on `Skeleton`.** spine-cpp has `_scaleX`, `_scaleY`
   modifiable at runtime (for mirrored skeletons). Include in Phase 2 or
   defer? Recommendation: **include**, it's a single field and affects
   `updateWorldTransform`.
3. **`Skeleton::update(delta)` method.** spine-cpp has this separate from
   `updateWorldTransform` — it advances `_time` for physics simulation.
   Phase 2 should expose the method but only implement time advancement
   (physics is Phase 5). Recommendation: **include**, one-liner.

## Completion criteria

Phase 2 is done when:

- [ ] `tools/spine_capture/` exists and produces `setup_pose.json` for
      every rig in `spine-runtimes/examples/`.
- [ ] `src/skeleton/` module compiles with all methods implemented
      (constraints are still stubs, that's fine).
- [ ] `cargo test --test golden_pose` passes against every captured
      fixture with tolerance `1e-4`.
- [ ] `cargo clippy --all-targets -- -D warnings` clean.
- [ ] `cargo fmt --check` clean.
- [ ] `CLAUDE.md` phase tracker shows `[x] 2` with a one-sentence
      summary of what landed.
- [ ] README phase tracker updated to match.
- [ ] This doc (`docs/PHASE_2_PLAN.md`) removed in the final Phase 2
      commit — it's session scaffolding, not permanent reference.

## Entry points for a fresh session

Minimum reading to bootstrap into Phase 2 from nothing:

1. `CLAUDE.md` (architectural invariants + phase tracker + license rule).
2. `docs/BINARY_FORMAT.md` (format reference if anything binary-related
   comes up).
3. `docs/PHASE_2_PLAN.md` (this file).
4. The five memory entries in `MEMORY.md` (loaded automatically).
5. `src/data/bone.rs` and `src/data/skeleton.rs` (the Phase 1b
   immutable data shape that Phase 2 builds the runtime against).

That's ~2000 lines total of orientation and gets you to "ready to write
2a" without rereading spine-cpp first.
