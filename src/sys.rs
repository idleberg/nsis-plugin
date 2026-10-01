//! The two allocation primitives the NSIS stack protocol is built on.
//!
//! Stack nodes are owned by whoever pops them, and the installer allocates them
//! with `GlobalAlloc(GPTR, ...)`. We must therefore use the same pair of
//! functions, not the Rust allocator.
//!
//! On non-Windows hosts these are emulated on top of the Rust allocator so the
//! stack and variable code — the code worth testing — runs unchanged under
//! `cargo test` on macOS and Linux.

#![allow(
	clippy::undocumented_unsafe_blocks,
	reason = "this module is the raw allocator; each function documents its \
	          own contract under `# Safety`"
)]

#[cfg(target_os = "windows")]
mod imp {
	use core::ffi::c_void;

	/// `GMEM_FIXED | GMEM_ZEROINIT`, what `pluginapi.c` passes.
	const GPTR: u32 = 0x0040;

	#[link(name = "kernel32")]
	unsafe extern "system" {
		fn GlobalAlloc(uflags: u32, dwbytes: usize) -> *mut c_void;
		fn GlobalFree(hmem: *mut c_void) -> *mut c_void;
	}

	/// Allocates `size` zeroed bytes with the allocator NSIS uses for the stack.
	///
	/// # Safety
	/// The result must be released with [`free`] and nothing else.
	pub unsafe fn alloc_zeroed(size: usize) -> *mut u8 {
		unsafe { GlobalAlloc(GPTR, size) }.cast()
	}

	/// Releases a pointer obtained from [`alloc_zeroed`] or from the installer.
	///
	/// # Safety
	/// `ptr` must come from `GlobalAlloc` and must not be used afterwards.
	pub unsafe fn free(ptr: *mut u8) {
		if !ptr.is_null() {
			unsafe { GlobalFree(ptr.cast()) };
		}
	}
}

#[cfg(not(target_os = "windows"))]
mod imp {
	use alloc::alloc::{Layout, alloc_zeroed as rust_alloc_zeroed, dealloc};

	/// `GlobalFree` takes no size, so the emulation stashes one in a header.
	const HEADER: usize = core::mem::size_of::<usize>();
	const ALIGN: usize = core::mem::align_of::<usize>();

	/// Host-side stand-in for `GlobalAlloc(GPTR, size)`.
	///
	/// # Safety
	/// The result must be released with [`free`] and nothing else.
	pub unsafe fn alloc_zeroed(size: usize) -> *mut u8 {
		let total = HEADER + size;
		let Ok(layout) = Layout::from_size_align(total, ALIGN) else {
			return core::ptr::null_mut();
		};
		let base = unsafe { rust_alloc_zeroed(layout) };
		if base.is_null() {
			return base;
		}
		unsafe { base.cast::<usize>().write(total) };
		unsafe { base.add(HEADER) }
	}

	/// Host-side stand-in for `GlobalFree`.
	///
	/// # Safety
	/// `ptr` must come from [`alloc_zeroed`] and must not be used afterwards.
	pub unsafe fn free(ptr: *mut u8) {
		if ptr.is_null() {
			return;
		}
		let base = unsafe { ptr.sub(HEADER) };
		let total = unsafe { base.cast::<usize>().read() };
		let layout = Layout::from_size_align(total, ALIGN).expect("valid layout");
		unsafe { dealloc(base, layout) };
	}
}

pub use imp::{alloc_zeroed, free};
