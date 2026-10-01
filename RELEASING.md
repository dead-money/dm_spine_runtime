# Releasing

How `spine_runtime` gets to crates.io.

## Rust version

`rust-version` in `Cargo.toml` tracks recent stable Rust; we don't hold the code
to older compilers. Bump it when the crate starts using something newer.

## Licensing

Cleared for crates.io on 2026-10-01, alongside the other Spine runtime ports
already published there. Every release ships `LICENSE` (the Spine Runtimes
License verbatim) and the license header in each source file. Developers who
build with the crate need a Spine Editor license; players of their games don't.

## CI

`ci.yml` runs on every push to `main` and every PR, all on hosted runners:

- **fmt + clippy + test**: also runs `cargo package` (a verification build of
  the exact tarball) and builds the capture harness. Tests need upstream
  `spine-runtimes` at the commit pinned there; bump it together with the
  fixtures and the harness.
- **doc**: `cargo doc` with `-D warnings`, so docs.rs links don't break.

The published crate leaves out `tests/` and `tools/`, which need a sibling
`spine-runtimes` checkout and can't run from crates.io.

## First release

crates.io can't set up Trusted Publishing for a crate that doesn't exist yet,
so 0.2.0 was published by hand with a `publish-new` API token, then tagged
`v0.2.0`. `release.yml` ran on the tag, saw 0.2.0 already published, and
skipped.

Trusted Publishing is configured on crates.io → `spine_runtime` → Settings →
Trusted Publishing: repository `dead-money/spine_runtime`, workflow
`release.yml`, environment blank. Later releases use GitHub's OIDC identity, so
there's no token to store.

## Later releases

Install [cargo-release](https://github.com/crate-ci/cargo-release) (`cargo
install cargo-release`). With `main` clean and CI green:

```sh
cargo release minor --dry-run
cargo release minor --execute
```

Per `release.toml`, it bumps the version, stamps `CHANGELOG.md`, commits, tags
`vX.Y.Z`, and pushes. The tag triggers `release.yml`, which runs the test suite
against the pinned `spine-runtimes`, authenticates through Trusted Publishing,
and publishes. Afterward, check the crate page and the docs.rs build.

Keep `## [Unreleased]` in `CHANGELOG.md` current as PRs land. The release
stamps whatever is there.
