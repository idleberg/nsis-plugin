//! The failure channel.
//!
//! An `Err` returned from a [`nsis_fn!`](crate::nsis_fn) body sets
//! `exec_flags->exec_error`, which is exactly what `IfErrors` reads in the
//! calling script. `?` on an empty stack therefore does the NSIS-native thing
//! with no ceremony.

use core::fmt;

/// Result of a plug-in operation.
pub type Result<T> = core::result::Result<T, Error>;

/// What can go wrong at the plug-in boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
	/// Popped from an empty stack. Mirrors `popstring` returning 1.
	EmptyStack,
	/// The installer passed a null `stacktop`, so there is no stack at all.
	NoStack,
	/// The installer passed a null `variables` array.
	NoVariables,
	/// The installer passed a null `extra_parameters`, or a null member of it.
	///
	/// The name is the C field that was missing.
	Unavailable(&'static str),
	/// The value was written, but did not fit in `string_size` characters.
	///
	/// The truncated value *is* stored — this mirrors `lstrcpyn` — and the
	/// error exists so the caller can decide whether that matters.
	Truncated,
	/// `GlobalAlloc` returned null.
	OutOfMemory,
	/// The installer is older than the plug-in ABI version required.
	UnsupportedApiVersion {
		/// What `exec_flags->plugin_api_version` reported.
		found: i32,
		/// What was required.
		required: i32,
	},
	/// The plug-in's own error, for `?` in user code that has no better fit.
	Failed,
}

impl fmt::Display for Error {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::EmptyStack => f.write_str("stack is empty"),
			Self::NoStack => f.write_str("installer passed a null stack pointer"),
			Self::NoVariables => f.write_str("installer passed a null variables array"),
			Self::Unavailable(what) => write!(f, "installer did not provide `{what}`"),
			Self::Truncated => f.write_str("value did not fit in the installer's string_size"),
			Self::OutOfMemory => f.write_str("GlobalAlloc failed"),
			Self::UnsupportedApiVersion { found, required } => write!(
				f,
				"installer plug-in API version {found:#010x} is older than the required {required:#010x}"
			),
			Self::Failed => f.write_str("plug-in reported failure"),
		}
	}
}

impl core::error::Error for Error {}
