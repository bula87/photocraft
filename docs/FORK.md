# Fork maintenance

Upstream: <https://github.com/storytold/photocraft> (`MIT OR Apache-2.0`, see `LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE`, `ATTRIBUTION.md`).

This fork's base is upstream `main` at the commit recorded by `git merge-base HEAD` against `upstream/main` after a sync (the import used for the first portrait-filter commit is `37a01ca8746c121170644a07968d11263ef64407`, which is `v0.20.1` plus later upstream commits). Tags from `v0.1.0` through `v0.20.1` are in the history.

## What this fork changes

Only the portrait-filter host:

| Path | Why |
|---|---|
| `crates/portrait-filters/` | Scan, menu state, blend contract, optional ONNX Runtime |
| `crates/engine/src/cmd/portrait_filter_cmds.rs` | Filter commands and new-layer placement |
| `crates/engine/src/cmd.rs` | Register those commands |
| `crates/engine/Cargo.toml`, `Cargo.toml` | Depend on the crate; `portrait-onnx` feature |
| `crates/engine/src/prefs.rs` | `plugIns.portraitFiltersFolder` |
| `crates/ui-egui/src/prefs/prefs_ui/fields.rs` | Preferences label |
| `crates/automation/src/workspace.rs` | Untrusted automation cannot read the package directory |
| `apps/photocraft/Cargo.toml`, `apps/photocraft-cli/Cargo.toml` | Pass `portrait-onnx` through |
| `xtask/src/layers.rs` | The new crate is layer 4 |
| `docs/PORTRAIT_FILTERS.md`, `docs/FORK.md` | Contract and this note |
| `README.md` | Fork banner between `portrait-fork-banner` comments |
| `ATTRIBUTION.md` | The synthetic ONNX test graphs |
| `.github/workflows/portrait-filters.yml` | Fork CI for the ML and non-ML paths |
| `Cargo.lock` | Lockfile for the new crate |

No filter weights, no training code, and no copy of photocraft-retouch.

## Sync upstream

```sh
git remote add upstream https://github.com/storytold/photocraft.git   # once
git fetch upstream
git merge upstream/main
```

Keep the README block between `<!-- portrait-fork-banner:begin -->` and `<!-- portrait-fork-banner:end -->`. Everything else in the README is upstream's. Re-run:

```sh
cargo test -p photocraft-portrait-filters
cargo test -p photocraft-engine portrait_filter --lib
cargo test -p photocraft-portrait-filters --features onnx
```

## Sending the host upstream

The patch to propose is the table above **without** the fork banner and without `.github/workflows/portrait-filters.yml`. Upstream CI is shared (`storytold/craft-common-actions`) and feature PRs are not supposed to add workflow jobs; ask in the PR whether the `onnx` tests should live there.

The host is one crate plus a command module so the diff stays reviewable. `pc-retouch apply` can stay the story for people who do not want ONNX Runtime inside PhotoCraft. This menu is the optional in-app host for the same packages.

## Publishing this fork to GitHub

This checkout's `origin` is the Origin remote for the new project. To also publish under a GitHub name you own:

```sh
git remote add github git@github.com:<you>/photocraft.git
git push -u github main --tags
```

Use a name that says it is a fork (for example `photocraft`). Do not force-push over upstream. The retouch packages stay in their own repository.
