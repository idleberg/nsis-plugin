//! The installer's 25 user variables.
//!
//! `variables` is a flat array of `__INST_LAST` slots, each `string_size`
//! characters wide. The enum below mirrors the `INST_*` enum in
//! `Contrib/ExDLL/pluginapi.h` — including its order, which puts `$0`–`$9`
//! before `$R0`–`$R9`.

#![allow(
	clippy::undocumented_unsafe_blocks,
	reason = "every pointer here is the installer's own, established once in \
	          `Variables::from_raw`; `slot` is the single place bounds are \
	          checked, and it returns `None` rather than an unchecked pointer"
)]

use alloc::string::String;

use crate::error::{Error, Result};
use crate::int;
use crate::tchar::{self, Tchar};

/// Number of user variables, i.e. `__INST_LAST`.
pub const VAR_COUNT: usize = 25;

/// A user variable, indexed exactly as the `INST_*` enum indexes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u32)]
#[allow(
	missing_docs,
	reason = "each variant is its NSIS variable, e.g. `N0` is `$0`"
)]
pub enum Var {
	N0 = 0,
	N1,
	N2,
	N3,
	N4,
	N5,
	N6,
	N7,
	N8,
	N9,
	R0,
	R1,
	R2,
	R3,
	R4,
	R5,
	R6,
	R7,
	R8,
	R9,
	/// `$CMDLINE`
	CmdLine,
	/// `$INSTDIR`
	InstDir,
	/// `$OUTDIR`
	OutDir,
	/// `$EXEDIR`
	ExeDir,
	/// `$LANGUAGE`
	Language,
}

impl Var {
	/// Slot index, always less than [`VAR_COUNT`] — this is the type-level
	/// equivalent of `isvalidnsisvarindex`.
	#[must_use]
	pub const fn index(self) -> usize {
		self as usize
	}

	/// All variables, in `INST_*` order.
	pub const ALL: [Var; VAR_COUNT] = [
		Var::N0,
		Var::N1,
		Var::N2,
		Var::N3,
		Var::N4,
		Var::N5,
		Var::N6,
		Var::N7,
		Var::N8,
		Var::N9,
		Var::R0,
		Var::R1,
		Var::R2,
		Var::R3,
		Var::R4,
		Var::R5,
		Var::R6,
		Var::R7,
		Var::R8,
		Var::R9,
		Var::CmdLine,
		Var::InstDir,
		Var::OutDir,
		Var::ExeDir,
		Var::Language,
	];
}

/// Handle to the installer's `variables` array.
pub struct Variables {
	base: *mut Tchar,
	string_size: usize,
}

impl Variables {
	/// Wraps the `variables` and `string_size` arguments of a plug-in export.
	///
	/// # Safety
	/// `base` must be the `variables` pointer the installer passed, pointing at
	/// [`VAR_COUNT`] × `string_size` characters, and must outlive the result.
	#[must_use]
	pub unsafe fn from_raw(base: *mut Tchar, string_size: usize) -> Self {
		Self { base, string_size }
	}

	/// The calling installer's `NSIS_MAX_STRLEN`, in characters.
	#[must_use]
	pub fn string_size(&self) -> usize {
		self.string_size
	}

	/// Reads a variable.
	///
	/// Returns an empty string when the installer passed no variables array,
	/// because there is nothing sensible to read and `$0` is empty by default.
	#[must_use]
	pub fn get(&self, var: Var) -> String {
		match self.slot(var) {
			Some(slot) => unsafe { tchar::read_bounded(slot, self.string_size) },
			None => String::new(),
		}
	}

	/// Writes a variable, bounded by the installer's `string_size`.
	///
	/// `setuservariable` uses an unbounded `lstrcpy` here, which overflows the
	/// slot for a long enough value; this bounds the copy and reports
	/// [`Error::Truncated`] instead.
	pub fn set(&mut self, var: Var, value: &str) -> Result<()> {
		self.set_units(var, &tchar::encode(value))
	}

	/// Reads a variable using NSIS's own integer conversion.
	#[must_use]
	pub fn get_int(&self, var: Var) -> isize {
		match self.slot(var) {
			Some(slot) => {
				let len = unsafe { tchar::strlen_bounded(slot, self.string_size) };
				let units = unsafe { core::slice::from_raw_parts(slot, len) };
				int::str_to_ptr(int::parse_window(units))
			}
			None => 0,
		}
	}

	/// Writes an integer to a variable, formatted as `pushintptr` formats it.
	pub fn set_int(&mut self, var: Var, value: isize) -> Result<()> {
		self.set_units(var, &int::format(value))
	}

	fn set_units(&mut self, var: Var, units: &[Tchar]) -> Result<()> {
		let slot = self.slot(var).ok_or(Error::NoVariables)?;
		if unsafe { tchar::write_bounded(slot, self.string_size, units) } {
			Ok(())
		} else {
			Err(Error::Truncated)
		}
	}

	fn slot(&self, var: Var) -> Option<*mut Tchar> {
		if self.base.is_null() || self.string_size == 0 {
			return None;
		}
		Some(unsafe { self.base.add(var.index() * self.string_size) })
	}
}

#[cfg(test)]
mod tests {
	use alloc::string::String;

	use super::*;
	use crate::testing::TestInstaller;

	#[test]
	fn indices_match_the_inst_enum() {
		// `$0`–`$9` come first, then `$R0`–`$R9`, then the named ones.
		assert_eq!(Var::N0.index(), 0);
		assert_eq!(Var::N9.index(), 9);
		assert_eq!(Var::R0.index(), 10);
		assert_eq!(Var::R9.index(), 19);
		assert_eq!(Var::CmdLine.index(), 20);
		assert_eq!(Var::Language.index(), 24);
		assert_eq!(Var::ALL.len(), VAR_COUNT);
	}

	#[test]
	fn round_trips_a_value() {
		let mut inst = TestInstaller::stock();
		inst.nsis().vars.set(Var::R0, "value").unwrap();
		assert_eq!(inst.nsis().vars.get(Var::R0), "value");
		assert_eq!(inst.var(Var::R0), "value");
	}

	#[test]
	fn every_slot_is_independent() {
		let mut inst = TestInstaller::new(16);
		{
			let mut nsis = inst.nsis();
			for (i, &var) in Var::ALL.iter().enumerate() {
				nsis.vars.set(var, &alloc::format!("v{i}")).unwrap();
			}
		}
		let nsis = inst.nsis();
		for (i, &var) in Var::ALL.iter().enumerate() {
			assert_eq!(nsis.vars.get(var), alloc::format!("v{i}"));
		}
	}

	/// `setuservariable` writes with an unbounded `lstrcpy`, so an over-long
	/// value runs into the next variable's slot. This is the bug the crate
	/// exists to make unrepresentable.
	#[test]
	fn an_over_long_value_does_not_reach_the_next_slot() {
		const SIZE: usize = 16;
		let mut inst = TestInstaller::new(SIZE);
		inst.nsis().vars.set(Var::N1, "neighbour").unwrap();

		let too_long: String = core::iter::repeat_n('x', SIZE * 4).collect();
		assert_eq!(
			inst.nsis().vars.set(Var::N0, &too_long).unwrap_err(),
			Error::Truncated
		);

		assert_eq!(inst.var(Var::N0).len(), SIZE - 1);
		assert_eq!(inst.var(Var::N1), "neighbour", "$1 was overwritten");
	}

	#[test]
	fn the_boundary_is_string_size_minus_one() {
		for size in [32, 1024, 8192] {
			let mut inst = TestInstaller::new(size);

			let exact: String = core::iter::repeat_n('a', size - 1).collect();
			inst.nsis().vars.set(Var::InstDir, &exact).unwrap();
			assert_eq!(inst.var(Var::InstDir), exact);

			let one_too_many: String = core::iter::repeat_n('a', size).collect();
			assert_eq!(
				inst.nsis()
					.vars
					.set(Var::InstDir, &one_too_many)
					.unwrap_err(),
				Error::Truncated
			);
			assert_eq!(inst.var(Var::InstDir).len(), size - 1);
		}
	}

	#[test]
	fn integers_use_nsis_semantics() {
		let mut inst = TestInstaller::stock();
		inst.set_var(Var::R5, "0x20");
		assert_eq!(inst.nsis().vars.get_int(Var::R5), 32);
		inst.nsis().vars.set_int(Var::R6, -7).unwrap();
		assert_eq!(inst.var(Var::R6), "-7");
	}

	#[test]
	fn a_null_array_is_not_a_crash() {
		let mut vars = unsafe { Variables::from_raw(core::ptr::null_mut(), 1024) };
		assert_eq!(vars.get(Var::N0), "");
		assert_eq!(vars.get_int(Var::N0), 0);
		assert_eq!(vars.set(Var::N0, "x").unwrap_err(), Error::NoVariables);
	}
}
