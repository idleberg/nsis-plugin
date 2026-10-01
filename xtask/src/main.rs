//! Build matrix driver: builds every NSIS plug-in variant and assembles the
//! `Plugins/` tree ready to drop into an NSIS installation or an
//! `!addplugindir` path.
//!
//! Configured by `dist.toml` at the workspace root. Run through the alias in
//! `.cargo/config.toml`:
//!
//! ```text
//! cargo xtask dist
//! cargo xtask dist --variant arm64-unicode
//! cargo xtask dist --toolchain msvc
//! cargo xtask size
//! cargo xtask smoke
//! ```

mod config;
mod variant;

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use config::Config;
use variant::{Toolchain, Variant};

type Res<T> = Result<T, Box<dyn Error>>;

const USAGE: &str = "\
cargo xtask — nsis-plugin build matrix

USAGE:
    cargo xtask dist  [--variant <name>]... [--toolchain gnu|msvc]
                      [--package <name>]... [--no-size-check]
    cargo xtask size  [--toolchain gnu|msvc]
    cargo xtask smoke [--variant <name>] [--wine] [--script <path.nsi>]
    cargo xtask targets

VARIANTS
    x86-ansi  x86-unicode  amd64-unicode  arm64-unicode

    Variants default to the `variants` list in dist.toml. x86-ansi is not in
    that list: NSIS 3 defaults to Unicode. Neither is arm64-unicode: NSIS ships
    no arm64 makensis or exehead yet, so the DLL can be built but not exercised.
    Pass --variant to build either anyway.

    A package without the variant's feature (an ANSI build of a Unicode-only
    plug-in, say) is skipped for that variant.

    smoke builds examples/hello/test/smoke.nsi unless --script names another.
";

fn main() {
	if let Err(err) = run() {
		eprintln!("error: {err}");
		std::process::exit(1);
	}
}

fn run() -> Res<()> {
	let args: Vec<String> = std::env::args().skip(1).collect();
	let (command, rest) = args
		.split_first()
		.map_or(("help", &[][..]), |(c, r)| (c.as_str(), r));

	match command {
		"dist" => dist(Args::parse(rest)?),
		"size" => size(Args::parse(rest)?),
		"smoke" => smoke(Args::parse(rest)?),
		"targets" => targets(),
		"help" | "--help" | "-h" => {
			print!("{USAGE}");
			Ok(())
		}
		other => Err(format!("unknown command `{other}`\n\n{USAGE}").into()),
	}
}

/// Parsed command line, kept deliberately small — this is a build script, not a
/// CLI, and a dependency on clap would be one more thing to keep current.
#[derive(Default)]
struct Args {
	variants: Vec<String>,
	packages: Vec<String>,
	toolchain: Option<Toolchain>,
	no_size_check: bool,
	wine: bool,
	script: Option<String>,
}

impl Args {
	fn parse(args: &[String]) -> Res<Self> {
		let mut out = Self::default();
		let mut it = args.iter();
		while let Some(arg) = it.next() {
			match arg.as_str() {
				"--variant" => out
					.variants
					.push(it.next().ok_or("--variant needs a value")?.clone()),
				"--package" | "-p" => out
					.packages
					.push(it.next().ok_or("--package needs a value")?.clone()),
				"--toolchain" => {
					let value = it.next().ok_or("--toolchain needs a value")?;
					out.toolchain = Some(value.parse()?);
				}
				"--no-size-check" => out.no_size_check = true,
				"--wine" => out.wine = true,
				"--script" => {
					out.script = Some(it.next().ok_or("--script needs a value")?.clone());
				}
				other => return Err(format!("unknown flag `{other}`\n\n{USAGE}").into()),
			}
		}
		Ok(out)
	}

	/// Toolchain to build with: `--toolchain` wins, otherwise `dist.toml`.
	fn toolchain(&self, config: &Config) -> Toolchain {
		self.toolchain.unwrap_or(config.dist.toolchain)
	}

	/// Variants to build: explicit `--variant` flags win, otherwise `dist.toml`.
	fn resolve_variants(&self, config: &Config) -> Res<Vec<Variant>> {
		let names = if self.variants.is_empty() {
			&config.dist.variants
		} else {
			&self.variants
		};
		names
			.iter()
			.map(|n| n.parse::<Variant>().map_err(Box::<dyn Error>::from))
			.collect()
	}
}

/// One DLL copied into `Plugins/`.
struct Built {
	package: String,
	name: String,
	bytes: u64,
}

/// A plug-in package, as `cargo metadata` describes it.
struct Plugin {
	package: String,
	/// The `[lib]` name, which is the DLL name and the script namespace. Not
	/// necessarily the package name.
	lib: String,
	features: Vec<String>,
}

impl Plugin {
	fn supports(&self, variant: Variant) -> bool {
		self.features.iter().any(|f| f == variant.feature())
	}
}

/// Looks up `packages` in the workspace manifest.
fn plugins(root: &Path, packages: &[String]) -> Res<Vec<Plugin>> {
	#[derive(serde::Deserialize)]
	struct Metadata {
		packages: Vec<Package>,
	}
	#[derive(serde::Deserialize)]
	struct Package {
		name: String,
		targets: Vec<Target>,
		features: BTreeMap<String, Vec<String>>,
	}
	#[derive(serde::Deserialize)]
	struct Target {
		name: String,
		kind: Vec<String>,
	}

	let output = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
		.current_dir(root)
		.args(["metadata", "--no-deps", "--format-version", "1"])
		.output()?;
	if !output.status.success() {
		return Err("`cargo metadata` failed".into());
	}
	let metadata: Metadata = serde_json::from_slice(&output.stdout)?;

	packages
		.iter()
		.map(|name| {
			let package = metadata
				.packages
				.iter()
				.find(|p| &p.name == name)
				.ok_or_else(|| format!("no package `{name}` in the workspace"))?;
			let lib = package
				.targets
				.iter()
				.find(|t| t.kind.iter().any(|k| k == "cdylib"))
				.ok_or_else(|| format!("`{name}` has no crate-type = [\"cdylib\"] target"))?;
			Ok(Plugin {
				package: name.clone(),
				lib: lib.name.replace('-', "_"),
				features: package.features.keys().cloned().collect(),
			})
		})
		.collect()
}

/// The workspace root and its `dist.toml`, which every command starts from.
fn load() -> Res<(PathBuf, Config)> {
	// xtask always lives one level below the workspace root.
	let root = Path::new(env!("CARGO_MANIFEST_DIR"))
		.parent()
		.expect("xtask has a parent directory")
		.to_path_buf();
	let config = Config::load(&root)?;
	Ok((root, config))
}

fn plugins_dir(root: &Path, config: &Config, variant: Variant) -> PathBuf {
	root.join(&config.dist.out_dir)
		.join("Plugins")
		.join(variant.to_string())
}

fn dist(args: Args) -> Res<()> {
	let (root, config) = load()?;
	let variants = args.resolve_variants(&config)?;
	let toolchain = args.toolchain(&config);
	let packages = if args.packages.is_empty() {
		config.dist.packages.clone()
	} else {
		args.packages.clone()
	};

	if variants.is_empty() {
		return Err("no variants selected; check `variants` in dist.toml".into());
	}

	let plugins = plugins(&root, &packages)?;
	let mut built: BTreeMap<Variant, Vec<Built>> = BTreeMap::new();

	for &variant in &variants {
		let target = variant.rust_target(toolchain);
		println!("\n== {variant} ({target}) ==");

		let (supported, skipped): (Vec<&Plugin>, Vec<&Plugin>) =
			plugins.iter().partition(|p| p.supports(variant));
		for plugin in &skipped {
			println!(
				"   skipping `{}`: it has no `{}` feature",
				plugin.package,
				variant.feature()
			);
		}
		// One `cargo build` per package: built together, their features on
		// `nsis-plugin` unify, and one plug-in's `std` breaks another's
		// `no_std` panic handler.
		for plugin in &supported {
			build(&root, &plugin.package, variant, toolchain)?;
		}

		let dest = plugins_dir(&root, &config, variant);
		fs::create_dir_all(&dest)?;

		for plugin in supported {
			let dll = dll_path(&root, target, &plugin.lib);
			if !dll.exists() {
				return Err(format!(
					"expected {} after building `{}`",
					dll.display(),
					plugin.package
				)
				.into());
			}
			let file_name = dll.file_name().expect("dll has a file name");
			let target_path = dest.join(file_name);
			fs::copy(&dll, &target_path)?;
			let bytes = fs::metadata(&target_path)?.len();
			println!("   {} ({bytes} bytes)", target_path.display());
			built.entry(variant).or_default().push(Built {
				package: plugin.package.clone(),
				name: file_name.to_string_lossy().into_owned(),
				bytes,
			});
		}
	}

	println!("\n{}", summary(&built));

	if args.no_size_check {
		println!("size budget: skipped (--no-size-check)");
		return Ok(());
	}
	check_budget(&config, toolchain, &built)
}

fn build(root: &Path, package: &str, variant: Variant, toolchain: Toolchain) -> Res<()> {
	let mut cmd = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
	cmd.current_dir(root);

	// cargo-xwin is how the MSVC targets build away from Windows.
	if toolchain == Toolchain::Msvc && !cfg!(target_os = "windows") {
		cmd.arg("xwin");
	}

	cmd.arg("build").arg("--release");
	cmd.arg("--target").arg(variant.rust_target(toolchain));
	cmd.arg("--package").arg(package);
	// The two x86 variants differ only in character width.
	cmd.arg("--no-default-features")
		.arg("--features")
		.arg(variant.feature());

	let status = cmd.status().map_err(|e| {
		if toolchain == Toolchain::Msvc && !cfg!(target_os = "windows") {
			format!("{e}; MSVC targets need `cargo install cargo-xwin` away from Windows")
		} else {
			e.to_string()
		}
	})?;

	if status.success() {
		Ok(())
	} else {
		Err(format!("`cargo build` failed for `{package}` ({variant})").into())
	}
}

fn dll_path(root: &Path, target: &str, lib: &str) -> PathBuf {
	root.join("target")
		.join(target)
		.join("release")
		.join(format!("{lib}.dll"))
}

fn summary(built: &BTreeMap<Variant, Vec<Built>>) -> String {
	let mut out = String::from("Plugins/\n");
	for (variant, files) in built {
		let _ = writeln!(out, "  {variant}/");
		for Built { name, bytes, .. } in files {
			let _ = writeln!(out, "    {name}  {bytes} bytes");
		}
	}
	out
}

fn check_budget(
	config: &Config,
	toolchain: Toolchain,
	built: &BTreeMap<Variant, Vec<Built>>,
) -> Res<()> {
	let mut over = Vec::new();
	for (variant, files) in built {
		for Built {
			package,
			name,
			bytes,
		} in files
		{
			let Some(budget) = config.budget(package, toolchain, *variant) else {
				continue;
			};
			if *bytes > budget {
				over.push(format!(
					"{variant}/{name} is {bytes} bytes, over its {budget}-byte budget by {}",
					bytes - budget
				));
			}
		}
	}
	if over.is_empty() {
		println!("size budget: ok");
		Ok(())
	} else {
		Err(format!(
			"size budget exceeded:\n  {}\n\nRaise the budget in dist.toml only deliberately: \
			 this DLL is embedded in every installer built with it.",
			over.join("\n  ")
		)
		.into())
	}
}

fn size(args: Args) -> Res<()> {
	let (root, config) = load()?;
	let toolchain = args.toolchain(&config);

	let plugins = plugins(&root, &config.dist.packages)?;

	for variant in args.resolve_variants(&config)? {
		for plugin in plugins.iter().filter(|p| p.supports(variant)) {
			let package = &plugin.package;
			let dll = dll_path(&root, variant.rust_target(toolchain), &plugin.lib);
			let budget = config.budget(package, toolchain, variant);
			match fs::metadata(&dll) {
				Ok(meta) => {
					let bytes = meta.len();
					let note = match budget {
						Some(b) if bytes > b => format!("  OVER BUDGET ({b})"),
						Some(b) => format!("  ({b} budget)"),
						None => String::new(),
					};
					println!("{variant:<16} {package:<12} {bytes:>8} bytes{note}");
				}
				Err(_) => println!("{variant:<16} {package:<12}    not built"),
			}
		}
	}
	Ok(())
}

fn targets() -> Res<()> {
	println!(
		"{:<16} {:<28} {:<28} features",
		"variant", "gnu target", "msvc target"
	);
	for variant in Variant::ALL {
		println!(
			"{:<16} {:<28} {:<28} {}",
			variant.to_string(),
			variant.rust_target(Toolchain::Gnu),
			variant.rust_target(Toolchain::Msvc),
			variant.feature(),
		);
	}
	Ok(())
}

fn smoke(args: Args) -> Res<()> {
	let (root, config) = load()?;
	let variants = args.resolve_variants(&config)?;
	let variant = *variants.first().ok_or("no variant selected")?;

	let plugin_dir = plugins_dir(&root, &config, variant);
	if !plugin_dir.exists() {
		return Err(format!(
			"{} does not exist; run `cargo xtask dist` first",
			plugin_dir.display()
		)
		.into());
	}

	let script = root.join(
		args.script
			.as_deref()
			.unwrap_or("examples/hello/test/smoke.nsi"),
	);
	let out_dir = root.join(&config.dist.out_dir).join("smoke");
	fs::create_dir_all(&out_dir)?;
	let installer = out_dir.join(format!("smoke-{variant}.exe"));

	// $MAKENSIS points the smoke test at a different NSIS build — in
	// particular one built with /DNSIS_MAX_STRLEN=8192.
	let override_makensis = std::env::var("MAKENSIS").ok();
	let mut cmd = Command::new(override_makensis.as_deref().unwrap_or("makensis"));
	// makensis takes its stubs from $NSISDIR when set, and mise.toml sets it to
	// the stock install. A custom makensis would then silently pair itself with
	// stock 1024-character exeheads, so let it find its own.
	if override_makensis.is_some() {
		cmd.env_remove("NSISDIR");
	}
	let status = cmd
		.arg(format!("-DPLUGINDIR={}", plugin_dir.display()))
		.arg(format!("-DOUTFILE={}", installer.display()))
		.arg(format!("-XTarget {}", variant.nsis_target()))
		.arg(&script)
		.status()?;
	if !status.success() {
		return Err("makensis failed".into());
	}
	println!("built {}", installer.display());

	if !args.wine {
		println!("run it with `--wine` to execute the installer");
		return Ok(());
	}

	let log = out_dir.join("smoke.log");
	let _ = fs::remove_file(&log);
	let status = Command::new("wine")
		.arg(&installer)
		.arg("/S")
		.arg(format!("/LOG={}", log.display()))
		.status()?;
	if !status.success() {
		return Err("installer exited non-zero".into());
	}

	let report = fs::read_to_string(&log)?;
	print!("{report}");
	if report.contains("FAIL") {
		return Err("smoke test reported failures".into());
	}
	println!("smoke test passed");
	Ok(())
}
