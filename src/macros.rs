//! The three macros that make up the plug-in surface.

/// Declares the crate to be an NSIS plug-in DLL. Invoke exactly once.
///
/// Generates the `DllMain` that stashes the module handle, a `#[panic_handler]`
/// that terminates rather than unwinding into the exehead, and a
/// `#[global_allocator]` on the process heap so the DLL links without a C
/// runtime.
///
/// Everything it emits is gated on `target_os = "windows"`, so a plug-in crate
/// still `cargo check`s and `cargo clippy`s on macOS and Linux. Pair it with
/// `#![cfg_attr(target_os = "windows", no_std)]`.
///
/// ```ignore
/// nsis_plugin!();       // no_std: panic handler and allocator included
/// nsis_plugin!(std);    // std build: only DllMain
/// ```
///
/// A `std` build links the C runtime, which owns the PE entry point
/// (`DllMainCRTStartup`), initialises itself and then calls `DllMain`. So the
/// `std` form defines `DllMain` instead; defining the entry point too is a
/// duplicate symbol.
#[macro_export]
macro_rules! nsis_plugin {
	() => {
		$crate::__nsis_dllmain!(DllMainCRTStartup);

		#[cfg(target_os = "windows")]
		#[panic_handler]
		fn __nsis_panic(_info: &::core::panic::PanicInfo) -> ! {
			$crate::rt::abort()
		}

		#[cfg(target_os = "windows")]
		#[global_allocator]
		static __NSIS_ALLOCATOR: $crate::rt::NsisAllocator = $crate::rt::NsisAllocator;

		$crate::__nsis_unwind_stubs!();
	};
	(std) => {
		$crate::__nsis_dllmain!(DllMain);

		/// Named by the landing pads in the precompiled `std`. Rust's i686
		/// target expects a DWARF-unwinding mingw, but many i686 mingw builds
		/// (Homebrew's among them) unwind with SJLJ, and their `libgcc_eh`
		/// only has `_Unwind_SjLj_Resume`. With `panic = "abort"` nothing
		/// unwinds, so the landing pads are dead code. On a DWARF toolchain
		/// this definition keeps the archive member out, and nothing else in a
		/// `panic = "abort"` DLL asks for it.
		#[cfg(all(target_os = "windows", target_env = "gnu", target_arch = "x86"))]
		#[unsafe(no_mangle)]
		extern "C" fn _Unwind_Resume() -> ! {
			$crate::rt::unwind_unreachable()
		}
	};
}

/// The symbols a `no_std` plug-in needs in place of `std` and the CRT.
///
/// These are expanded into the plug-in crate, not defined in `nsis-plugin`,
/// and that placement is load-bearing — see the note in `rt.rs`. Nothing
/// references them in the IR, so as upstream symbols fat LTO would internalise
/// and drop them, and the DLL would fail to link on exactly these names.
#[doc(hidden)]
#[macro_export]
macro_rules! __nsis_unwind_stubs {
	() => {
		/// Named by the unwind tables in the precompiled `core` and `alloc`.
		#[cfg(target_os = "windows")]
		#[unsafe(no_mangle)]
		extern "C" fn rust_eh_personality() {}

		/// The MSVC personality, named by the abort-on-unwind guard the
		/// compiler attaches to every `extern "C"` export. `vcruntime` defines
		/// it; `/NODEFAULTLIB` removes `vcruntime`.
		#[cfg(all(target_os = "windows", target_env = "msvc"))]
		#[unsafe(no_mangle)]
		extern "C" fn __CxxFrameHandler3() -> ! {
			$crate::rt::unwind_unreachable()
		}

		/// Named by mingw's unwinder on the GNU targets.
		#[cfg(all(target_os = "windows", target_env = "gnu"))]
		#[unsafe(no_mangle)]
		extern "C" fn _Unwind_Resume() -> ! {
			$crate::rt::unwind_unreachable()
		}
	};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __nsis_dllmain {
	($entry:ident) => {
		/// PE entry point, or the `DllMain` the CRT's entry point calls.
		/// Returning 0 would make `LoadLibrary` fail.
		#[cfg(target_os = "windows")]
		#[unsafe(no_mangle)]
		pub extern "system" fn $entry(
			hinst: *mut ::core::ffi::c_void,
			reason: u32,
			_reserved: *mut ::core::ffi::c_void,
		) -> i32 {
			const DLL_PROCESS_ATTACH: u32 = 1;
			if reason == DLL_PROCESS_ATTACH {
				$crate::rt::set_hinstance(hinst);
			}
			1
		}
	};
}

/// Defines one or more plug-in exports.
///
/// Each body gets a `&mut Nsis` and returns [`Result<()>`](crate::Result).
/// Returning `Err` sets `exec_flags->exec_error`, so `IfErrors` works in the
/// calling script; `Ok` leaves the flag untouched.
///
/// The export name is the function name, verbatim and undecorated — that is
/// what `plugin::Name` resolves to in an `.nsi` script.
///
/// ```ignore
/// nsis_fn! {
///     fn Add(nsis: &mut Nsis) -> Result<()> {
///         let b = nsis.stack.pop_int()?;
///         let a = nsis.stack.pop_int()?;
///         nsis.stack.push_int(a + b)?;
///         Ok(())
///     }
///
///     fn Reverse(nsis: &mut Nsis) -> Result<()> {
///         let s = nsis.stack.pop()?;
///         nsis.stack.push(&s.chars().rev().collect::<alloc::string::String>())
///     }
/// }
/// ```
///
/// Arguments arrive on the stack, not in the signature: the five-pointer
/// boundary is fixed by NSIS and the same for every export.
#[macro_export]
macro_rules! nsis_fn {
	($(
		$(#[$attr:meta])*
		fn $name:ident($nsis:ident: &mut Nsis) -> Result<()> $body:block
	)*) => {$(
		$(#[$attr])*
		#[unsafe(no_mangle)]
		pub unsafe extern "C" fn $name(
			hwnd: $crate::Hwnd,
			string_size: ::core::ffi::c_int,
			variables: *mut $crate::Tchar,
			stacktop: *mut *mut $crate::StackNode,
			extra: *mut $crate::ExtraParameters,
		) {
			// A plain nested fn, not a closure: nothing from the environment
			// can leak in, and `?` cannot escape past `__nsis_body`. The
			// signature is written with the caller's own `Nsis` and `Result`,
			// so the source reads as the real thing rather than as a pattern.
			fn __nsis_body($nsis: &mut Nsis) -> Result<()> $body

			// SAFETY: these are the installer's own arguments, by construction.
			let mut nsis = unsafe {
				$crate::Nsis::from_raw(hwnd, string_size, variables, stacktop, extra)
			};
			let outcome = __nsis_body(&mut nsis);
			if outcome.is_err() {
				nsis.set_error(true);
			}
		}
	)*};
}

/// Defines an `NSPIM_UNLOAD` callback, to be handed to
/// [`Nsis::register_callback`](crate::Nsis::register_callback).
///
/// This is where cleanup belongs: `/NOUNLOAD` and `SetPluginsUnload` were
/// deprecated in NSIS 3, and plug-ins now stay loaded for the life of the
/// installer. `api.h` documents `NSPIM_UNLOAD` as the last message a plug-in
/// receives.
///
/// ```ignore
/// nsis_unload! {
///     fn cleanup() {
///         // release anything held across calls
///     }
/// }
///
/// nsis_fn! {
///     fn Init(nsis: &mut Nsis) -> Result<()> {
///         nsis.register_callback(cleanup)?;
///         Ok(())
///     }
/// }
/// ```
#[macro_export]
macro_rules! nsis_unload {
	($(
		$(#[$attr:meta])*
		fn $name:ident() $body:block
	)*) => {$(
		$(#[$attr])*
		///
		/// Registered with `RegisterPluginCallback`; not a DLL export.
		pub unsafe extern "C" fn $name(message: ::core::ffi::c_int) -> usize {
			if message == $crate::NSPIM_UNLOAD {
				fn __nsis_unload_body() $body
				__nsis_unload_body();
			}
			// Unknown messages must return 0.
			0
		}
	)*};
}
