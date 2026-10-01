//! The installer's argument stack.
//!
//! Every allocation here is sized from the `string_size` the exehead handed us
//! at call time, never from a constant. That is what makes long-string builds
//! (`/DNSIS_MAX_STRLEN=8192`) work structurally rather than by luck.

#![allow(
	clippy::undocumented_unsafe_blocks,
	reason = "every pointer here is the installer's own, established once in \
	          `Stack::from_raw` and valid for the duration of a plug-in call; \
	          restating it on each dereference would bury the comments that \
	          carry real information"
)]

use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{Error, Result};
use crate::int;
use crate::raw::StackNode;
use crate::sys;
use crate::tchar::{self, Tchar};

/// Handle to the installer's `stack_t **`.
///
/// Obtained from [`Nsis::stack`](crate::Nsis::stack); not constructed directly
/// outside of tests.
pub struct Stack {
	top: *mut *mut StackNode,
	string_size: usize,
}

impl Stack {
	/// Wraps the `stacktop` and `string_size` arguments of a plug-in export.
	///
	/// # Safety
	/// `top` must be the `stacktop` pointer the installer passed, and
	/// `string_size` its `string_size` argument. The stack must outlive the
	/// returned value.
	#[must_use]
	pub unsafe fn from_raw(top: *mut *mut StackNode, string_size: usize) -> Self {
		Self { top, string_size }
	}

	/// The calling installer's `NSIS_MAX_STRLEN`, in characters.
	#[must_use]
	pub fn string_size(&self) -> usize {
		self.string_size
	}

	/// Longest string this installer can hold, excluding the NUL terminator.
	#[must_use]
	pub fn max_len(&self) -> usize {
		self.string_size.saturating_sub(1)
	}

	/// Whether the stack has no entries. Also true when there is no stack.
	#[must_use]
	pub fn is_empty(&self) -> bool {
		self.top.is_null() || unsafe { (*self.top).is_null() }
	}

	/// Number of entries currently on the stack.
	///
	/// Walks the whole list, so prefer [`is_empty`](Self::is_empty) in a loop.
	#[must_use]
	pub fn len(&self) -> usize {
		if self.top.is_null() {
			return 0;
		}
		let mut node = unsafe { *self.top };
		let mut n = 0;
		while !node.is_null() {
			n += 1;
			node = unsafe { (*node).next };
		}
		n
	}

	/// Pops the top entry and frees its node, as `popstring` does.
	pub fn pop(&mut self) -> Result<String> {
		self.pop_units().map(|units| tchar::decode(&units))
	}

	/// Reads the top entry without removing it.
	pub fn peek(&self) -> Result<String> {
		let node = self.head()?;
		Ok(unsafe { tchar::read_bounded(self.text_of(node), self.string_size) })
	}

	/// Pops and discards the top entry.
	pub fn discard(&mut self) -> Result<()> {
		self.pop_units().map(|_| ())
	}

	/// Pops an integer using NSIS's own conversion.
	///
	/// Unparseable input is `0`, not an error — this matches `popintptr`, and
	/// scripts rely on it. Only an *empty stack* is an error.
	pub fn pop_int(&mut self) -> Result<isize> {
		let units = self.pop_units()?;
		Ok(int::str_to_ptr(int::parse_window(&units)))
	}

	/// Pops an integer, additionally accepting `2|4|8` forms, as `popint_or`.
	pub fn pop_int_or(&mut self) -> Result<isize> {
		let units = self.pop_units()?;
		Ok(int::atoi_or(int::parse_window(&units)))
	}

	/// Pushes a string, bounded by the installer's `string_size`.
	///
	/// On [`Error::Truncated`] the clipped value has still been pushed, exactly
	/// as `lstrcpyn` would leave it. The error exists so the caller can decide
	/// whether the truncation matters.
	pub fn push(&mut self, s: &str) -> Result<()> {
		self.push_units(&tchar::encode(s))
	}

	/// Pushes an integer, formatted as `pushintptr` formats it.
	pub fn push_int(&mut self, value: isize) -> Result<()> {
		self.push_units(&int::format(value))
	}

	/// Pushes a boolean as the `0`/`1` NSIS scripts compare against.
	pub fn push_bool(&mut self, value: bool) -> Result<()> {
		self.push_int(isize::from(value))
	}

	// -- internals ----------------------------------------------------------

	fn head(&self) -> Result<*mut StackNode> {
		if self.top.is_null() {
			return Err(Error::NoStack);
		}
		let node = unsafe { *self.top };
		if node.is_null() {
			return Err(Error::EmptyStack);
		}
		Ok(node)
	}

	fn text_of(&self, node: *mut StackNode) -> *mut Tchar {
		unsafe { (&raw mut (*node).text).cast::<Tchar>() }
	}

	fn pop_units(&mut self) -> Result<Vec<Tchar>> {
		let node = self.head()?;
		let text = self.text_of(node);
		let len = unsafe { tchar::strlen_bounded(text, self.string_size) };
		let units = unsafe { core::slice::from_raw_parts(text, len) }.to_vec();
		unsafe {
			*self.top = (*node).next;
			sys::free(node.cast());
		}
		Ok(units)
	}

	fn push_units(&mut self, units: &[Tchar]) -> Result<()> {
		if self.top.is_null() {
			return Err(Error::NoStack);
		}
		// No early return for `string_size == 0`: `pushstring` still pushes a
		// node, empty because `lstrcpyn` with 0 writes nothing. Skipping the
		// push would leave the script popping someone else's value.
		let node = unsafe { sys::alloc_zeroed(StackNode::alloc_size(self.string_size)) }
			.cast::<StackNode>();
		if node.is_null() {
			return Err(Error::OutOfMemory);
		}

		let fit = unsafe { tchar::write_bounded(self.text_of(node), self.string_size, units) };
		unsafe {
			(*node).next = *self.top;
			*self.top = node;
		}

		if fit { Ok(()) } else { Err(Error::Truncated) }
	}
}

#[cfg(test)]
mod tests {
	use alloc::string::String;
	use alloc::vec::Vec;

	use super::*;
	use crate::testing::TestInstaller;

	#[test]
	fn round_trips_a_string() {
		let mut inst = TestInstaller::stock();
		inst.nsis().stack.push("hello").unwrap();
		assert_eq!(inst.nsis().stack.pop().unwrap(), "hello");
	}

	#[test]
	fn is_last_in_first_out() {
		let mut inst = TestInstaller::stock();
		let mut nsis = inst.nsis();
		for value in ["first", "second", "third"] {
			nsis.stack.push(value).unwrap();
		}
		assert_eq!(inst.stack(), ["third", "second", "first"]);
		assert_eq!(inst.pop().unwrap(), "third");
		assert_eq!(inst.pop().unwrap(), "second");
		assert_eq!(inst.pop().unwrap(), "first");
		assert!(inst.is_empty());
	}

	#[test]
	fn popping_an_empty_stack_is_an_error() {
		let mut inst = TestInstaller::stock();
		assert_eq!(inst.nsis().stack.pop().unwrap_err(), Error::EmptyStack);
	}

	#[test]
	fn a_null_stack_is_not_a_crash() {
		let mut stack = unsafe { Stack::from_raw(core::ptr::null_mut(), 1024) };
		assert!(stack.is_empty());
		assert_eq!(stack.len(), 0);
		assert_eq!(stack.pop().unwrap_err(), Error::NoStack);
		assert_eq!(stack.push("x").unwrap_err(), Error::NoStack);
	}

	/// `lstrcpyn(dst, src, string_size)` copies `string_size - 1` characters
	/// plus a NUL, so that is exactly what fits.
	#[test]
	fn the_boundary_is_string_size_minus_one() {
		for size in [64, 1024, 8192] {
			let mut inst = TestInstaller::new(size);

			let exact: String = core::iter::repeat_n('x', size - 1).collect();
			inst.nsis().stack.push(&exact).unwrap();
			assert_eq!(inst.pop().unwrap(), exact);

			let one_too_many: String = core::iter::repeat_n('x', size).collect();
			assert_eq!(
				inst.nsis().stack.push(&one_too_many).unwrap_err(),
				Error::Truncated
			);
			// The clipped value is still pushed, exactly as `lstrcpyn` leaves it.
			assert_eq!(inst.pop().unwrap().len(), size - 1);
		}
	}

	/// The whole point of the crate: buffers come from the installer's runtime
	/// `string_size`, so the same code is correct against a long-string build.
	#[test]
	fn a_long_string_build_holds_long_strings() {
		let mut inst = TestInstaller::long_string();
		let long: String = core::iter::repeat_n('z', 8191).collect();
		inst.nsis().stack.push(&long).unwrap();
		assert_eq!(inst.pop().unwrap(), long);

		// The same value against a stock installer must be reported as
		// truncated rather than written past the node.
		let mut stock = TestInstaller::stock();
		assert_eq!(
			stock.nsis().stack.push(&long).unwrap_err(),
			Error::Truncated
		);
		assert_eq!(stock.pop().unwrap().len(), 1023);
	}

	/// `pushstring` with `g_stringsize == 0` still pushes an (empty) node.
	#[test]
	fn a_zero_string_size_still_pushes_an_empty_entry() {
		let mut inst = TestInstaller::new(0);
		assert_eq!(inst.nsis().stack.push("x").unwrap_err(), Error::Truncated);
		assert_eq!(inst.stack(), [""]);
	}

	#[test]
	fn pops_integers_with_nsis_semantics() {
		let mut inst = TestInstaller::stock();
		inst.push("0x10");
		assert_eq!(inst.nsis().stack.pop_int().unwrap(), 16);

		inst.push("2|4|8");
		assert_eq!(inst.nsis().stack.pop_int_or().unwrap(), 14);

		// Unparseable is zero, not an error — only an empty stack is an error.
		inst.push("not a number");
		assert_eq!(inst.nsis().stack.pop_int().unwrap(), 0);
	}

	#[test]
	fn pushes_integers_as_scripts_expect() {
		let mut inst = TestInstaller::stock();
		inst.nsis().stack.push_int(-42).unwrap();
		assert_eq!(inst.pop().unwrap(), "-42");
		inst.nsis().stack.push_bool(true).unwrap();
		assert_eq!(inst.pop().unwrap(), "1");
	}

	#[test]
	fn peek_does_not_consume() {
		let mut inst = TestInstaller::stock();
		inst.push("kept");
		assert_eq!(inst.nsis().stack.peek().unwrap(), "kept");
		assert_eq!(inst.nsis().stack.peek().unwrap(), "kept");
		assert_eq!(inst.nsis().stack.len(), 1);
	}

	/// Unicode builds carry anything; ANSI builds are limited to whatever the
	/// active code page can represent, so the two cases differ.
	#[test]
	fn survives_non_ascii() {
		let mut inst = TestInstaller::stock();

		#[cfg(feature = "unicode")]
		let values = ["grüße", "日本語", "🦀"];
		#[cfg(all(feature = "ansi", not(feature = "unicode")))]
		let values = ["grüße", "façade"];

		for value in values {
			inst.nsis().stack.push(value).unwrap();
			assert_eq!(inst.pop().unwrap(), value);
		}
	}

	#[test]
	fn many_entries_do_not_leak_into_each_other() {
		let mut inst = TestInstaller::new(32);
		let values: Vec<String> = (0..50).map(|i| alloc::format!("value-{i}")).collect();
		{
			let mut nsis = inst.nsis();
			for value in &values {
				nsis.stack.push(value).unwrap();
			}
		}
		for value in values.iter().rev() {
			assert_eq!(&inst.pop().unwrap(), value);
		}
		assert!(inst.is_empty());
	}
}
