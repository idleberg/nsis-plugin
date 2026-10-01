; Smoke test for {{project-name}}.
;
; Exercises the export names, the calling convention and the stack protocol
; through a real installer — the failures host-side unit tests structurally
; cannot see. Run it with `mise run smoke`.

!include "LogicLib.nsh"

!ifndef PLUGINDIR
	!error "define PLUGINDIR: the Plugins/<variant> directory holding the DLL"
!endif
!ifndef OUTFILE
	!define OUTFILE "smoke.exe"
!endif

!addplugindir "${PLUGINDIR}"

Name "{{project-name}} smoke test"
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

Section "Smoke"
	StrCpy $FAILURES 0
	FileOpen $LOG "$EXEDIR\smoke.log" w

	Push "world"
	{{crate_name}}::Hello
	Pop $0
	!insertmacro Expect "Hello" "Hello, world!"

	${If} $FAILURES == 0
		FileWrite $LOG "ALL PASSED$\r$\n"
	${Else}
		FileWrite $LOG "$FAILURES FAILED$\r$\n"
	${EndIf}
	FileClose $LOG

	SetErrorLevel $FAILURES
SectionEnd
