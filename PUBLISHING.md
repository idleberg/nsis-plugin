# Publishing `nsis-plugin` to crates.io

Notes from an assessment on 2026-08-29. Nothing here has been acted on; this is
the state of the question, not a decision.

## Short answer

Yes, the crate can be published on its own. It is already shaped for
it: full publish metadata, no path dependencies, and nothing in `src/` reads a
file from the repo. One blocker is real (see below); the rest is judgement.

Since the crate moved to the repo root, `include` limits the package to `src/`,
`README.md` and `LICENSE`, and CI runs `cargo publish --dry-run` on every PR.

## Blockers

- ~~**`readme = "README.md"` points at a file that does not exist**~~ — fixed by
  moving the crate to the repo root, where `README.md` lives.

## The forcing reason

`template/Cargo.toml` already declares:

```toml
nsis-plugin = { version = "0.1", default-features = false, features = ["mem-intrinsics"] }
```

That is a crates.io dependency, not a path one. **The `cargo-generate` template
cannot build for anyone until the crate is published.** This is less a
trade-off than an outstanding obligation.

## Pros

- The SDK is the reusable half. The rest of the repo — `xtask`, `dist.toml`,
  the mise tasks, the Wine smoke tests — is the build harness for *this* repo,
  not something a consumer needs. Publishing draws that line cleanly.
- docs.rs gives the boundary documentation a stable, browsable URL. For a crate
  whose whole value proposition is "here is the exehead ABI, done correctly,"
  discoverable docs are most of the pitch. The `[package.metadata.docs.rs]`
  target list is already set.
- Consumers write `nsis-plugin = "0.1"` instead of pinning a git revision.
- The feature flags (`unicode`/`ansi`, `mem-intrinsics`, `std`, `testing`)
  become the public contract people select against.
- Semver becomes a forcing function on the FFI surface — which is exactly where
  a forcing function is wanted.

## Cons

- **The invariants become promises to strangers.** Today a bad `Tchar` change is
  caught by `mise run smoke`. Published, it breaks someone's installer and the
  cost is a yank plus a patch release.
- **Testing asymmetry.** Consumers get `--features testing` and `TestInstaller`,
  but not `mise run smoke` / `smoke:long`. The crate cannot ship the end-to-end
  verification that is the actual source of confidence in it; downstream users
  are trusting CI they cannot run.
- **The size budgets do not travel.** `dist.toml` enforces them here only. A
  consumer whose profile lacks `opt-level = "z"`, `lto = "fat"` and
  `panic = "abort"` gets a substantially larger DLL and will reasonably blame
  the crate. The required release profile needs to be documented prominently,
  not just present in the workspace root and the template.
- **`examples/hello` is the real usage documentation and lives outside the
  published crate.** docs.rs readers get no runnable example without clicking
  through to GitHub.
- **arm64-unicode is buildable but not testable** — NSIS ships no arm64
  `makensis` or exehead yet (see the comment in `dist.toml`). Publishing means
  shipping with that gap stated rather than merely known.

## Suggested shape, if it goes ahead

1. ~~Fix the `readme` blocker~~ (done); confirm with
   `cargo package -p nsis-plugin --list` that the file set is what is intended.
2. Add a usage example to the crate-level docs mirroring `examples/hello`, so
   the docs.rs story stands on its own.
3. Document the required release profile in the crate-level docs, not only in
   the workspace `Cargo.toml`.
4. Leave `xtask`, `dist.toml` and the mise tasks in the repo, unpublished.
5. Run `mise run checks`, `mise run smoke` and `mise run smoke:long` before the
   first `cargo publish`, per the FFI-boundary rule in `CLAUDE.md`.
