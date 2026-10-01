# nsis-plugin

A Rust SDK for writing NSIS plug-ins. All the value is at the five-pointer
boundary between the exehead and a plug-in DLL; everything past it is somebody
else's solved problem. See `README.md` for the design rationale and the
user-facing story.

## Project Structure

- `src/raw.rs` — FFI declarations transcribed from
  `Source/exehead/api.h` and `Contrib/ExDLL/pluginapi.h`. Field order is ABI.
- `src/stack.rs` — the `stack_t **` protocol
- `src/vars.rs` — the 25 `INST_*` user variables
- `src/sys.rs` — `GlobalAlloc`/`GlobalFree` for stack
  nodes, emulated on the host
- `src/error.rs` — `Error`, whose `Err` becomes
  `exec_error`
- `src/int.rs` — NSIS integer semantics, ported from
  `pluginapi.c`
- `src/tchar.rs` — the `Tchar` width abstraction and
  `CP_ACP` conversion
- `src/rt.rs` — allocator, panic path, memory intrinsics;
  everything a `no_std` cdylib needs to link
- `src/macros.rs` — `nsis_plugin!`, `nsis_fn!`,
  `nsis_unload!`
- `src/testing.rs` — `TestInstaller`, the fake installer
- `examples/hello/` — minimal plug-in and `test/smoke.nsi`
- `xtask/` — build matrix driver
- `dist.toml` — variants, toolchain and per-variant size budgets
- `template/` — `cargo-generate` template for a new plug-in; standalone, with
  its own `mise.toml`. CI generates it and builds it against this crate
  (`.github/workflows/template.yml`)

## Tooling

Everything runs through [mise](https://mise.jdx.dev/):

```
mise run checks           # format:check + lint + test
mise run format           # cargo fmt --all
mise run lint             # clippy, both character widths + the Windows target
mise run test             # host tests, both character widths
mise run targets          # show the build matrix
mise run dist             # build every variant into dist/Plugins/
mise run size             # DLL sizes against their budgets
mise run smoke            # build an installer and run it under Wine
mise run nsis:longstring  # build a /DNSIS_MAX_STRLEN=8192 makensis
mise run smoke:long       # smoke test against that build
```

Pre-commit hooks are managed by hk.

## Ground truth

The NSIS sources are the authority for anything about the ABI or the plug-in
protocol. `makensis -HDRINFO` reports `NSISDIR`, the root of the installed
NSIS; the paths below are relative to a source checkout.

- `Source/exehead/api.h` — `exec_flags_t`, `extra_parameters`, `NSPIM`
- `Contrib/ExDLL/pluginapi.h` — `stack_t`, the `INST_*` enum
- `Contrib/ExDLL/pluginapi.c` — the semantics reproduced here, exactly
- `Source/build.h` / `Source/build.cpp` — `TARGETTYPE` and
  `get_target_suffix`, i.e. the four variants

Never reason from memory about these. Read the file. When adding or changing
anything that touches the boundary, cite the file and the symbol (struct,
function, macro) in a comment.

`makensis -CMDHELP <command>` is the authority for script syntax.

## Invariants

These are the reasons the crate exists. Breaking one is a defect, not a
regression in style.

- **Nothing is sized from a constant.** Every buffer comes from the runtime
  `string_size` the exehead passed. If you write a fixed-size array holding
  installer strings, you have reintroduced the bug.
- **Every copy into installer memory is bounded**, with `lstrcpyn` semantics:
  at most `string_size - 1` characters plus a NUL. Truncation is reported, not
  silent, and the truncated value is still written — matching C.
- **`Err` maps to `exec_flags->exec_error`.** That is the whole error design.
- **Nothing unwinds across the boundary.** `nsis_fn!` bodies cannot let `?`
  escape, and the panic handler terminates.
- **Integer parsing is NSIS's, not Rust's.** Unparseable input is `0`, not an
  error.

## Testing Requirements

- Every new feature and bugfix needs a test. Prefer `TestInstaller::call`,
  which exercises the generated export rather than the body.
- Test at the boundary: `string_size - 1` fits, `string_size` truncates.
- Boundary tests cover a small size, `TestInstaller::stock()` (1024) and
  `long_string()` (8192). The small size catches a hardcoded 1024; the
  other two are the sizes real installers pass.
- Run `mise run checks`. Before touching anything at the FFI boundary, also run
  `mise run smoke` and `mise run smoke:long`.
- Editing existing tests always requires user confirmation.

## Code Style

- Rust edition 2024
- Indentation: tabs (see `.editorconfig`)
- `#![warn(missing_docs)]` — public items need doc comments
- `clippy::undocumented_unsafe_blocks` is on. Modules that are
  uniformly unsafe allow it once, at module level, with a reason; everywhere
  else write the `SAFETY:` comment.
- Comments explain why, not what. The NSIS ABI has a lot of "why".
