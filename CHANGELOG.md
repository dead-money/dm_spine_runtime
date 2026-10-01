# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). While the crate is
pre-1.0, any `0.x` release may contain breaking changes.

## [Unreleased]

## [0.2.2] - 2026-10-01

### Changed

- Expanded the API docs for docs.rs: module overviews, a crate-level starting
  point, and contracts on the public types and methods.

## [0.2.1] - 2026-10-01

### Fixed

- Animation durations now count the last key of deform timelines. Animations
  ending on a deform key loaded one frame short and looped early.

### Changed

- Corrected and tightened the API docs and comments throughout.

## [0.2.0] - 2026-10-01

### Changed

- **Breaking:** targets Spine 4.3. Binary and JSON loaders accept only 4.3
  exports; for 4.2, use the `v0.1.0` tag.
- **Breaking:** the 4.3 pose model. Bones, slots, and constraints carry a pose
  and an applied pose, constraints live in one ordered list addressed by
  `ConstraintId`, and the 4.3 API renames (`setup_pose`, `AnimationState::track`,
  skin placeholders) are followed without aliases.
- The crate is renamed from `dm_spine_runtime` to `spine_runtime`.

### Added

- Slider constraint and the rewritten 4.3 transform constraint.
- Runtime skins: `Arc<Skin>` tables over `(slot, placeholder)` keys, with owned
  attachment copies (`copy`, `set_region`, `tag`), `add_skin`, `copy_skin`,
  `Skeleton::skin_mut`, and `compact_skin`.
- `RenderOptions` per-vertex slot and tag streams and merging across colors;
  `RegionGeometry` for mapping region points.
- `AnimationState::pose`, an event-free re-apply of the current tracks.
- `AnimationState::apply_events`, an apply that fires events and advances
  track bookkeeping without posing a skeleton.

## [0.1.0] - 2026-04-23

Native Rust port of the Spine 4.2 runtime. Not published to crates.io; tagged
on GitHub.
