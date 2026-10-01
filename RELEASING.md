# Releasing

How `spine_runtime` gets to crates.io. **It isn't cleared to publish yet.**

## Rust version

`rust-version` in `Cargo.toml` tracks recent stable Rust; we don't hold the code
to older compilers. Bump it when the crate starts using something newer.

## The gate

`publish = false` in `Cargo.toml` blocks every path to crates.io: `cargo
publish` refuses, and so does the tag-triggered `release.yml`. Flip it only
once Esoteric Software has agreed to a spine-cpp derivative being published
there. Their written OK, and any conditions on it, belong in this file.

Everything else is ready: the manifest, the package contents, CI's `cargo
package` and `doc` checks, and the release workflow.

## CI

`ci.yml` runs on every push to `main` and every PR, all on hosted runners:

- **fmt + clippy + test**: also runs `cargo package` (a verification build of
  the exact tarball) and builds the capture harness. Tests need upstream
  `spine-runtimes` at the commit pinned there; bump it together with the
  fixtures and the harness.
- **doc**: `cargo doc` with `-D warnings`, so docs.rs links don't break.

The published crate leaves out `tests/` and `tools/`, which need a sibling
`spine-runtimes` checkout and can't run from crates.io.

## First release (manual)

crates.io can't set up Trusted Publishing for a crate that doesn't exist yet,
so the first version is published by hand.

1. On a branch, remove `publish = false` from `Cargo.toml` and stamp
   `CHANGELOG.md`: under `## [Unreleased]`, add `## [0.2.0] - <date>` above the
   notes. Merge it.
2. From a clean `main` with CI green:

   ```sh
   cargo publish --dry-run
   CARGO_REGISTRY_TOKEN=<token> cargo publish
   git tag v0.2.0 && git push origin v0.2.0
   ```

   Create the token at crates.io → Account → API Tokens with the
   `publish-new` scope, and revoke it afterward. `release.yml` runs on the tag,
   sees 0.2.0 is already published, and skips.
3. On crates.io → `spine_runtime` → Settings → Trusted Publishing, add a GitHub
   publisher: repository `dead-money/spine_runtime`, workflow `release.yml`,
   environment blank. Later releases use GitHub's OIDC identity, so there's no
   token to store.

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
