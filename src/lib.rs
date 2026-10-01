//! Write NSIS plug-ins in Rust, with all four target variants building
//! correctly by default.
//!
//! ```ignore
//! #![cfg_attr(target_os = "windows", no_std)]
//! extern crate alloc;
//!
//! use nsis_plugin::{Nsis, Result, nsis_fn, nsis_plugin};
//!
//! nsis_plugin!();
//!
//! nsis_fn! {
//!     fn Add(nsis: &mut Nsis) -> Result<()> {
//!         let b = nsis.stack.pop_int()?;
//!         let a = nsis.stack.pop_int()?;
//!         nsis.stack.push_int(a + b)?;
//!         Ok(())
//!     }
//! }
//! ```
//!
//! ```nsis
//! Push 2
//! Push 40
//! example::Add
//! Pop $0   ; 42
//! ```
//!
//! # What the wrapper is for
//!
//! A plug-in export is a single C function with five arguments and a trailing
//! ellipsis. The two facts that make it easy to get wrong:
//!
//! - **`string_size` is a runtime parameter**, the calling installer's
//!   `NSIS_MAX_STRLEN`. Every buffer holding installer strings is sized from
//!   it, so long-string builds (`/DNSIS_MAX_STRLEN=8192`) work structurally
//!   rather than as a feature.
//! - **Stack nodes are `GlobalAlloc`'d and the caller frees on pop.** Pushing a
//!   longer string than `string_size` is a heap overflow. [`Stack::push`] is
//!   bounded and reports [`Error::Truncated`].
//!
//! # The error flag
//!
//! Returning `Err` from a [`nsis_fn!`] body sets `exec_flags->exec_error`,
//! which is what `IfErrors` reads. `?` on an empty stack therefore does the
//! NSIS-native thing with no ceremony.
//!
//! # Character width
//!
//! [`Tchar`] is `u16` under the default `unicode` feature and `u8` under
//! `ansi`; the two are mutually exclusive. The public API traffics in
//! `String`/`&str` and converts at the boundary. 64-bit NSIS targets are always
//! Unicode, so the build matrix is four combinations, not eight.

#![cfg_attr(not(any(test, feature = "std")), no_std)]
#![warn(missing_docs)]
#![warn(clippy::undocumented_unsafe_blocks)]

extern crate alloc;

#[cfg(all(feature = "std", not(test)))]
extern crate std;

use alloc::string::String;
use core::ffi::c_int;

pub mod error;
pub mod int;
mod macros;
pub mod raw;
pub mod rt;
mod stack;
mod sys;
mod tchar;
mod vars;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use crate::error::{Error, Result};
pub use crate::raw::{
	ExecFlags, ExtraParameters, Hmodule, Hwnd, NSISPIAPIVER_1_0, NSISPIAPIVER_CURR,
	NSPIM_GUIUNLOAD, NSPIM_UNLOAD, NsisPluginCallback, StackNode,
};
pub use crate::stack::Stack;
pub use crate::tchar::Tchar;
pub use crate::vars::{VAR_COUNT, Var, Variables};

/// Everything a plug-in export receives, in one place.
///
/// Constructed for you by [`nsis_fn!`]; the equivalent of `EXDLL_INIT()`.
pub struct Nsis {
	/// The installer's argument stack.
	pub stack: Stack,
	/// The installer's 25 user variables.
	pub vars: Variables,
	/// The installer's parent window.
	pub hwnd: Hwnd,
	extra: *mut ExtraParameters,
}

impl Nsis {
	/// Builds an `Nsis` from the five arguments of a plug-in export.
	///
	/// # Safety
	/// All five arguments must be exactly what the installer passed, and the
	/// result must not outlive the call.
	#[must_use]
	pub unsafe fn from_raw(
		hwnd: Hwnd,
		string_size: c_int,
		variables: *mut Tchar,
		stacktop: *mut *mut StackNode,
		extra: *mut ExtraParameters,
	) -> Self {
		let string_size = string_size.max(0) as usize;
		Self {
			// SAFETY: the caller guarantees these are the installer's own
			// pointers, valid for the duration of the call.
			stack: unsafe { Stack::from_raw(stacktop, string_size) },
			// SAFETY: as above.
			vars: unsafe { Variables::from_raw(variables, string_size) },
			hwnd,
			extra,
		}
	}

	/// The calling installer's `NSIS_MAX_STRLEN`, in characters.
	///
	/// This is a *runtime* value. Never assume 1024.
	#[must_use]
	pub fn string_size(&self) -> usize {
		self.stack.string_size()
	}

	/// The raw `extra_parameters` block, if the installer supplied one.
	#[must_use]
	pub fn extra(&self) -> Option<&ExtraParameters> {
		// SAFETY: `extra` is either null or the installer's own block, valid
		// for the duration of the call.
		unsafe { self.extra.as_ref() }
	}

	/// The installer's live [`ExecFlags`], if available.
	#[must_use]
	pub fn exec_flags(&self) -> Option<&ExecFlags> {
		// SAFETY: `exec_flags` points into the installer's own state.
		unsafe { self.extra()?.exec_flags.as_ref() }
	}

	/// The installer's live [`ExecFlags`], mutably.
	#[must_use]
	pub fn exec_flags_mut(&mut self) -> Option<&mut ExecFlags> {
		// SAFETY: as above; `&mut self` keeps this exclusive on our side, and
		// the installer is single-threaded while a plug-in call is running.
		unsafe { self.extra.as_ref()?.exec_flags.as_mut() }
	}

	/// `exec_flags->plugin_api_version`.
	///
	/// Only meaningful from NSIS 2.42 onward; older installers leave unrelated
	/// data in this field. Compare with `>=`, never `==`.
	#[must_use]
	pub fn plugin_api_version(&self) -> c_int {
		self.exec_flags().map_or(0, |f| f.plugin_api_version)
	}

	/// Fails unless the installer reports at least `required`.
	pub fn require_api_version(&self, required: c_int) -> Result<()> {
		let found = self.plugin_api_version();
		if found >= required {
			Ok(())
		} else {
			Err(Error::UnsupportedApiVersion { found, required })
		}
	}

	/// Runs a code segment in the installer.
	///
	/// An honest passthrough of `ExecuteCodeSegment`, with the same contract:
	/// `position` is zero-based, so a function address obtained from
	/// `GetFunctionAddress` needs `- 1`.
	///
	/// # Safety
	/// The installer will run arbitrary script code, which can re-enter this
	/// plug-in. Nothing here makes that safe.
	pub unsafe fn execute_code_segment(&mut self, position: c_int) -> Result<c_int> {
		let f = self
			.extra()
			.and_then(|e| e.execute_code_segment)
			.ok_or(Error::Unavailable("ExecuteCodeSegment"))?;
		// SAFETY: delegated to the caller, per this function's contract.
		Ok(unsafe { f(position, self.hwnd) })
	}

	/// Replaces characters that are invalid in a filename, via the installer's
	/// own `validate_filename`.
	pub fn validate_filename(&mut self, name: &str) -> Result<String> {
		let f = self
			.extra()
			.and_then(|e| e.validate_filename)
			.ok_or(Error::Unavailable("validate_filename"))?;

		// `validate_filename` (`Source/exehead/util.c`) takes no length and only
		// ever shortens the string, so a buffer the size of the input is always
		// enough. The buffer is ours, not the installer's, so `string_size` does
		// not apply; it bounds the result wherever the caller stores it.
		let mut buf = tchar::encode(name);
		buf.push(0);
		// SAFETY: `buf` is NUL-terminated and the installer never lengthens it.
		unsafe { f(buf.as_mut_ptr()) };
		// SAFETY: `buf` is still `buf.len()` elements long.
		Ok(unsafe { tchar::read_bounded(buf.as_ptr(), buf.len()) })
	}

	/// Registers an unload callback, as built by [`nsis_unload!`].
	///
	/// `/NOUNLOAD` and `SetPluginsUnload` were deprecated in NSIS 3 and
	/// plug-ins now stay loaded for the life of the installer, so this is where
	/// cleanup belongs. Registering the same callback twice is not an error.
	pub fn register_callback(&mut self, callback: NsisPluginCallback) -> Result<()> {
		self.require_api_version(NSISPIAPIVER_1_0)?;
		let f = self
			.extra()
			.and_then(|e| e.register_plugin_callback)
			.ok_or(Error::Unavailable("RegisterPluginCallback"))?;

		let module = rt::hinstance();
		if module.is_null() {
			return Err(Error::Unavailable("HINSTANCE"));
		}
		// SAFETY: `module` is this DLL's own handle and `callback` is a valid
		// `extern "C"` function pointer.
		match unsafe { f(module, callback) } {
			// 0 is success; 1 means it was already registered, which is fine.
			0 | 1 => Ok(()),
			_ => Err(Error::Failed),
		}
	}
}

/// Generates a paired getter and setter for an `exec_flags_t` field.
macro_rules! flag_accessors {
	($(
		$(#[$attr:meta])*
		$get:ident / $set:ident => $field:ident : bool
	),* $(,)?) => {
		impl Nsis {$(
			$(#[$attr])*
			#[must_use]
			pub fn $get(&self) -> bool {
				self.exec_flags().is_some_and(|f| f.$field != 0)
			}

			$(#[$attr])*
			pub fn $set(&mut self, value: bool) {
				if let Some(f) = self.exec_flags_mut() {
					f.$field = c_int::from(value);
				}
			}
		)*}
	};
	($(
		$(#[$attr:meta])*
		$get:ident / $set:ident => $field:ident : int
	),* $(,)?) => {
		impl Nsis {$(
			$(#[$attr])*
			#[must_use]
			pub fn $get(&self) -> c_int {
				self.exec_flags().map_or(0, |f| f.$field)
			}

			$(#[$attr])*
			pub fn $set(&mut self, value: c_int) {
				if let Some(f) = self.exec_flags_mut() {
					f.$field = value;
				}
			}
		)*}
	};
}

flag_accessors! {
	/// The error flag `IfErrors` reads. Set automatically when a
	/// [`nsis_fn!`] body returns `Err`.
	error / set_error => exec_error: bool,
	/// `IfSilent` / `SetSilent`.
	silent / set_silent => silent: bool,
	/// `IfAbort`.
	abort / set_abort => abort: bool,
	/// `IfRebootFlag` / `SetRebootFlag`.
	reboot_flag / set_reboot_flag => exec_reboot: bool,
	/// `SetAutoClose`.
	autoclose / set_autoclose => autoclose: bool,
	/// `IfRtlLanguage`: whether `$LANGUAGE` is right-to-left.
	rtl / set_rtl => rtl: bool,
	/// `SetShellVarContext`: false is user context, true is machine context.
	all_users / set_all_users => all_user_var: bool,
}

flag_accessors! {
	/// `SetErrorLevel`.
	errlvl / set_errlvl => errlvl: int,
	/// `SetRegView`: 0 is the default view.
	alter_reg_view / set_alter_reg_view => alter_reg_view: int,
	/// `SetDetailsPrint`.
	status_update / set_status_update => status_update: int,
	/// `GetInstDirError`.
	instdir_error / set_instdir_error => instdir_error: int,
}

#[cfg(test)]
mod tests {
	use alloc::string::String;

	use crate::tchar::Tchar;
	use crate::testing::TestInstaller;

	/// Stands in for the exehead's `validate_filename`: drops every `:` in
	/// place, shortening the string as the real one does.
	unsafe extern "system" fn strip_colons(s: *mut Tchar) {
		let (mut read, mut write) = (s, s);
		// SAFETY: `s` is NUL-terminated; `write` never passes `read`.
		unsafe {
			while *read != 0 {
				if *read != b':' as Tchar {
					*write = *read;
					write = write.add(1);
				}
				read = read.add(1);
			}
			*write = 0;
		}
	}

	/// The buffer is sized from the input, so a name longer than `string_size`
	/// is validated in full rather than rejected.
	#[test]
	fn validate_filename_is_not_bounded_by_string_size() {
		let mut inst = TestInstaller::stock();
		inst.extra_mut().validate_filename = Some(strip_colons);

		let name: String = "a:".repeat(2000);
		let clean = inst.nsis().validate_filename(&name).unwrap();
		assert_eq!(clean, "a".repeat(2000));
	}
}
