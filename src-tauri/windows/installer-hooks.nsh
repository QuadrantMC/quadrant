; Tauri NSIS installer hooks, merged in by src-tauri/tauri.cli.conf.json.
; They put the install folder, where the quadrantmc sidecar lands, on the
; per-user PATH. user-path.ps1 holds the logic so it can be tested outside NSIS.

; Expanded here rather than inside the macro, where it would name the folder
; of installer.nsi, which inserts the macro.
!define QUADRANT_HOOKS_DIR "${__FILEDIR__}"

!macro QUADRANT_USER_PATH ACTION
  Push $0
  InitPluginsDir
  File "/oname=$PLUGINSDIR\user-path.ps1" "${QUADRANT_HOOKS_DIR}\user-path.ps1"
  nsExec::Exec '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\user-path.ps1" -Action ${ACTION} -Directory "$INSTDIR"'
  Pop $0
  ${If} $0 != 0
    DetailPrint "Could not update PATH for quadrantmc (exit code $0)"
  ${EndIf}
  ; WM_SETTINGCHANGE to all top-level windows, so new terminals see the change.
  SendMessage 0xFFFF 0x1A 0 "STR:Environment" /TIMEOUT=5000
  Pop $0
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro QUADRANT_USER_PATH Add
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro QUADRANT_USER_PATH Remove
!macroend
