//! Link args that keep the DLL small. A plug-in is embedded in every installer
//! built with it, so the C runtime is not worth its weight: `nsis_plugin!()`
//! supplies the entry point, the allocator and the memory intrinsics itself.
//!
//! Removing this file is safe — the DLL just gets bigger.
//!
//! These belong here rather than in `.cargo/config.toml`. A `[target.<triple>]`
//! table also applies to host units — build scripts and proc macros — whenever
//! cargo is invoked without `--target` and the host triple matches, which on a
//! Windows host means every build script links with `/NODEFAULTLIB` and fails.
//! `rustc-link-arg-cdylib` applies to this crate's cdylib and nothing else.

fn main() {
	println!("cargo::rerun-if-changed=build.rs");

	// `CARGO_CFG_TARGET_*` describe the crate being built, not the host.
	if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
		return;
	}

	let args: &[&str] = match std::env::var("CARGO_CFG_TARGET_ENV").as_deref() {
		Ok("msvc") => &[
			"/NODEFAULTLIB",
			"/ENTRY:DllMainCRTStartup",
			"/OPT:REF",
			"/OPT:ICF",
		],
		// gnu and gnullvm both drive the linker through gcc/clang.
		_ => &["-nostartfiles", "-nodefaultlibs"],
	};

	for arg in args {
		println!("cargo::rustc-link-arg-cdylib={arg}");
	}

	// rustc puts `/SAFESEH` in the pre-link args for `i686-pc-windows-msvc`.
	// Emitting the load config that implies needs `_load_config_used`, which
	// lives in the CRT that `/NODEFAULTLIB` just removed. There are no SEH
	// handlers to register in a `panic = "abort"` DLL, so opt out; ours is the
	// later flag and wins.
	if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
		&& std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("x86")
	{
		println!("cargo::rustc-link-arg-cdylib=/SAFESEH:NO");
	}
}
