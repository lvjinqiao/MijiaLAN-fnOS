Unicode True
!include "MUI2.nsh"

Name "MijiaLAN Rust"
OutFile "${OUTPUT_FILE}"
InstallDir "$LOCALAPPDATA\Programs\MijiaLAN Rust"
RequestExecutionLevel user
SetCompressor /SOLID lzma
VIProductVersion "${APP_VERSION}.0"
VIAddVersionKey "ProductName" "MijiaLAN Rust"
VIAddVersionKey "FileDescription" "MijiaLAN Rust per-user installer"
VIAddVersionKey "FileVersion" "${APP_VERSION}"
VIAddVersionKey "LegalCopyright" "Copyright iPocket-APP"

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\mijialan-rust.exe"
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "SimpChinese"

Section "MijiaLAN Rust"
    SetShellVarContext current
    SetOutPath "$INSTDIR"
    File /r "${SOURCE_DIR}\*"
    WriteUninstaller "$INSTDIR\Uninstall.exe"
    CreateDirectory "$SMPROGRAMS\MijiaLAN Rust"
    CreateShortcut "$SMPROGRAMS\MijiaLAN Rust\MijiaLAN Rust.lnk" "$INSTDIR\mijialan-rust.exe"
    CreateShortcut "$SMPROGRAMS\MijiaLAN Rust\MijiaLAN Rust Demo.lnk" "$INSTDIR\mijialan-rust.exe" "--demo"
    CreateShortcut "$SMPROGRAMS\MijiaLAN Rust\Uninstall.lnk" "$INSTDIR\Uninstall.exe"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MijiaLANRust" "DisplayName" "MijiaLAN Rust"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MijiaLANRust" "DisplayVersion" "${APP_VERSION}"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MijiaLANRust" "Publisher" "iPocket"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MijiaLANRust" "InstallLocation" "$INSTDIR"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MijiaLANRust" "UninstallString" '"$INSTDIR\Uninstall.exe"'
    WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MijiaLANRust" "NoModify" 1
    WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MijiaLANRust" "NoRepair" 1
SectionEnd

Section "Uninstall"
    SetShellVarContext current
    Delete "$INSTDIR\mijialan-rust.exe"
    Delete "$INSTDIR\mijia-rust.exe"
    Delete "$INSTDIR\libgcc_s_seh-1.dll"
    Delete "$INSTDIR\libstdc++-6.dll"
    Delete "$INSTDIR\libwinpthread-1.dll"
    Delete "$INSTDIR\LICENSE"
    Delete "$INSTDIR\README.md"
    Delete "$INSTDIR\PRIVACY.md"
    Delete "$INSTDIR\THIRD_PARTY_NOTICES.md"
    RMDir /r "$INSTDIR\licenses"
    RMDir /r "$INSTDIR\docs"
    Delete "$INSTDIR\Uninstall.exe"
    RMDir "$INSTDIR"
    Delete "$SMPROGRAMS\MijiaLAN Rust\MijiaLAN Rust.lnk"
    Delete "$SMPROGRAMS\MijiaLAN Rust\MijiaLAN Rust Demo.lnk"
    Delete "$SMPROGRAMS\MijiaLAN Rust\Uninstall.lnk"
    RMDir "$SMPROGRAMS\MijiaLAN Rust"
    DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\MijiaLANRust"
SectionEnd
