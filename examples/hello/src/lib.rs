//! A minimal NSIS plug-in, and the smoke test for the whole build matrix.
//!
//! Compiled four ways — `x86-ansi`, `x86-unicode`, `amd64-unicode` and
//! `arm64-unicode` — and exercised through `makensis` by `test/smoke.nsi`.
//!
//! `no_std` only on Windows, so `cargo check`, `cargo clippy` and `cargo test`
//! all work on the host.

#![cfg_attr(target_os = "windows", no_std)]

extern crate alloc;

use alloc::format;
use alloc::string::String;

use nsis_plugin::{Nsis, Result, Var, nsis_fn, nsis_plugin};

nsis_plugin!();

nsis_fn! {
	/// `Push "world"` → `hello::Hello` → `Pop $0` → `Hello, world!`
	fn Hello(nsis: &mut Nsis) -> Result<()> {
		let name = nsis.stack.pop()?;
		nsis.stack.push(&format!("Hello, {name}!"))
	}

	/// Pops two integers and pushes their sum, using NSIS integer semantics —
	/// `0x2a`, `052` and `42` all mean the same thing.
	fn Add(nsis: &mut Nsis) -> Result<()> {
		let b = nsis.stack.pop_int()?;
		let a = nsis.stack.pop_int()?;
		nsis.stack.push_int(a.wrapping_add(b))
	}

	/// Reverses a string. Handy for proving long-string builds work: push
	/// something longer than 1024 characters and it comes back intact.
	fn Reverse(nsis: &mut Nsis) -> Result<()> {
		let input = nsis.stack.pop()?;
		let reversed: String = input.chars().rev().collect();
		nsis.stack.push(&reversed)
	}

	/// Pushes the calling installer's `NSIS_MAX_STRLEN`.
	///
	/// A stock `makensis` reports 1024; one built with
	/// `/DNSIS_MAX_STRLEN=8192` reports 8192. A plug-in with a hard-coded
	/// buffer could not tell you this.
	fn StringSize(nsis: &mut Nsis) -> Result<()> {
		let size = nsis.string_size();
		nsis.stack.push_int(size as isize)
	}

	/// Copies `$INSTDIR` onto the stack, showing variable access.
	fn InstDir(nsis: &mut Nsis) -> Result<()> {
		let dir = nsis.vars.get(Var::InstDir);
		nsis.stack.push(&dir)
	}

	/// Always fails, so `IfErrors` has something to catch.
	fn Fail(nsis: &mut Nsis) -> Result<()> {
		let _ = nsis;
		Err(nsis_plugin::Error::Failed)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use nsis_plugin::testing::TestInstaller;

	#[test]
	fn greets() {
		let mut inst = TestInstaller::stock();
		inst.push("world");
		inst.call(Hello);
		assert_eq!(inst.pop().as_deref(), Some("Hello, world!"));
		assert!(!inst.error());
	}

	#[test]
	fn adds_with_nsis_integer_semantics() {
		let mut inst = TestInstaller::stock();
		inst.push("052"); // octal 42
		inst.push("0x2a"); // hex 42
		inst.call(Add);
		assert_eq!(inst.pop().as_deref(), Some("84"));
	}

	#[test]
	fn sets_the_error_flag_on_an_empty_stack() {
		let mut inst = TestInstaller::stock();
		inst.call(Add);
		assert!(inst.error(), "IfErrors should see the failure");
	}

	#[test]
	fn fail_sets_the_error_flag() {
		let mut inst = TestInstaller::stock();
		inst.call(Fail);
		assert!(inst.error());
	}

	#[test]
	fn reports_the_installers_string_size() {
		for size in [1024usize, 8192] {
			let mut inst = TestInstaller::new(size);
			inst.call(StringSize);
			assert_eq!(inst.pop().unwrap().parse::<usize>().unwrap(), size);
		}
	}

	#[test]
	fn round_trips_a_long_string() {
		let mut inst = TestInstaller::long_string();
		let long: String = core::iter::repeat_n('a', 4000)
			.chain(core::iter::repeat_n('b', 4000))
			.collect();
		inst.push(&long);
		inst.call(Reverse);
		let out = inst.pop().unwrap();
		assert_eq!(out.len(), 8000);
		assert!(out.starts_with("bbbb") && out.ends_with("aaaa"));
	}

	#[test]
	fn reads_instdir() {
		let mut inst = TestInstaller::stock();
		inst.set_var(Var::InstDir, r"C:\Program Files\Thing");
		inst.call(InstDir);
		assert_eq!(inst.pop().as_deref(), Some(r"C:\Program Files\Thing"));
	}
}
