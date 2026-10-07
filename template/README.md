# {{project-name}}

{{plugin_description}}

Built with [nsis-plugin](https://crates.io/crates/nsis-plugin).

## Stack contract

Document what each export pops and pushes — this is the plug-in's real API, and
nothing in the Rust signature conveys it.

### `{{crate_name}}::Hello`

| | |
|---|---|
| **Pops** | the name to greet |
| **Pushes** | `Hello, <name>!` |
| **Error flag** | set when the stack is empty |

```nsis
Push "world"
{{crate_name}}::Hello
Pop $0   ; Hello, world!
```

## Building

```sh
mise run dist   # build every variant into dist/Plugins/
mise run size   # check the size budget
```

The Windows cross-targets come from `rust-toolchain.toml`, so rustup installs
them on the first build.

`dist/Plugins/` is laid out the way NSIS expects, so it can be dropped into an
NSIS installation or pointed at with `!addplugindir`.

The default variants are `x86-unicode` and `amd64-unicode`. `x86-ansi` is
opt-in, for installers that still say `Unicode false`:

```sh
VARIANTS="x86-ansi x86-unicode amd64-unicode" mise run dist
```

`arm64-unicode` is opt-in too — NSIS ships no arm64 `makensis` or exehead yet, so
the DLL can be built but not exercised, and building it needs an
aarch64-windows linker that mingw-w64 does not provide:

```sh
VARIANTS="arm64-unicode" TOOLCHAIN=msvc mise run dist
```

Cross-compiling from macOS or Linux needs mingw-w64; the MSVC toolchain is
native on Windows, or `cargo-xwin` elsewhere.

```sh
TOOLCHAIN=msvc mise run dist
```

## Releasing

The source lives in `Contrib/{{crate_name}}/`, the NSISDIR layout that
[nsis-dev/release-package](https://github.com/nsis-dev/release-package) expects.
Publishing a GitHub release runs `.github/workflows/release.yml`, which builds
the MSVC DLLs and attaches a zip, an installer and `SHA256SUMS` to the release.

## Testing

```sh
mise run test    # host-native, no Windows and no installer needed
mise run smoke   # build an installer with makensis and run it under Wine
```

Unit tests call the *generated export* through the raw five-pointer boundary
rather than the body directly, so the calling convention and the
`Result`-to-error-flag mapping are covered too.

Test against a long-string installer as well. `string_size` is a runtime
parameter, and a plug-in that assumes 1024 breaks against a `makensis` built
with `/DNSIS_MAX_STRLEN=8192`:

```rust
let mut inst = TestInstaller::long_string();
```

## Size

The release profile is tuned for size because this DLL is embedded in every
installer built with it. `mise run size` fails past `SIZE_BUDGET` (65536 bytes
by default).

32-bit GNU builds are legitimately larger than 64-bit ones: rustc's
`rsbegin.o` on `i686-pc-windows-gnu` pulls in libgcc_eh, the UCRT and
winpthreads for DWARF unwinding that `panic = "abort"` never uses. x86_64 and
all the MSVC targets do not have the problem. Set a tighter `SIZE_BUDGET` for
the release artifacts.

## License

This work is licensed under {{license}}.
