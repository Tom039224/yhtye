; Hooks of the NSIS installer (`bundle.windows.nsis.installerHooks` in tauri.conf.json).
;
; Tauri's installer keeps the install directory in HKCU\Software\<publisher>\<product> (the default
; value) for the next install, and its uninstaller removes it only when "delete the application data"
; is ticked. Left behind, it is read by a later MSI (per machine, AppSearch), which then installs into
; %LOCALAPPDATA%. So a real uninstall removes it; an update (/UPDATE) keeps it. A reinstall or an
; upgrade over an older version still lands in the same directory: the installer reads it before it runs
; the old uninstaller, and writes it again afterwards.
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    DeleteRegValue HKCU "${MANUPRODUCTKEY}" ""
    DeleteRegValue HKCU "${MANUPRODUCTKEY}" "Installer Language"
    DeleteRegKey /ifempty HKCU "${MANUPRODUCTKEY}"
    DeleteRegKey /ifempty HKCU "${MANUKEY}"
  ${EndIf}
!macroend
