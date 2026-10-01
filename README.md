# nsis-plugin

![Crates.io License](https://img.shields.io/crates/l/nsis-plugin?style=for-the-badge)
[![Crates.io Version](https://img.shields.io/crates/v/nsis-plugin?style=for-the-badge)](https://crates.io/crates/nsis-plugin)
[![CI](https://img.shields.io/github/actions/workflow/status/idleberg/nsis-plugin/ci.yml?style=for-the-badge)](https://github.com/idleberg/nsis-plugin/actions)

Write [NSIS](https://nsis.sourceforge.io/) plug-ins in Rust. A plug-in has to
ship one DLL per installer flavour — `x86-unicode`, `amd64-unicode`, and
optionally `x86-ansi` and `arm64-unicode` — and this crate builds all of
them from one source, correct against long-string installers too. See
[the build matrix](#the-build-matrix).

```rust
#![cfg_attr(target_os = "windows", no_std)]
extern crate alloc;

use nsis_plugin::{Nsis, Result, nsis_fn, nsis_plugin};

nsis_plugin!();

nsis_fn! {
	fn Add(nsis: &mut Nsis) -> Result<()> {
		let b = nsis.stack.pop_int()?;
		let a = nsis.stack.pop_int()?;
		nsis.stack.push_int(a + b)?;
		Ok(())
	}
}
```

```nsis
Push 2
Push 40
example::Add
Pop $0   ; 42
```

## Why

A plug-in export is one C function with five arguments and a trailing ellipsis.
Three facts about it drive everything here:

**`string_size` is a runtime parameter**, the calling installer's
`NSIS_MAX_STRLEN`. Every classic plug-in bug is a variant of `TCHAR buf[1024]`
ignoring it, which is exactly why plug-ins break against long-string builds.
Every buffer holding installer strings is sized from the value the exehead
hands over, so long-string support is structural rather than a feature.

**Stack nodes are `GlobalAlloc`'d and the caller frees on pop.** Pushing a
longer string than `string_size` is a heap overflow. `Stack::push` is bounded
and reports `Error::Truncated`; `Variables::set` bounds a copy that
`setuservariable` leaves unbounded.

**`Result` already means something in NSIS.** Returning `Err` sets
`exec_flags->exec_error`, which is what `IfErrors` reads. `?` on an empty stack
does the NSIS-native thing with no ceremony.

Plug-in logic is unit-testable on macOS and Linux against a fake installer —
the layer C plug-ins have no equivalent for.

## The build matrix

Four artifacts, matching `get_target_suffix` in `Source/build.cpp`. There is no
`amd64-ansi`: 64-bit NSIS targets are always Unicode, so the matrix is four
combinations, not eight.

| Variant         | Rust target (gnu)            | Rust target (msvc)        | Feature   |
| --------------- | ---------------------------- | ------------------------- | --------- |
| `x86-ansi`      | `i686-pc-windows-gnu`        | `i686-pc-windows-msvc`    | `ansi`    |
| `x86-unicode`   | `i686-pc-windows-gnu`        | `i686-pc-windows-msvc`    | `unicode` |
| `amd64-unicode` | `x86_64-pc-windows-gnu`      | `x86_64-pc-windows-msvc`  | `unicode` |
| `arm64-unicode` | `aarch64-pc-windows-gnullvm` | `aarch64-pc-windows-msvc` | `unicode` |

```sh
cargo xtask dist          # build every variant, assemble dist/Plugins/
cargo xtask size          # sizes against their budgets
cargo xtask smoke         # build an installer from examples/hello/test/smoke.nsi
cargo xtask smoke --wine  # …and run it, under Wine, checking its log
```

`smoke` only compiles the test installer; `--wine` is what executes it and
asserts on the log it writes. The runner is Wine, which is how the whole loop
works on macOS and Linux — on Windows, drop the flag and run the produced
`.exe` yourself.

`dist.toml` decides which variants get built, which toolchain is used, and what
each DLL is allowed to weigh.

### x86-ansi and arm64 are opt-in

`x86-ansi` is **not** in the default variant list. NSIS 3 defaults to Unicode,
and an ANSI DLL only serves installers that still say `Unicode false`. It is
still smoke-tested in CI and shipped in releases.

```sh
cargo xtask dist --variant x86-ansi
```

`arm64-unicode` is not in the default list either. NSIS defines
`TARGET_ARM64` and `makensis` knows the suffix, but no arm64 `makensis` or
exehead ships yet, so the DLL can be built and never exercised. Building it also
needs an aarch64-windows linker, which mingw-w64 does not provide — use
`--toolchain msvc`.

```sh
cargo xtask dist --variant arm64-unicode --toolchain msvc
```

Add either to `variants` in `dist.toml` to make it part of every build.

### Building from macOS and Linux

The `*-pc-windows-gnu` targets cross-compile with mingw-w64 from Homebrew or
apt, and `makensis` builds natively on both. That is a complete local
build-and-test loop with no Windows machine and no VM.

```sh
brew install mingw-w64 makensis
mise run dist   # rust-toolchain.toml pulls in the cross targets
mise run smoke
```

MSVC is the release path: native on Windows, or via `cargo-xwin` elsewhere.

### Which Windows versions

Nothing here pins one. There is no `WINVER`, no `/SUBSYSTEM:...,5.01`, and
`dist.toml` selects architectures and character widths, not Windows releases.
The floor falls out of what the artifacts import.

The MSVC builds — the release path — import `KERNEL32` and nothing else, and
every function they use (`GetProcessHeap`, `HeapAlloc`, `MultiByteToWideChar`,
`WideCharToMultiByte`) has been exported since NT 4. The plug-in adds no floor
of its own to the installer that loads it.

The 32-bit `*-pc-windows-gnu` builds are not equivalent. The unwinding
machinery described under [Size](#size) drags in the UCRT, so they also import
`api-ms-win-crt-*` and need Windows 10, or the UCRT redistributable on
anything older. That is one more reason they are the development loop and MSVC
is what ships.

Rust's own statement is separate from either: its tier-1 Windows targets
document Windows 10 as the supported minimum, and older releases are
best-effort even where every import resolves. If you need a guaranteed floor
below that — the reason `x86-ansi` exists at all — nothing in this build
enforces it for you.

## Size

A plug-in DLL is embedded in every installer built with it, so the
[template](template/) ships `opt-level = "z"`,
`lto = "fat"`, `codegen-units = 1`, `panic = "abort"` and `strip = true`, and `nsis_plugin!()` supplies the entry point, a process-
heap allocator and the memory intrinsics itself so the DLL links with
`-nodefaultlibs`. Under MSVC that leaves `KERNEL32` as the only import; the
32-bit GNU builds pick up the UCRT along with the unwinder, as below.

The `hello` example, six exports:

| Variant         | Toolchain | Size  |
| --------------- | --------- | ----- |
| `amd64-unicode` | gnu       | 12 KB |
| `x86-unicode`   | gnu       | 50 KB |
| `x86-ansi`      | gnu       | 51 KB |

The 32-bit GNU figure is a toolchain artifact, not this crate: rustc's
`rsbegin.o` on `i686-pc-windows-gnu` calls `__register_frame_info` for DWARF
unwinding, which pulls in libgcc_eh, the UCRT and winpthreads — around 40 KB
that `panic = "abort"` never uses, and six `api-ms-win-crt-*` imports with it. x86_64 unwinds with SEH and has none of it. MSVC
does not have the problem either, which is why the budgets in `dist.toml` are
per toolchain.

Budgets are enforced: `cargo xtask dist` fails when a DLL grows past its
ceiling.

The DLL also exports `memcpy`, `memset`, `DllMainCRTStartup` and friends. NSIS
resolves plug-in functions by name, so the extra entries are inert.

## Testing

Three layers, because the interesting failures live at different levels.

**Unit tests, host-native.** `nsis_plugin::testing::TestInstaller` builds a real
stack, a real variables array and real exec flags, then calls the _generated
export_ through the raw five-pointer boundary.

```rust
#[test]
fn adds_with_nsis_integer_semantics() {
	let mut inst = TestInstaller::stock();
	inst.push("052");   // octal 42
	inst.push("0x2a");  // hex 42
	inst.call(Add);
	assert_eq!(inst.pop().as_deref(), Some("84"));
}
```

**Integration, under Wine.** `examples/hello/test/smoke.nsi` is compiled by
`makensis` and run silently; the installer writes an assertable log. This is
what catches export-name and calling-convention breakage.

**Matrix conformance.** CI builds every variant with both toolchains, and runs
the same script under Wine for `x86-ansi` and `x86-unicode`, against a stock
`makensis` and a `/DNSIS_MAX_STRLEN=8192` one. `amd64-unicode` is built but not
run. Locally:

```sh
mise run nsis:longstring   # builds one into .nsis-longstring/
mise run smoke:long
```

The long-string run is the point of the project. The smoke test asks the
plug-in for the installer's `string_size` and then round-trips exactly that
many characters, so one script proves the same unmodified DLL is correct at
1023 characters and at 8191.

Note that `makensis` and the exehead stubs must share `NSIS_MAX_STRLEN`;
building only `makensis` and reusing stock stubs produces a mismatched pair.
`mise run nsis:longstring` builds both.

## Character width

`Tchar` is `u16` under the default `unicode` feature and `u8` under `ansi`; the
two are mutually exclusive and a `compile_error!` says so. The public API
traffics in `String`/`&str` and converts at the boundary: UTF-16 in a Unicode
build, `CP_ACP` via `MultiByteToWideChar`/`WideCharToMultiByte` in an ANSI
build, mirroring `pluginapi.c`.

A generic-over-char-type design was considered and rejected: it infects every
signature for the benefit of a configuration chosen once per compilation.

## Integer semantics

`str::parse` is not equivalent to what NSIS does and is not used. `int` is a
line-for-line port of `nsishelper_str_to_ptr` and `myatoi_or`: `0x`/`0X` hex,
leading-zero octal, signed decimal, stopping at the first unrecognised
character, wrapping on overflow, and `0` rather than an error for garbage.
`pop_int_or` handles the `2|4|8` forms. Values are `isize`, which is what
`INT_PTR` is.

## Layout

```
src/                    the crate
examples/hello/         minimal plug-in, doubles as the smoke test
xtask/                  build matrix driver, Plugins/ layout, size budget
template/               cargo-generate template for a new plug-in
dist.toml               what to build, with what, and how big it may be
```

## Starting a plug-in

```sh
cargo generate --git https://github.com/idleberg/nsis-plugin template
```

## Non-goals

Wrapping Win32 broadly (use `windows-sys`), modelling custom pages or the
`WM_NOTIFY_OUTER_NEXT` UI protocol, abstracting over `ExecuteCodeSegment` — it
is an honest passthrough — or generating `.nsh` wrappers. The value is entirely
at the five-pointer boundary.

## Prior art

[`nsis-plugin-api`](https://docs.rs/nsis-plugin-api) from `nsis-tauri-utils` is
the closest existing work: Unicode-only, and it pushes with an unbounded
`lstrcpyW` into a `string_size`-sized allocation, so it is not long-string-safe.
Worth reading for its `no_std` DLL setup.

`Contrib/ExDLL/` in the NSIS tree is the reference C implementation, and the
semantics reproduced here.

## License

MIT
mple.com 'Example title'

Lorem ipsum dolor sit amet, consectetur adipiscing elit.
Curabitur consectetur maximus risus, sed maximus tellus tincidunt et.
