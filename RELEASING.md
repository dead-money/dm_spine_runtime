# Releasing

## CI

`ci.yml` runs on hosted runners for every push to `main` and every PR:

- **`fmt + clippy + test`** also runs `cargo package` and builds the capture
  harness. Tests need upstream `spine-runtimes` at the commit pinned there; bump
  it together with the fixtures and the harness.
- **`doc`** runs `cargo doc` with `-D warnings`.

`rust-version` tracks recent stable Rust. Bump it rather than avoiding newer
features.

## Cutting a release

Requires [cargo-release](https://github.com/crate-ci/cargo-release). With
`main` clean and CI green:

```sh
cargo release minor --dry-run
cargo release minor --execute
```

It bumps the version, stamps `CHANGELOG.md`, commits, tags `vX.Y.Z`, and
pushes. The tag triggers `release.yml`, which tests against the pinned
`spine-runtimes` and publishes through crates.io Trusted Publishing. Afterward,
check the crate page and the docs.rs build.

Keep `## [Unreleased]` in `CHANGELOG.md` current as PRs land.
