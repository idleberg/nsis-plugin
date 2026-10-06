# nsis-plugin

![Crates.io License](https://img.shields.io/crates/l/nsis-plugin?style=for-the-badge)
[![Crates.io Version](https://img.shields.io/crates/v/nsis-plugin?style=for-the-badge)](https://crates.io/crates/nsis-plugin)
[![CI](https://img.shields.io/github/actions/workflow/status/idleberg/nsis-plugin/ci.yml?style=for-the-badge)](https://github.com/idleberg/nsis-plugin/actions)

> Write [NSIS](https://nsis.sourceforge.io/) plug-ins in Rust.

## Description

Build NSIS plug-ins without writing C. The crate takes care of the details that usually go wrong:

- **Long strings:** buffers are sized at runtime, so one DLL works with both stock and long-string NSIS builds.
- **Safe copies:** writes into the installer are bounded. Truncation is reported as an error.
- **Errors:** returning `Err` sets the NSIS error flag, which `IfErrors` can check.
- **NSIS integers:** `0x2a`, `052` and garbage input are parsed the way NSIS parses them.
- **Testable:** test your plug-in on macOS and Linux with a fake installer.

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

## Installation

Generate a new plug-in from the template:

```sh
cargo generate --git https://github.com/idleberg/nsis-plugin template
```

The template asks for an [SPDX license identifier](https://spdx.org/licenses/). Apache-2.0 is the default; other texts are downloaded when you generate.

To add the crate to an existing project instead:

```sh
cargo add nsis-plugin
```

## Usage

### Building

Tooling is managed with [mise](https://mise.jdx.dev/).

```sh
mise install
mise run dist   # build all DLLs into dist/Plugins/
mise run size   # check DLL sizes against their budgets
```

NSIS needs one DLL per installer type:

| Variant         | Installer             |
| --------------- | --------------------- |
| `x86-unicode`   | 32-bit, Unicode       |
| `amd64-unicode` | 64-bit                |
| `x86-ansi`      | 32-bit, ANSI (opt-in) |
| `arm64-unicode` | ARM64 (opt-in)        |

The two opt-in variants are not built unless you ask for them:

```sh
cargo xtask dist --variant x86-ansi
cargo xtask dist --variant arm64-unicode --toolchain msvc
```

To build them every time, add them to `variants` in `dist.toml`.

> [!NOTE]
> NSIS 3 defaults to Unicode, so you only need `x86-ansi` for installers that set `Unicode false`. ARM64 has no NSIS release to test against yet and needs the MSVC toolchain.

### Building on macOS and Linux

Cross-compile with MinGW and test with Wine, no Windows machine required.

```sh
brew install mingw-w64 makensis
mise run dist
```

MSVC is what you should ship with. It runs natively on Windows, or via [cargo-xwin](https://github.com/rust-cross/cargo-xwin) elsewhere.

### Testing

Unit tests run on your host and call the exported function like an installer would.

```rust
#[test]
fn adds_numbers() {
	let mut inst = TestInstaller::stock();
	inst.push("052");   // octal 42
	inst.push("0x2a");  // hex 42
	inst.call(Add);
	assert_eq!(inst.pop().as_deref(), Some("84"));
}
```

Run `mise run smoke` to compile a real installer and run it under Wine. To test with long strings (8192 characters), build a matching `makensis` first:

```sh
mise run nsis:longstring
mise run smoke:long
```

### Windows versions

The MSVC builds only import `KERNEL32`, so they add no minimum Windows version of their own. The 32-bit MinGW builds also need the Universal C Runtime (Windows 10, or the redistributable on older systems).

### DLL size

The template is configured for small DLLs. The `hello` example is 12 KB for `amd64-unicode` and about 50 KB for `x86-unicode` with MinGW. `cargo xtask dist` fails when a DLL exceeds its budget in `dist.toml`.

## Non-goals

This crate does not wrap Win32 (use [`windows-sys`](https://crates.io/crates/windows-sys)), custom pages or `.nsh` wrappers.

## License

This work is licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
