//! `dist.toml`, the workspace's build-matrix configuration.

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::variant::{Toolchain, Variant};

/// The whole of `dist.toml`.
#[derive(Debug, Deserialize)]
pub struct Config {
	/// What to build and where to put it.
	pub dist: Dist,
	/// DLL size ceilings in bytes, keyed by toolchain and then by variant.
	///
	/// Budgets are per toolchain because the toolchains genuinely differ: see
	/// the comment in `dist.toml`.
	#[serde(default, rename = "size-budget")]
	pub size_budget: BTreeMap<String, BTreeMap<String, u64>>,
	/// Per-package ceilings, keyed by package, toolchain and variant. A
	/// package listed here uses only these: a `std` plug-in is a different
	/// size class, and falling back to the shared `no_std` ceiling would only
	/// ever fail.
	#[serde(default, rename = "package-size-budget")]
	pub package_size_budget: BTreeMap<String, BTreeMap<String, BTreeMap<String, u64>>>,
}

impl Config {
	/// The size ceiling for one build, if `dist.toml` sets one.
	pub fn budget(&self, package: &str, toolchain: Toolchain, variant: Variant) -> Option<u64> {
		self.package_size_budget
			.get(package)
			.unwrap_or(&self.size_budget)
			.get(toolchain.as_str())?
			.get(variant.nsis_target())
			.copied()
	}
}

/// The `[dist]` table.
#[derive(Debug, Deserialize)]
pub struct Dist {
	/// Cargo packages to build; each produces one DLL per variant.
	pub packages: Vec<String>,
	/// Which Windows toolchain to use by default.
	#[serde(default = "default_toolchain")]
	pub toolchain: Toolchain,
	/// Where the `Plugins/` tree is assembled.
	#[serde(default = "default_out_dir", rename = "out-dir")]
	pub out_dir: String,
	/// Variants built when `--variant` is not given.
	pub variants: Vec<String>,
}

fn default_toolchain() -> Toolchain {
	Toolchain::Gnu
}

fn default_out_dir() -> String {
	"dist".into()
}

impl Config {
	/// Reads `dist.toml` from the workspace root.
	pub fn load(root: &Path) -> Result<Self, Box<dyn Error>> {
		let path = root.join("dist.toml");
		let text = fs::read_to_string(&path)
			.map_err(|e| format!("could not read {}: {e}", path.display()))?;
		let config: Config = toml::from_str(&text)
			.map_err(|e| format!("could not parse {}: {e}", path.display()))?;
		Ok(config)
	}
}
