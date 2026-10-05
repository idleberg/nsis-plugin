//! {{plugin_description}}
//!
//! `no_std` only on Windows, so `cargo check`, `cargo clippy` and `cargo test`
//! all work on the host.

#![cfg_attr(target_os = "windows", no_std)]

extern crate alloc;

use alloc::format;

use nsis_plugin::{Nsis, Result, nsis_fn, nsis_plugin};

nsis_plugin!();

nsis_fn! {
	/// Pops a name and pushes a greeting.
	///
	/// ```nsis
	/// Push "world"
	/// {{crate_name}}::Hello
	/// Pop $0   ; Hello, world!
	/// ```
	fn Hello(nsis: &mut Nsis) -> Result<()> {
		let name = nsis.stack.pop()?;
		nsis.stack.push(&format!("Hello, {name}!"))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use nsis_plugin::testing::TestInstaller;

	/// `call` goes through the generated export and the raw five-pointer
	/// boundary, not straight into the body — that is the part worth testing.
	#[test]
	fn greets() {
		let mut inst = TestInstaller::stock();
		inst.push("world");
		inst.call(Hello);
		assert_eq!(inst.pop().as_deref(), Some("Hello, world!"));
		assert!(!inst.error(), "IfErrors should see nothing");
	}

	#[test]
	fn an_empty_stack_sets_the_error_flag() {
		let mut inst = TestInstaller::stock();
		inst.call(Hello);
		assert!(inst.error());
	}

	/// The same plug-in has to be correct against a `/DNSIS_MAX_STRLEN=8192`
	/// installer without being rebuilt.
	#[test]
	fn works_against_a_long_string_installer() {
		let mut inst = TestInstaller::long_string();
		inst.push("world");
		inst.call(Hello);
		assert_eq!(inst.pop().as_deref(), Some("Hello, world!"));
	}
}
