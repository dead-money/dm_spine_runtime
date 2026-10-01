## Attribution

Do not add `Co-Authored-By: Claude` trailers to commits or "Generated with Claude Code" footers to PR bodies. Author lines and PR bodies stay clean. `scripts/git-hooks/commit-msg` strips these defensively; activate per clone with `git config core.hooksPath scripts/git-hooks`. Do not work around the hook.

## Upstream

`~/deadmoney/spine-runtimes` is read-only, and its working tree is hommlet's patched branch, not upstream. Port from `upstream/4.3` at the commit CI pins, in a worktree. Keep Esoteric's license header at the top of every source file.

## Parity

The goldens (captured from spine-cpp by `tools/spine_capture/`) are the contract. Port math and order of operations literally; never loosen a tolerance to pass.

## Process

Merge PRs with a merge commit, never squash or rebase. `spine_bevy` builds against this repo's same-named branch when one exists, so an API change that breaks it gets a matching branch there.
