//! The runtime pieces a `no_std` plug-in DLL needs in order to link at all.
//!
//! [`nsis_plugin!`](crate::nsis_plugin) wires all of this up; you rarely name
//! anything here directly. Everything is Windows-only, so a plug-in crate still
//! `cargo check`s and `cargo test`s on macOS and Linux.

#![allow(clippy::missing_safety_doc, reason = "these are C intrinsics")]

use core::ffi::c_void;
use core::sync::atomic::{AtomicUsize, Ordering};

/// `HINSTANCE` of the loaded plug-in, stashed by `DllMain`.
static HINSTANCE: AtomicUsize = AtomicUsize::new(0);

/// Records the module handle. Called from the generated `DllMain`.
pub fn set_hinstance(hinst: *mut c_void) {
	HINSTANCE.store(hinst as usize, Ordering::Relaxed);
}

/// The plug-in's own module handle, as `RegisterPluginCallback` requires.
///
/// Null until `DllMain` has run, which cannot happen before an export is
/// called.
#[must_use]
pub fn hinstance() -> *mut c_void {
	HINSTANCE.load(Ordering::Relaxed) as *mut c_void
}

// -- Termination ------------------------------------------------------------

// `#[link]` is what puts `kernel32.lib` on the link line. `std` normally
// requests it; a `no_std` cdylib has to ask. On MSVC the request is the only
// thing that survives `/NODEFAULTLIB`, which drops the CRT's own
// `/defaultlib:kernel32.lib` directive along with the CRT.
#[cfg(target_os = "windows")]
#[link(name = "kernel32")]
unsafe extern "system" {
	fn GetCurrentProcess() -> *mut c_void;
	fn TerminateProcess(process: *mut c_void, exit_code: u32) -> i32;
}

/// Ends the process.
///
/// Unwinding across the plug-in boundary into the exehead is undefined
/// behaviour, so a panic in a plug-in has nowhere to go. Terminating is at
/// least defined; the real answer is to return `Err` rather than panic.
#[cfg(target_os = "windows")]
pub fn abort() -> ! {
	// SAFETY: `GetCurrentProcess` returns a pseudo-handle that is always valid
	// for the calling process; neither call has further preconditions.
	unsafe { TerminateProcess(GetCurrentProcess(), 0xC000_0409) };
	// Unreachable in practice, but `TerminateProcess` is not `-> !`.
	#[allow(clippy::empty_loop, reason = "process is already terminating")]
	loop {}
}

/// Ends the process. Host builds defer to the standard abort.
#[cfg(all(not(target_os = "windows"), feature = "std"))]
pub fn abort() -> ! {
	std::process::abort()
}

/// Ends the process. Host `no_std` builds have nothing better than a spin.
#[cfg(all(not(target_os = "windows"), not(feature = "std")))]
pub fn abort() -> ! {
	#[allow(clippy::empty_loop, reason = "no_std host build has no abort")]
	loop {}
}

// -- Unwinding stubs --------------------------------------------------------
//
// The precompiled `core` and `alloc` carry unwind tables that name
// `rust_eh_personality`, even in a `panic = "abort"` build, and every
// `extern "C"` export carries a guard that aborts if an unwind ever reaches
// it — on the MSVC targets that guard names `__CxxFrameHandler3`. Normally
// `std` and the CRT supply these; a `no_std` cdylib linked with
// `/NODEFAULTLIB` has neither.
//
// The definitions themselves are emitted by `nsis_plugin!()`, into the
// plug-in crate rather than into this one. They cannot live here: nothing
// references them in the IR — the reference is attached to the exports by the
// backend, after LTO has run — so fat LTO internalises them as unreachable
// upstream symbols and drops them, and the link fails on the very symbol this
// module was meant to provide. The memory intrinsics below are safe here
// because IR calls them by name.
//
// None of the three can be reached: a panic goes to the `#[panic_handler]`,
// which never returns.

/// The body behind the unwinding stubs `nsis_plugin!()` emits.
///
/// Not meant to be called; it exists so the generated stubs stay one line
/// each. Aborting is the honest answer — a plug-in has no unwinder.
#[doc(hidden)]
pub fn unwind_unreachable() -> ! {
	abort()
}

// -- Global allocator -------------------------------------------------------

#[cfg(target_os = "windows")]
mod allocator {
	use core::alloc::{GlobalAlloc, Layout};
	use core::ffi::c_void;
	use core::sync::atomic::{AtomicUsize, Ordering};

	const HEAP_ZERO_MEMORY: u32 = 0x0000_0008;

	/// `MEMORY_ALLOCATION_ALIGNMENT`: what `HeapAlloc` already guarantees.
	const NATIVE_ALIGN: usize = if cfg!(target_pointer_width = "64") {
		16
	} else {
		8
	};

	#[link(name = "kernel32")]
	unsafe extern "system" {
		fn GetProcessHeap() -> *mut c_void;
		fn HeapAlloc(heap: *mut c_void, flags: u32, bytes: usize) -> *mut c_void;
		fn HeapReAlloc(
			heap: *mut c_void,
			flags: u32,
			mem: *mut c_void,
			bytes: usize,
		) -> *mut c_void;
		fn HeapFree(heap: *mut c_void, flags: u32, mem: *mut c_void) -> i32;
	}

	static HEAP: AtomicUsize = AtomicUsize::new(0);

	fn heap() -> *mut c_void {
		let cached = HEAP.load(Ordering::Relaxed);
		if cached != 0 {
			return cached as *mut c_void;
		}
		// SAFETY: `GetProcessHeap` has no preconditions.
		let h = unsafe { GetProcessHeap() };
		HEAP.store(h as usize, Ordering::Relaxed);
		h
	}

	/// A `GlobalAlloc` on the process heap.
	///
	/// The process heap is the smallest allocator available to a plug-in — it
	/// needs no CRT and no initialisation, so the DLL links without a C
	/// runtime.
	pub struct NsisAllocator;

	/// Header holding the real base pointer, for over-aligned allocations.
	const HEADER: usize = core::mem::size_of::<usize>();

	impl NsisAllocator {
		unsafe fn alloc_flagged(layout: Layout, flags: u32) -> *mut u8 {
			if layout.align() <= NATIVE_ALIGN {
				// SAFETY: `heap()` is the process heap and `flags` is 0 or
				// `HEAP_ZERO_MEMORY`; `HeapAlloc` reports failure as null.
				return unsafe { HeapAlloc(heap(), flags, layout.size()) }.cast();
			}
			// Over-allocate, then place the aligned pointer and record the base
			// just below it.
			let total = layout.size() + layout.align() + HEADER;
			// SAFETY: as above.
			let base: *mut u8 = unsafe { HeapAlloc(heap(), flags, total) }.cast();
			if base.is_null() {
				return base;
			}
			let candidate = base as usize + HEADER;
			let aligned = (candidate + layout.align() - 1) & !(layout.align() - 1);
			let ptr = aligned as *mut u8;
			// SAFETY: `base + HEADER <= aligned < base + HEADER + align`, and
			// `total` reserves `align + HEADER` bytes beyond `size`, so both the
			// header slot and the `size` bytes after `aligned` lie inside the
			// block. `aligned` is a multiple of `align > NATIVE_ALIGN`, hence of
			// `align_of::<usize>() == HEADER`, so the slot is aligned for `usize`.
			unsafe { ptr.sub(HEADER).cast::<usize>().write(base as usize) };
			ptr
		}

		unsafe fn base_of(ptr: *mut u8, layout: Layout) -> *mut u8 {
			if layout.align() <= NATIVE_ALIGN {
				ptr
			} else {
				// SAFETY: the caller passes a pointer `alloc_flagged` returned for
				// this `layout`, which wrote the base into this slot.
				unsafe { ptr.sub(HEADER).cast::<usize>().read() as *mut u8 }
			}
		}
	}

	// SAFETY: blocks up to `NATIVE_ALIGN` come straight from `HeapAlloc`, which
	// guarantees that alignment; larger alignments go through the header path
	// in `alloc_flagged`. `dealloc` and `realloc` recover the pointer `HeapAlloc`
	// returned via `base_of`, so the heap only ever sees its own pointers.
	unsafe impl GlobalAlloc for NsisAllocator {
		unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
			// SAFETY: forwards `GlobalAlloc::alloc`'s contract unchanged.
			unsafe { Self::alloc_flagged(layout, 0) }
		}

		unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
			// SAFETY: forwards `GlobalAlloc::alloc_zeroed`'s contract unchanged.
			unsafe { Self::alloc_flagged(layout, HEAP_ZERO_MEMORY) }
		}

		unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
			// SAFETY: `GlobalAlloc::dealloc` requires `ptr` to come from this
			// allocator with this `layout`, which is what `base_of` needs.
			let base = unsafe { Self::base_of(ptr, layout) };
			// SAFETY: `base` is the pointer `HeapAlloc` returned on this heap.
			unsafe { HeapFree(heap(), 0, base.cast()) };
		}

		unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
			if layout.align() <= NATIVE_ALIGN {
				// SAFETY: with no header, `ptr` is `HeapAlloc`'s own pointer on
				// this heap.
				return unsafe { HeapReAlloc(heap(), 0, ptr.cast(), new_size) }.cast();
			}
			// Over-aligned: the header makes in-place growth unsafe, so move.
			let Ok(new_layout) = Layout::from_size_align(new_size, layout.align()) else {
				return core::ptr::null_mut();
			};
			// SAFETY: `GlobalAlloc::realloc` guarantees `new_size` is non-zero.
			let new_ptr = unsafe { self.alloc(new_layout) };
			if !new_ptr.is_null() {
				// SAFETY: `ptr` is valid for `layout.size()` bytes and `new_ptr`
				// for `new_size`; they are distinct live blocks, so they do not
				// overlap. `ptr` came from this allocator with `layout`.
				unsafe {
					core::ptr::copy_nonoverlapping(ptr, new_ptr, layout.size().min(new_size));
					self.dealloc(ptr, layout);
				}
			}
			new_ptr
		}
	}
}

#[cfg(target_os = "windows")]
pub use allocator::NsisAllocator;

// -- Memory intrinsics ------------------------------------------------------
//
// A plug-in links with `-nodefaultlibs` to keep the DLL small, which removes
// the CRT's `memcpy` and friends. LLVM still emits calls to them, so provide
// them here. Turn off the `mem-intrinsics` feature if you link a CRT that
// already has them.

#[cfg(all(target_os = "windows", feature = "mem-intrinsics"))]
#[allow(
	clippy::undocumented_unsafe_blocks,
	reason = "every block rests on the same C contract: the caller guarantees \
	          `n` valid bytes behind each pointer"
)]
#[allow(
	suspicious_runtime_symbol_definitions,
	reason = "`*mut u8` is ABI-identical to `*mut c_void` and keeps the byte \
	          loops free of casts"
)]
mod mem {
	#[unsafe(no_mangle)]
	unsafe extern "C" fn memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
		let mut i = 0;
		while i < n {
			unsafe { *dest.add(i) = *src.add(i) };
			i += 1;
		}
		dest
	}

	#[unsafe(no_mangle)]
	unsafe extern "C" fn memmove(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
		if (dest as usize) < (src as usize) {
			let mut i = 0;
			while i < n {
				unsafe { *dest.add(i) = *src.add(i) };
				i += 1;
			}
		} else {
			let mut i = n;
			while i != 0 {
				i -= 1;
				unsafe { *dest.add(i) = *src.add(i) };
			}
		}
		dest
	}

	#[unsafe(no_mangle)]
	unsafe extern "C" fn memset(dest: *mut u8, c: i32, n: usize) -> *mut u8 {
		let byte = c as u8;
		let mut i = 0;
		while i < n {
			unsafe { *dest.add(i) = byte };
			i += 1;
		}
		dest
	}

	#[unsafe(no_mangle)]
	unsafe extern "C" fn memcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
		let mut i = 0;
		while i < n {
			let (x, y) = unsafe { (*a.add(i), *b.add(i)) };
			if x != y {
				return i32::from(x) - i32::from(y);
			}
			i += 1;
		}
		0
	}

	#[unsafe(no_mangle)]
	unsafe extern "C" fn bcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
		unsafe { memcmp(a, b, n) }
	}
}
