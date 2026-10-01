//! The character-width abstraction.
//!
//! C solves this with `TCHAR` and two compilations. We do the same, but behind
//! a type: `Tchar` is `u16` in a Unicode build and `u8` in an ANSI build, and
//! the public API traffics in `String`/`&str`, converting at the boundary.
//!
//! The conversion mirrors `Contrib/ExDLL/pluginapi.c`: UTF-16 in the Unicode
//! build, `CP_ACP` via `MultiByteToWideChar`/`WideCharToMultiByte` in the ANSI
//! build.

#![allow(
	clippy::undocumented_unsafe_blocks,
	reason = "each function documents the buffer it requires under `# Safety`; \
	          the Win32 conversion calls are bounded by lengths computed by \
	          the preceding sizing call"
)]

use alloc::string::String;
use alloc::vec::Vec;

#[cfg(all(feature = "ansi", feature = "unicode"))]
compile_error!(
	"the `ansi` and `unicode` features of nsis-plugin are mutually exclusive; \
	 build an ANSI plug-in with `default-features = false, features = [\"ansi\"]`"
);

#[cfg(not(any(feature = "ansi", feature = "unicode")))]
compile_error!("nsis-plugin needs exactly one of the `ansi` or `unicode` features");

/// The installer's character type: `u16` under `unicode`, `u8` under `ansi`.
#[cfg(feature = "unicode")]
pub type Tchar = u16;

/// The installer's character type: `u16` under `unicode`, `u8` under `ansi`.
#[cfg(all(feature = "ansi", not(feature = "unicode")))]
pub type Tchar = u8;

/// Reads a NUL-terminated string, never looking past `max` characters.
///
/// `pluginapi.c` reads with an unbounded `lstrcpy`; we bound every read at the
/// installer's `string_size`, which is the whole point of this crate.
///
/// # Safety
/// `ptr` must be readable for `max` elements.
pub unsafe fn read_bounded(ptr: *const Tchar, max: usize) -> String {
	let len = unsafe { strlen_bounded(ptr, max) };
	let units = unsafe { core::slice::from_raw_parts(ptr, len) };
	decode(units)
}

/// Length of the NUL-terminated string at `ptr`, capped at `max`.
///
/// # Safety
/// `ptr` must be readable for `max` elements.
pub unsafe fn strlen_bounded(ptr: *const Tchar, max: usize) -> usize {
	let mut len = 0;
	while len < max && unsafe { *ptr.add(len) } != 0 {
		len += 1;
	}
	len
}

/// Copies `src` into `dst` with `lstrcpyn(dst, src, capacity)` semantics:
/// at most `capacity - 1` characters plus a NUL terminator.
///
/// Returns `true` when the whole source fit.
///
/// # Safety
/// `dst` must be writable for `capacity` elements.
pub unsafe fn write_bounded(dst: *mut Tchar, capacity: usize, src: &[Tchar]) -> bool {
	if capacity == 0 {
		return src.is_empty();
	}
	let room = capacity - 1;
	let n = src.len().min(room);
	unsafe { core::ptr::copy_nonoverlapping(src.as_ptr(), dst, n) };
	unsafe { *dst.add(n) = 0 };
	src.len() <= room
}

// -- Unicode ----------------------------------------------------------------

/// Encodes a `&str` as installer characters, without a NUL terminator.
#[cfg(feature = "unicode")]
pub fn encode(s: &str) -> Vec<Tchar> {
	s.encode_utf16().collect()
}

/// Decodes installer characters into a `String`, replacing anything invalid.
#[cfg(feature = "unicode")]
pub fn decode(units: &[Tchar]) -> String {
	decode_utf16(units)
}

/// UTF-16 to `String`, replacing unpaired surrogates rather than failing.
#[cfg(any(feature = "unicode", target_os = "windows"))]
fn decode_utf16(units: &[u16]) -> String {
	char::decode_utf16(units.iter().copied())
		.map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER))
		.collect()
}

// -- ANSI -------------------------------------------------------------------

#[cfg(all(feature = "ansi", not(feature = "unicode"), target_os = "windows"))]
mod ansi {
	use alloc::string::String;
	use alloc::vec;
	use alloc::vec::Vec;
	use core::ffi::{c_char, c_int, c_void};

	const CP_ACP: u32 = 0;

	#[link(name = "kernel32")]
	unsafe extern "system" {
		fn MultiByteToWideChar(
			codepage: u32,
			flags: u32,
			mbstr: *const c_char,
			cbmb: c_int,
			wcstr: *mut u16,
			cchwc: c_int,
		) -> c_int;
		fn WideCharToMultiByte(
			codepage: u32,
			flags: u32,
			wcstr: *const u16,
			cchwc: c_int,
			mbstr: *mut c_char,
			cbmb: c_int,
			default_char: *const c_char,
			used_default: *mut c_void,
		) -> c_int;
	}

	pub fn encode(s: &str) -> Vec<u8> {
		let wide: Vec<u16> = s.encode_utf16().collect();
		if wide.is_empty() {
			return Vec::new();
		}
		let len = wide.len() as c_int;
		// Explicit length, so no NUL terminator is appended.
		let needed = unsafe {
			WideCharToMultiByte(
				CP_ACP,
				0,
				wide.as_ptr(),
				len,
				core::ptr::null_mut(),
				0,
				core::ptr::null(),
				core::ptr::null_mut(),
			)
		};
		if needed <= 0 {
			return Vec::new();
		}
		let mut buf = vec![0u8; needed as usize];
		unsafe {
			WideCharToMultiByte(
				CP_ACP,
				0,
				wide.as_ptr(),
				len,
				buf.as_mut_ptr().cast(),
				needed,
				core::ptr::null(),
				core::ptr::null_mut(),
			)
		};
		buf
	}

	pub fn decode(units: &[u8]) -> String {
		if units.is_empty() {
			return String::new();
		}
		let len = units.len() as c_int;
		let needed = unsafe {
			MultiByteToWideChar(
				CP_ACP,
				0,
				units.as_ptr().cast(),
				len,
				core::ptr::null_mut(),
				0,
			)
		};
		if needed <= 0 {
			return String::new();
		}
		let mut wide = vec![0u16; needed as usize];
		unsafe {
			MultiByteToWideChar(
				CP_ACP,
				0,
				units.as_ptr().cast(),
				len,
				wide.as_mut_ptr(),
				needed,
			)
		};
		super::decode_utf16(&wide)
	}
}

/// Host stand-in for the `CP_ACP` codepage, so ANSI logic is testable off
/// Windows. Latin-1 is not `CP_ACP`, and this path is never compiled into a
/// shipped plug-in — it exists only for `cargo test` on macOS and Linux.
#[cfg(all(feature = "ansi", not(feature = "unicode"), not(target_os = "windows")))]
mod ansi {
	use alloc::string::String;
	use alloc::vec::Vec;

	pub fn encode(s: &str) -> Vec<u8> {
		s.chars()
			.map(|c| if (c as u32) < 0x100 { c as u8 } else { b'?' })
			.collect()
	}

	pub fn decode(units: &[u8]) -> String {
		units.iter().map(|&b| b as char).collect()
	}
}

/// Encodes a `&str` as installer characters, without a NUL terminator.
#[cfg(all(feature = "ansi", not(feature = "unicode")))]
pub fn encode(s: &str) -> Vec<Tchar> {
	ansi::encode(s)
}

/// Decodes installer characters into a `String`, replacing anything invalid.
#[cfg(all(feature = "ansi", not(feature = "unicode")))]
pub fn decode(units: &[Tchar]) -> String {
	ansi::decode(units)
}
