; Blink installer (NSIS). Per user: no administrator rights, nothing outside the user's profile.
;
;   makensis /DVERSION=0.2.0 /DBINDIR=..\target\release /DOUT=..\scripts\out\Blink-0.2.0-setup.exe installer\blink.nsi
;
; Silent use:  Blink-setup.exe /S [/AUTOSTART] [/D=C:\where]      install (autostart off unless asked)
;              uninstall.exe /S [/PURGE]                           remove; /PURGE also deletes settings,
;                                                                   caches and saved feed links
;
; Test hooks (never set by users): BLINK_RUN_KEY redirects the autostart value to a scratch key.

Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "nsDialogs.nsh"
!include "FileFunc.nsh"
!insertmacro GetParameters
!insertmacro GetOptions
!insertmacro GetSize
!insertmacro un.GetParameters
!insertmacro un.GetOptions

!ifndef VERSION
  !define VERSION "0.2.0"
!endif
!ifndef BINDIR
  !define BINDIR "..\target\release"
!endif
!ifndef OUT
  !define OUT "Blink-${VERSION}-setup.exe"
!endif

!define APP "Blink"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP}"
!define LEGACY_UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Deadline Notch"
!define RUNKEY "Software\Microsoft\Windows\CurrentVersion\Run"
!define WEBVIEW2_GUID "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"

Name "${APP}"
OutFile "${OUT}"
InstallDir "$LOCALAPPDATA\Programs\${APP}"
InstallDirRegKey HKCU "${UNINSTKEY}" "InstallLocation"
RequestExecutionLevel user
SetCompressor /SOLID lzma
ShowInstDetails nevershow
BrandingText "${APP} ${VERSION}"

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${APP}"
VIAddVersionKey "FileDescription" "${APP} installer"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"

!define MUI_ICON "..\crates\blink-settings\icons\icon.ico"
!define MUI_UNICON "..\crates\blink-settings\icons\icon.ico"

Var AutostartBox
Var AutostartState

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
Page custom OptionsPage OptionsPageLeave
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\blink.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Start Blink now"
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
UninstPage custom un.OptionsPage un.OptionsPageLeave
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

; ----------------------------------------------------------------------------- helpers

!macro RunKeyName OUTVAR
  ReadEnvStr ${OUTVAR} "BLINK_RUN_KEY"
  ${If} ${OUTVAR} == ""
    StrCpy ${OUTVAR} "${RUNKEY}"
  ${EndIf}
!macroend

; Ask a running notch to leave; end it if it does not (an older build has no --quit).
!macro StopBlink DIR
  ${If} ${FileExists} "${DIR}\blink.exe"
    nsExec::Exec '"${DIR}\blink.exe" --quit'
    Pop $0
    Sleep 800
  ${EndIf}
  ; Only our own copies: a notch run from somewhere else is not ours to end.
  nsExec::Exec `powershell -NoProfile -Command "Get-Process blink,blink-settings -ErrorAction SilentlyContinue | Where-Object Path -like '${DIR}\*' | Stop-Process -Force"`
  Pop $0
  Sleep 300
!macroend

!macro WebView2Check
  ClearErrors
  ReadRegStr $0 HKLM "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\${WEBVIEW2_GUID}" "pv"
  ${If} $0 == ""
    ReadRegStr $0 HKCU "Software\Microsoft\EdgeUpdate\Clients\${WEBVIEW2_GUID}" "pv"
  ${EndIf}
  ${If} $0 == ""
  ${OrIf} $0 == "0.0.0.0"
    ${IfNot} ${Silent}
      MessageBox MB_YESNO|MB_ICONINFORMATION "Blink's settings window needs the Microsoft Edge WebView2 Runtime, which is not installed on this computer. Blink itself (the notch) does not need it.$\r$\n$\r$\nOpen the download page now?" IDNO +2
      ExecShell "open" "https://go.microsoft.com/fwlink/p/?LinkId=2124703"
    ${EndIf}
  ${EndIf}
!macroend

; ----------------------------------------------------------------------------- options page

Function OptionsPage
  ${If} ${Silent}
    Abort
  ${EndIf}
  !insertmacro MUI_HEADER_TEXT "Options" "Choose how Blink starts."
  nsDialogs::Create 1018
  Pop $0
  ${NSD_CreateCheckbox} 0 8u 100% 12u "Start Blink when I sign in to Windows"
  Pop $AutostartBox
  ; Keep what an existing install had; first installs start with it on.
  !insertmacro RunKeyName $1
  ReadRegStr $2 HKCU "$1" "${APP}"
  ReadRegStr $3 HKCU "${UNINSTKEY}" "InstallLocation"
  ${If} $2 != ""
  ${OrIf} $3 == ""
    ${NSD_Check} $AutostartBox
  ${EndIf}
  ${NSD_CreateLabel} 0 28u 100% 36u "Blink shows your nearest deadline at the top of the screen. Your calendar links are kept in Windows Credential Manager, never in a file.$\r$\n$\r$\nThis installer is not code-signed, so Windows SmartScreen may warn you when you run it."
  Pop $0
  nsDialogs::Show
FunctionEnd

Function OptionsPageLeave
  ${NSD_GetState} $AutostartBox $AutostartState
FunctionEnd

; ----------------------------------------------------------------------------- install

Section "Install"
  SetOutPath "$INSTDIR"
  !insertmacro StopBlink "$INSTDIR"

  ; An install of the WebView2 build ("Deadline Notch"): remove its program, keep its data.
  ReadRegStr $R0 HKCU "${LEGACY_UNINSTKEY}" "UninstallString"
  ${If} $R0 != ""
    nsExec::Exec '$R0 /S'
    Pop $0
    Sleep 1500
  ${EndIf}

  File "${BINDIR}\blink.exe"
  File "${BINDIR}\blink-settings.exe"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  CreateDirectory "$SMPROGRAMS\${APP}"
  CreateShortcut "$SMPROGRAMS\${APP}\${APP}.lnk" "$INSTDIR\blink.exe"
  CreateShortcut "$SMPROGRAMS\${APP}\${APP} settings.lnk" "$INSTDIR\blink-settings.exe"

  ; Autostart: asked for on the options page, or /AUTOSTART when silent.
  ${GetParameters} $R1
  ${GetOptions} $R1 "/AUTOSTART" $R2
  !insertmacro RunKeyName $R3
  ${If} ${Silent}
    ${IfNot} ${Errors}
      WriteRegStr HKCU "$R3" "${APP}" '"$INSTDIR\blink.exe"'
    ${EndIf}
  ${Else}
    ${If} $AutostartState == ${BST_CHECKED}
      WriteRegStr HKCU "$R3" "${APP}" '"$INSTDIR\blink.exe"'
    ${Else}
      DeleteRegValue HKCU "$R3" "${APP}"
    ${EndIf}
  ${EndIf}
  DeleteRegValue HKCU "$R3" "Deadline Notch"

  WriteRegStr HKCU "${UNINSTKEY}" "DisplayName" "${APP}"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTKEY}" "Publisher" "${APP}"
  WriteRegStr HKCU "${UNINSTKEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayIcon" "$INSTDIR\blink.exe"
  WriteRegStr HKCU "${UNINSTKEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINSTKEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  WriteRegDWORD HKCU "${UNINSTKEY}" "EstimatedSize" $0

  !insertmacro WebView2Check
SectionEnd

; --------------------------------------------------------------------------- uninstall

Var PurgeBox
Var PurgeState

Function un.OptionsPage
  ${If} ${Silent}
    Abort
  ${EndIf}
  !insertmacro MUI_HEADER_TEXT "Your data" "Blink keeps your settings and feed links after it is removed."
  nsDialogs::Create 1018
  Pop $0
  ${NSD_CreateCheckbox} 0 8u 100% 24u "Also delete my settings, cached calendars and saved calendar links"
  Pop $PurgeBox
  ${NSD_CreateLabel} 0 40u 100% 24u "Leave this unticked if you may install Blink again: it will pick up where you left off."
  Pop $0
  nsDialogs::Show
FunctionEnd

Function un.OptionsPageLeave
  ${NSD_GetState} $PurgeBox $PurgeState
FunctionEnd

Section "Uninstall"
  ; Purging needs the program, so do it before the files go.
  ${un.GetParameters} $R1
  ${un.GetOptions} $R1 "/PURGE" $R2
  ${If} ${Silent}
    ${IfNot} ${Errors}
      StrCpy $PurgeState ${BST_CHECKED}
    ${EndIf}
  ${EndIf}

  ${If} ${FileExists} "$INSTDIR\blink.exe"
    nsExec::Exec '"$INSTDIR\blink.exe" --quit'
    Pop $0
    Sleep 800
  ${EndIf}
  nsExec::Exec `powershell -NoProfile -Command "Get-Process blink,blink-settings -ErrorAction SilentlyContinue | Where-Object Path -like '$INSTDIR\*' | Stop-Process -Force"`
  Pop $0
  Sleep 300

  ${If} $PurgeState == ${BST_CHECKED}
    nsExec::Exec '"$INSTDIR\blink.exe" --purge-data'
    Pop $0
  ${EndIf}

  Delete "$INSTDIR\blink.exe"
  Delete "$INSTDIR\blink-settings.exe"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${APP}\${APP}.lnk"
  Delete "$SMPROGRAMS\${APP}\${APP} settings.lnk"
  RMDir "$SMPROGRAMS\${APP}"

  ReadEnvStr $R3 "BLINK_RUN_KEY"
  ${If} $R3 == ""
    StrCpy $R3 "${RUNKEY}"
  ${EndIf}
  DeleteRegValue HKCU "$R3" "${APP}"
  DeleteRegKey HKCU "${UNINSTKEY}"
SectionEnd
