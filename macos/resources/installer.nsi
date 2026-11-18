; nrzCode – Installer für Windows (NSIS 3, Unicode).
;
;   makensis -DVERSION=1.0.29 -DEXE=<Pfad>\nrzCode.exe -DOUT=<Pfad>\nrzCode-1.0.29-Setup.exe installer.nsi
;
; Installiert je Benutzer nach %LOCALAPPDATA%\Programs\nrzCode – ohne Administratorrechte, damit
; die App sich selbst aktualisieren kann (sie ersetzt ihre eigene nrzCode.exe). Legt Einträge im
; Startmenü (und auf Wunsch auf dem Desktop) an und trägt sich unter „Apps“ zum Deinstallieren ein.
; Daten (%APPDATA%\nrzCode) bleiben beim Deinstallieren erhalten.
; scripts/windows.sh bzw. scripts/windows.ps1 rufen das hier auf.

Unicode true
SetCompressor /SOLID lzma
RequestExecutionLevel user
ManifestDPIAware true

!ifndef VERSION
  !error "VERSION fehlt (-DVERSION=1.2.3)"
!endif
!ifndef EXE
  !error "EXE fehlt (-DEXE=Pfad zu nrzCode.exe)"
!endif
!ifndef OUT
  !define OUT "nrzCode-${VERSION}-Setup.exe"
!endif

!define APP "nrzCode"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\nrzCode"

Name "${APP}"
OutFile "${OUT}"
InstallDir "$LOCALAPPDATA\Programs\${APP}"
InstallDirRegKey HKCU "Software\${APP}" "InstallDir"
BrandingText "${APP} ${VERSION}"

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${APP}"
VIAddVersionKey "CompanyName" "nrzStudio"
VIAddVersionKey "FileDescription" "${APP} Setup"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "AGPL-3.0-or-later"

!include "MUI2.nsh"
!include "FileFunc.nsh"

!define MUI_ICON "nrzCode.ico"
!define MUI_UNICON "nrzCode.ico"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\nrzCode.exe"
!define MUI_FINISHPAGE_RUN_TEXT "$(RunNow)"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

; Erste Sprache = Vorgabe, wenn die Sprache von Windows nicht dabei ist
!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "German"

LangString RunNow ${LANG_GERMAN} "nrzCode jetzt starten"
LangString RunNow ${LANG_ENGLISH} "Start nrzCode now"
LangString SecApp ${LANG_GERMAN} "nrzCode (erforderlich)"
LangString SecApp ${LANG_ENGLISH} "nrzCode (required)"
LangString SecDesktop ${LANG_GERMAN} "Verknüpfung auf dem Desktop"
LangString SecDesktop ${LANG_ENGLISH} "Desktop shortcut"
LangString Running ${LANG_GERMAN} "nrzCode läuft noch. Bitte beende nrzCode (Fenster schliessen) und klicke dann auf „Wiederholen“."
LangString Running ${LANG_ENGLISH} "nrzCode is still running. Please quit nrzCode (close the window), then click “Retry”."
LangString KeepData ${LANG_GERMAN} "Deine Daten (Threads, Konten, Einstellungen) unter %APPDATA%\nrzCode bleiben erhalten."
LangString KeepData ${LANG_ENGLISH} "Your data (threads, accounts, settings) in %APPDATA%\nrzCode is kept."

; Solange nrzCode.exe läuft, lässt sie sich nicht zum Schreiben öffnen: warten, bis sie frei ist.
!macro DefineWaitUntilClosed UN
Function ${UN}WaitUntilClosed
  retry:
    IfFileExists "$INSTDIR\nrzCode.exe" 0 done
    ClearErrors
    FileOpen $0 "$INSTDIR\nrzCode.exe" a
    IfErrors busy
    FileClose $0
    Goto done
  busy:
    MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "$(Running)" /SD IDCANCEL IDRETRY retry
    Abort
  done:
FunctionEnd
!macroend
!insertmacro DefineWaitUntilClosed ""
!insertmacro DefineWaitUntilClosed "un."

Section "$(SecApp)" SEC_APP
  SectionIn RO
  Call WaitUntilClosed
  SetOutPath "$INSTDIR"
  ; Rest eines Updates der App (umbenanntes altes Programm)
  Delete "$INSTDIR\nrzCode.old.exe"
  File "/oname=nrzCode.exe" "${EXE}"
  WriteUninstaller "$INSTDIR\Uninstall.exe"

  CreateShortcut "$SMPROGRAMS\${APP}.lnk" "$INSTDIR\nrzCode.exe"

  WriteRegStr HKCU "Software\${APP}" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "${APP}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST_KEY}" "Publisher" "nrzStudio"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\nrzCode.exe"
  WriteRegStr HKCU "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINST_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "${UNINST_KEY}" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKCU "${UNINST_KEY}" "EstimatedSize" "$0"
SectionEnd

Section "$(SecDesktop)" SEC_DESKTOP
  CreateShortcut "$DESKTOP\${APP}.lnk" "$INSTDIR\nrzCode.exe"
SectionEnd

Section "Uninstall"
  Call un.WaitUntilClosed
  Delete "$INSTDIR\nrzCode.exe"
  Delete "$INSTDIR\nrzCode.old.exe"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${APP}.lnk"
  Delete "$DESKTOP\${APP}.lnk"
  DeleteRegKey HKCU "${UNINST_KEY}"
  DeleteRegKey HKCU "Software\${APP}"
  IfSilent +2
  MessageBox MB_OK|MB_ICONINFORMATION "$(KeepData)"
SectionEnd
