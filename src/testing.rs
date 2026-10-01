//! A fake installer, so plug-in logic can be tested on macOS and Linux.
//!
//! [`TestInstaller`] builds a real `GlobalAlloc`-shaped stack, a real
//! `variables` array and a real [`ExecFlags`] block, then hands out [`Nsis`]
//! values pointing at them. The code under test is the same code that ships;
//! only the allocator underneath is emulated. This is the layer C plug-ins have
//! no equivalent for.
//!
//! ```
//! use nsis_plugin::testing::TestInstaller;
//!
//! let mut inst = TestInstaller::new(1024);
//! inst.push("world");
//!
//! let mut nsis = inst.nsis();
//! let who = nsis.stack.pop().unwrap();
//! nsis.stack.push(&format!("hello {who}")).unwrap();
//!
//! assert_eq!(inst.pop().as_deref(), Some("hello world"));
//! ```
//!
//! Enable with the `testing` feature; it is on automatically under `cfg(test)`.

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::ffi::c_int;

use crate::raw::{ExecFlags, ExtraParameters, StackNode};
use crate::tchar::{self, Tchar};
use crate::{Nsis, VAR_COUNT, Var};

/// A stand-in for the calling installer.
pub struct TestInstaller {
	head: Box<*mut StackNode>,
	vars: Box<[Tchar]>,
	flags: Box<ExecFlags>,
	extra: Box<ExtraParameters>,
	string_size: usize,
}

impl TestInstaller {
	/// Builds an installer with the given `NSIS_MAX_STRLEN`.
	///
	/// Stock NSIS is 1024; a long-string build is 8192. Test against both.
	#[must_use]
	pub fn new(string_size: usize) -> Self {
		let mut flags = Box::new(ExecFlags {
			plugin_api_version: crate::NSISPIAPIVER_CURR,
			..ExecFlags::default()
		});
		let extra = Box::new(ExtraParameters {
			exec_flags: &raw mut *flags,
			execute_code_segment: None,
			validate_filename: None,
			register_plugin_callback: None,
		});
		Self {
			head: Box::new(core::ptr::null_mut()),
			vars: vec![0 as Tchar; VAR_COUNT * string_size].into_boxed_slice(),
			flags,
			extra,
			string_size,
		}
	}

	/// A stock 1024-character installer.
	#[must_use]
	pub fn stock() -> Self {
		Self::new(1024)
	}

	/// A `/DNSIS_MAX_STRLEN=8192` long-string installer.
	#[must_use]
	pub fn long_string() -> Self {
		Self::new(8192)
	}

	/// The installer's `NSIS_MAX_STRLEN`.
	#[must_use]
	pub fn string_size(&self) -> usize {
		self.string_size
	}

	/// Hands out an [`Nsis`] pointing at this installer's state.
	///
	/// The result borrows `self` for as long as it is alive; the exclusive
	/// borrow is what keeps that honest.
	#[must_use]
	pub fn nsis(&mut self) -> Nsis {
		// SAFETY: every pointer below is into a `Box` owned by `self`, which
		// `&mut self` keeps alive and exclusive for the returned value's use.
		unsafe {
			Nsis::from_raw(
				core::ptr::null_mut(),
				self.string_size as c_int,
				self.vars.as_mut_ptr(),
				&raw mut *self.head,
				&raw mut *self.extra,
			)
		}
	}

	/// Calls a plug-in export exactly as the installer would, through the raw
	/// five-pointer boundary.
	///
	/// Testing through this rather than by calling the body directly is the
	/// point: it exercises the generated export, the `Nsis` construction and
	/// the `Result`-to-error-flag mapping.
	pub fn call(&mut self, export: crate::raw::Export) {
		// SAFETY: every argument is this installer's own state, shaped exactly
		// as the exehead shapes it.
		unsafe {
			export(
				core::ptr::null_mut(),
				self.string_size as c_int,
				self.vars.as_mut_ptr(),
				&raw mut *self.head,
				&raw mut *self.extra,
			);
		}
	}

	/// Pushes a string, as a script's `Push` would.
	pub fn push(&mut self, value: &str) {
		let _ = self.nsis().stack.push(value);
	}

	/// Pops a string, as a script's `Pop` would. `None` on an empty stack.
	pub fn pop(&mut self) -> Option<String> {
		self.nsis().stack.pop().ok()
	}

	/// The whole stack, top first, without consuming it.
	#[must_use]
	pub fn stack(&self) -> Vec<String> {
		let mut out = Vec::new();
		let mut node = *self.head;
		while !node.is_null() {
			// SAFETY: nodes are ours, allocated with room for `string_size`.
			let text = unsafe { (&raw const (*node).text).cast::<Tchar>() };
			// SAFETY: as above.
			out.push(unsafe { tchar::read_bounded(text, self.string_size) });
			// SAFETY: as above.
			node = unsafe { (*node).next };
		}
		out
	}

	/// Whether the stack is empty.
	#[must_use]
	pub fn is_empty(&self) -> bool {
		(*self.head).is_null()
	}

	/// Reads a user variable.
	#[must_use]
	pub fn var(&self, var: Var) -> String {
		let offset = var.index() * self.string_size;
		// SAFETY: `vars` is `VAR_COUNT * string_size` elements long.
		unsafe { tchar::read_bounded(self.vars.as_ptr().add(offset), self.string_size) }
	}

	/// Writes a user variable, as a script's `StrCpy` would.
	pub fn set_var(&mut self, var: Var, value: &str) {
		let _ = self.nsis().vars.set(var, value);
	}

	/// The installer's exec flags.
	#[must_use]
	pub fn flags(&self) -> &ExecFlags {
		&self.flags
	}

	/// The installer's exec flags, mutably.
	pub fn flags_mut(&mut self) -> &mut ExecFlags {
		&mut self.flags
	}

	/// Whether the error flag is set — what `IfErrors` would see.
	#[must_use]
	pub fn error(&self) -> bool {
		self.flags.exec_error != 0
	}

	/// Clears the error flag, as `ClearErrors` would.
	pub fn clear_error(&mut self) {
		self.flags.exec_error = 0;
	}

	/// The installer's `extra_parameters`, mutably, so a test can install fake
	/// callbacks such as `validate_filename`.
	pub fn extra_mut(&mut self) -> &mut ExtraParameters {
		&mut self.extra
	}
}

impl Drop for TestInstaller {
	fn drop(&mut self) {
		let mut node = *self.head;
		while !node.is_null() {
			// SAFETY: nodes are ours and were allocated by `sys::alloc_zeroed`.
			let next = unsafe { (*node).next };
			// SAFETY: as above; the node is not used again.
			unsafe { crate::sys::free(node.cast()) };
			node = next;
		}
	}
}
