//! NSIS integer semantics, ported line-for-line from `Contrib/ExDLL/pluginapi.c`.
//!
//! `str::parse` is *not* equivalent and must not be used. NSIS accepts `0x`/`0X`
//! hex, leading-zero octal, and signed decimal; it stops at the first character
//! it does not recognise instead of failing; it wraps on overflow; and an
//! unparseable string is `0`, not an error. Scripts depend on all of that.

use alloc::vec::Vec;

use crate::tchar::Tchar;

/// The `TCHAR buf[128]` in `popintptr`/`popint_or`, which `popstringn` fills
/// with `lstrcpyn`. Anything past 127 characters is invisible to NSIS, and is
/// invisible here too.
const PARSE_BUFFER: usize = 128;

/// The part of `units` NSIS would parse: at most `PARSE_BUFFER - 1` characters.
pub(crate) fn parse_window(units: &[Tchar]) -> &[Tchar] {
	&units[..units.len().min(PARSE_BUFFER - 1)]
}

/// Character at `i`, or NUL past the end — the C reads a NUL-terminated buffer.
#[inline]
fn at(s: &[Tchar], i: usize) -> u32 {
	s.get(i).map_or(0, |&c| u32::from(c))
}

/// Scans one integer, returning its value and the index that stopped the scan.
///
/// This is `nsishelper_str_to_ptr` with the terminating position exposed, which
/// is what `myatoi_or` needs in order to look for a `|`.
fn scan(s: &[Tchar]) -> (isize, usize) {
	const ZERO: u32 = b'0' as u32;
	let mut v: isize = 0;

	// 0x / 0X hexadecimal
	if at(s, 0) == ZERO && (at(s, 1) == b'x' as u32 || at(s, 1) == b'X' as u32) {
		let mut i = 1;
		loop {
			i += 1;
			let c = at(s, i);
			let digit = match c {
				0x30..=0x39 => c - 0x30,
				0x61..=0x66 => c - 0x61 + 10,
				0x41..=0x46 => c - 0x41 + 10,
				_ => break,
			};
			v = (v << 4).wrapping_add(digit as isize);
		}
		return (v, i);
	}

	// Leading-zero octal
	if at(s, 0) == ZERO && (ZERO..=b'7' as u32).contains(&at(s, 1)) {
		let mut i = 0;
		loop {
			i += 1;
			let c = at(s, i);
			if !(ZERO..=b'7' as u32).contains(&c) {
				break;
			}
			v = (v << 3).wrapping_add((c - ZERO) as isize);
		}
		return (v, i);
	}

	// Signed decimal. The C does `if (*s == '-') sign++; else s--;` so that the
	// following `*(++s)` lands on the first digit either way.
	let negative = at(s, 0) == b'-' as u32;
	let mut i = usize::from(negative);
	loop {
		let c = at(s, i);
		if !(ZERO..=b'9' as u32).contains(&c) {
			break;
		}
		v = v.wrapping_mul(10).wrapping_add((c - ZERO) as isize);
		i += 1;
	}
	if negative {
		v = v.wrapping_neg();
	}
	(v, i)
}

/// `nsishelper_str_to_ptr`: NSIS's own string-to-integer conversion.
pub fn str_to_ptr(s: &[Tchar]) -> isize {
	scan(s).0
}

/// `myatoi_or`: like [`str_to_ptr`], but ORs together `2|4|8` forms.
///
/// The C recurses; this iterates, which is the same result without a stack
/// depth proportional to the input.
pub fn atoi_or(s: &[Tchar]) -> isize {
	let mut acc: isize = 0;
	let mut rest = s;
	loop {
		let (v, end) = scan(rest);
		acc |= v;
		if at(rest, end) != b'|' as u32 {
			return acc;
		}
		rest = &rest[end + 1..];
	}
}

/// Formats an integer the way `pushintptr` does: plain signed decimal.
pub fn format(v: isize) -> Vec<Tchar> {
	// `isize::MIN` has no positive counterpart, so work in the unsigned domain.
	let negative = v < 0;
	let mut magnitude = if negative {
		(v as usize).wrapping_neg()
	} else {
		v as usize
	};

	let mut digits = [0u8; 24];
	let mut n = 0;
	loop {
		digits[n] = b'0' + (magnitude % 10) as u8;
		magnitude /= 10;
		n += 1;
		if magnitude == 0 {
			break;
		}
	}

	let mut out = Vec::with_capacity(n + 1);
	if negative {
		out.push(b'-' as Tchar);
	}
	for i in (0..n).rev() {
		out.push(digits[i] as Tchar);
	}
	out
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::tchar::encode;

	fn parse(s: &str) -> isize {
		str_to_ptr(&encode(s))
	}

	fn parse_or(s: &str) -> isize {
		atoi_or(&encode(s))
	}

	#[test]
	fn decimal() {
		assert_eq!(parse("0"), 0);
		assert_eq!(parse("42"), 42);
		assert_eq!(parse("-42"), -42);
		assert_eq!(parse("9abc"), 9);
	}

	#[test]
	fn hexadecimal() {
		assert_eq!(parse("0x1f"), 31);
		assert_eq!(parse("0X1F"), 31);
		assert_eq!(parse("0xdeadg"), 0xdead);
		// `0x` with no digits is zero, not an error.
		assert_eq!(parse("0x"), 0);
	}

	#[test]
	fn octal() {
		assert_eq!(parse("0755"), 0o755);
		assert_eq!(parse("017"), 15);
		// `08` is not octal: the second character is out of range, so the
		// decimal branch runs and consumes both digits.
		assert_eq!(parse("08"), 8);
	}

	#[test]
	fn garbage_is_zero_not_an_error() {
		assert_eq!(parse(""), 0);
		assert_eq!(parse("hello"), 0);
		assert_eq!(parse("-"), 0);
		assert_eq!(parse(" 12"), 0);
	}

	#[test]
	fn ored_forms() {
		assert_eq!(parse_or("2|4|8"), 14);
		assert_eq!(parse_or("0x1|0x2"), 3);
		assert_eq!(parse_or("5"), 5);
		// A trailing bar contributes nothing.
		assert_eq!(parse_or("1|"), 1);
	}

	#[test]
	fn or_is_not_applied_by_the_plain_parser() {
		assert_eq!(parse("2|4|8"), 2);
	}

	#[test]
	fn round_trip() {
		for v in [0isize, 1, -1, 12345, -12345, isize::MAX, isize::MIN] {
			assert_eq!(str_to_ptr(&format(v)), v, "round-tripping {v}");
		}
	}
}
