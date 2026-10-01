//! The four NSIS build variants, and the Rust targets they map to.
//!
//! The names match `CEXEBuild::get_target_suffix` in `Source/build.cpp`, which
//! is also what NSIS looks for under `Plugins/`. There is no `amd64-ansi`:
//! `Source/build.h` defines exactly four targets and the 64-bit ones are always
//! Unicode, so the matrix is 4 combinations, not 8.

use std::fmt;
use std::str::FromStr;

use serde::Deserialize;

/// Which Windows toolchain to build with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Toolchain {
	/// mingw-w64. Cross-compiles from macOS and Linux; the local loop.
	Gnu,
	/// Native on Windows, `cargo-xwin` elsewhere; the release artifacts.
	Msvc,
}

impl Toolchain {
	/// The name used on the command line and as the `[size-budget.*]` key.
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Gnu => "gnu",
			Self::Msvc => "msvc",
		}
	}
}

impl FromStr for Toolchain {
	type Err = String;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		[Self::Gnu, Self::Msvc]
			.into_iter()
			.find(|t| t.as_str() == s)
			.ok_or_else(|| format!("unknown toolchain `{s}`; expected `gnu` or `msvc`"))
	}
}

/// One of the four NSIS plug-in build variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Variant {
	/// 32-bit, ANSI. The legacy path; NSIS 3 defaults to Unicode.
	X86Ansi,
	/// 32-bit, Unicode.
	X86Unicode,
	/// 64-bit x86. Always Unicode.
	Amd64Unicode,
	/// 64-bit ARM. Always Unicode, and see the note in `dist.toml`.
	Arm64Unicode,
}

impl Variant {
	/// Every variant, in `TARGETTYPE` order.
	pub const ALL: [Variant; 4] = [
		Variant::X86Ansi,
		Variant::X86Unicode,
		Variant::Amd64Unicode,
		Variant::Arm64Unicode,
	];

	/// The Rust target triple for this variant.
	///
	/// arm64 has no mingw-w64 toolchain, so the GNU column uses `gnullvm`,
	/// which needs an LLVM mingw install. In practice arm64 wants `msvc`.
	pub fn rust_target(self, toolchain: Toolchain) -> &'static str {
		match (self, toolchain) {
			(Self::X86Ansi | Self::X86Unicode, Toolchain::Gnu) => "i686-pc-windows-gnu",
			(Self::X86Ansi | Self::X86Unicode, Toolchain::Msvc) => "i686-pc-windows-msvc",
			(Self::Amd64Unicode, Toolchain::Gnu) => "x86_64-pc-windows-gnu",
			(Self::Amd64Unicode, Toolchain::Msvc) => "x86_64-pc-windows-msvc",
			(Self::Arm64Unicode, Toolchain::Gnu) => "aarch64-pc-windows-gnullvm",
			(Self::Arm64Unicode, Toolchain::Msvc) => "aarch64-pc-windows-msvc",
		}
	}

	/// The cargo feature selecting this variant's character width.
	pub fn feature(self) -> &'static str {
		match self {
			Self::X86Ansi => "ansi",
			_ => "unicode",
		}
	}

	/// The argument for makensis's `Target` command.
	pub fn nsis_target(self) -> &'static str {
		match self {
			Self::X86Ansi => "x86-ansi",
			Self::X86Unicode => "x86-unicode",
			Self::Amd64Unicode => "amd64-unicode",
			Self::Arm64Unicode => "arm64-unicode",
		}
	}
}

impl fmt::Display for Variant {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.pad(self.nsis_target())
	}
}

impl FromStr for Variant {
	type Err = String;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		Self::ALL
			.into_iter()
			.find(|v| v.nsis_target() == s)
			.ok_or_else(|| {
				let names: Vec<_> = Self::ALL.iter().map(|v| v.nsis_target()).collect();
				format!(
					"unknown variant `{s}`; expected one of {}",
					names.join(", ")
				)
			})
	}
}
