; Smoke test for the `hello` plug-in.
;
; Exercises the export names, the calling convention, the stack protocol and
; the error flag through a real installer — the failures that host-side unit
; tests structurally cannot see.
;
; Built by `cargo xtask smoke`, which supplies PLUGINDIR and OUTFILE and picks
; the variant with -XTarget. Results are written to smoke.log next to the
; installer so the run can be asserted on; the installer itself is silent.

!include "LogicLib.nsh"

!ifndef PLUGINDIR
	!error "define PLUGINDIR: the Plugins/<variant> directory holding hello.dll"
!endif
!ifndef OUTFILE
	!define OUTFILE "smoke.exe"
!endif

; No target switch: this picks up whichever target -XTarget selected.
!addplugindir "${PLUGINDIR}"

Name "nsis-plugin smoke test"
OutFile "${OUTFILE}"
RequestExecutionLevel user
SilentInstall silent
ShowInstDetails nevershow

Var LOG
Var FAILURES

!macro Expect name expected
	${If} $0 == "${expected}"
		FileWrite $LOG "ok   ${name}: $0$\r$\n"
	${Else}
		IntOp $FAILURES $FAILURES + 1
		FileWrite $LOG "FAIL ${name}: expected '${expected}', got '$0'$\r$\n"
	${EndIf}
!macroend

!macro ExpectNonZero name
	${If} $0 > 0
		FileWrite $LOG "ok   ${name}: $0$\r$\n"
	${Else}
		IntOp $FAILURES $FAILURES + 1
		FileWrite $LOG "FAIL ${name}: expected a positive value, got '$0'$\r$\n"
	${EndIf}
!macroend

Section "Smoke"
	StrCpy $FAILURES 0
	FileOpen $LOG "$EXEDIR\smoke.log" w

	; A string round trip, and proof the export name resolves at all.
	Push "world"
	hello::Hello
	Pop $0
	!insertmacro Expect "Hello" "Hello, world!"

	; Plain decimal arithmetic, and the argument order off the stack.
	Push 2
	Push 40
	hello::Add
	Pop $0
	!insertmacro Expect "Add/decimal" "42"

	; NSIS integer semantics: hex and leading-zero octal both mean 42.
	Push "0x2a"
	Push "052"
	hello::Add
	Pop $0
	!insertmacro Expect "Add/radix" "84"

	; Negative values, via NSIS's own sign handling.
	Push "-5"
	Push "3"
	hello::Add
	Pop $0
	!insertmacro Expect "Add/negative" "-2"

	; Character handling all the way through the boundary.
	Push "abcdef"
	hello::Reverse
	Pop $0
	!insertmacro Expect "Reverse" "fedcba"

	; The runtime string_size, which a hard-coded buffer could not report.
	hello::StringSize
	Pop $R0
	StrCpy $0 $R0
	!insertmacro ExpectNonZero "StringSize"
	FileWrite $LOG "note NSIS_MAX_STRLEN is $R0$\r$\n"

	; The longest string this installer can hold, built at run time from what
	; the plug-in just reported. Against a stock makensis that is 1023
	; characters; against /DNSIS_MAX_STRLEN=8192 it is 8191, and the same DLL
	; must handle both without a rebuild. This is the test the whole project
	; exists for.
	IntOp $R1 $R0 - 1
	StrCpy $1 ""
	StrCpy $2 0
	${Do}
		StrCpy $1 "$1abcdefghij"
		IntOp $2 $2 + 10
	${LoopUntil} $2 >= $R1
	StrCpy $1 $1 $R1
	StrLen $2 $1
	Push $1
	hello::Reverse
	Pop $0
	StrLen $3 $0
	${If} $3 == $2
		FileWrite $LOG "ok   Reverse/max: $3 of $R1 characters$\r$\n"
	${Else}
		IntOp $FAILURES $FAILURES + 1
		FileWrite $LOG "FAIL Reverse/max: expected $2 characters, got $3$\r$\n"
	${EndIf}

	; Variable access.
	StrCpy $INSTDIR "C:\smoke-test"
	hello::InstDir
	Pop $0
	!insertmacro Expect "InstDir" "C:\smoke-test"

	; An Err return must set the flag IfErrors reads.
	ClearErrors
	hello::Fail
	${If} ${Errors}
		FileWrite $LOG "ok   Fail: error flag set$\r$\n"
	${Else}
		IntOp $FAILURES $FAILURES + 1
		FileWrite $LOG "FAIL Fail: error flag not set$\r$\n"
	${EndIf}

	; ...and Ok must leave it alone.
	ClearErrors
	Push "quiet"
	hello::Hello
	Pop $0
	${If} ${Errors}
		IntOp $FAILURES $FAILURES + 1
		FileWrite $LOG "FAIL Hello/noerror: error flag set on success$\r$\n"
	${Else}
		FileWrite $LOG "ok   Hello/noerror: error flag untouched$\r$\n"
	${EndIf}

	${If} $FAILURES == 0
		FileWrite $LOG "ALL PASSED$\r$\n"
	${Else}
		FileWrite $LOG "$FAILURES FAILED$\r$\n"
	${EndIf}
	FileClose $LOG

	SetErrorLevel $FAILURES
SectionEnd
