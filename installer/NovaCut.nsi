Unicode True

!include "MUI2.nsh"
!include "LogicLib.nsh"

!define APP_NAME "NovaCut"
!define APP_VERSION "0.1.0"
!define APP_EXE "novacut-windows.exe"

Name "${APP_NAME}"
OutFile "..\build\installer\NovaCut-Windows-Setup.exe"
InstallDir "$LOCALAPPDATA\Programs\NovaCut"
InstallDirRegKey HKCU "Software\NovaCut" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
VIProductVersion "0.1.0.0"
VIAddVersionKey "ProductName" "NovaCut"
VIAddVersionKey "FileDescription" "NovaCut Windows Installer"
VIAddVersionKey "FileVersion" "${APP_VERSION}"
VIAddVersionKey "ProductVersion" "${APP_VERSION}"
VIAddVersionKey "LegalCopyright" "Copyright NovaCut contributors"

!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\${APP_EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Abrir NovaCut"
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "..\LICENSE"
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "Spanish"

Section "NovaCut (obligatorio)" SEC_APP
    SectionIn RO
    SetOutPath "$INSTDIR"
    File "..\build\NovaCut-Windows\novacut-windows.exe"
    File /oname=LEEME-WINDOWS.md "..\docs\GUIA-WINDOWS.md"
    File "ffmpeg-install.ps1"
    WriteUninstaller "$INSTDIR\Desinstalar-NovaCut.exe"
    WriteRegStr HKCU "Software\NovaCut" "InstallDir" "$INSTDIR"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\NovaCut" "DisplayName" "NovaCut"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\NovaCut" "DisplayVersion" "${APP_VERSION}"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\NovaCut" "DisplayIcon" "$INSTDIR\${APP_EXE}"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\NovaCut" "UninstallString" '"$INSTDIR\Desinstalar-NovaCut.exe"'
    CreateDirectory "$SMPROGRAMS\NovaCut"
    CreateShortcut "$SMPROGRAMS\NovaCut\NovaCut.lnk" "$INSTDIR\${APP_EXE}"
    CreateShortcut "$SMPROGRAMS\NovaCut\Desinstalar NovaCut.lnk" "$INSTDIR\Desinstalar-NovaCut.exe"

    WriteRegStr HKCU "Software\Classes\.ncrough" "" "NovaCut.RoughProject"
    WriteRegStr HKCU "Software\Classes\NovaCut.RoughProject" "" "Proyecto NovaCut Windows"
    WriteRegStr HKCU "Software\Classes\NovaCut.RoughProject\DefaultIcon" "" "$INSTDIR\${APP_EXE},0"
    WriteRegStr HKCU "Software\Classes\NovaCut.RoughProject\shell\open\command" "" '"$INSTDIR\${APP_EXE}" "%1"'
    System::Call 'shell32.dll::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
SectionEnd

Section "Acceso directo en el escritorio" SEC_DESKTOP
    CreateShortcut "$DESKTOP\NovaCut.lnk" "$INSTDIR\${APP_EXE}"
SectionEnd

Section "Motor multimedia FFmpeg (recomendado)" SEC_FFMPEG
    DetailPrint "Descargando e instalando FFmpeg (~80 MB)..."
    nsExec::ExecToLog 'powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\ffmpeg-install.ps1" -InstallDir "$INSTDIR"'
    Pop $0
    ${If} $0 <> 0
        MessageBox MB_ICONEXCLAMATION|MB_OK "No se pudo instalar FFmpeg. Descargalo de https://www.gyan.dev/ffmpeg/builds/ y copia ffmpeg.exe junto a NovaCut."
    ${EndIf}
SectionEnd

Section "Uninstall"
    Delete "$DESKTOP\NovaCut.lnk"
    RMDir /r "$SMPROGRAMS\NovaCut"
    DeleteRegKey HKCU "Software\Classes\.ncrough"
    DeleteRegKey HKCU "Software\Classes\NovaCut.RoughProject"
    DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\NovaCut"
    DeleteRegKey HKCU "Software\NovaCut"
    Delete "$INSTDIR\${APP_EXE}"
    Delete "$INSTDIR\LEEME-WINDOWS.md"
    Delete "$INSTDIR\ffmpeg-install.ps1"
    Delete "$INSTDIR\ffmpeg.exe"
    Delete "$INSTDIR\ffprobe.exe"
    Delete "$INSTDIR\ffplay.exe"
    Delete "$INSTDIR\Desinstalar-NovaCut.exe"
    RMDir "$INSTDIR"
    System::Call 'shell32.dll::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
SectionEnd

LangString DESC_SEC_APP ${LANG_SPANISH} "Instala NovaCut, el menu Inicio, el desinstalador y la asociacion de proyectos."
LangString DESC_SEC_DESKTOP ${LANG_SPANISH} "Crea un acceso directo en el escritorio."
LangString DESC_SEC_FFMPEG ${LANG_SPANISH} "Instala el motor necesario para importar, previsualizar y exportar video."

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
    !insertmacro MUI_DESCRIPTION_TEXT ${SEC_APP} $(DESC_SEC_APP)
    !insertmacro MUI_DESCRIPTION_TEXT ${SEC_DESKTOP} $(DESC_SEC_DESKTOP)
    !insertmacro MUI_DESCRIPTION_TEXT ${SEC_FFMPEG} $(DESC_SEC_FFMPEG)
!insertmacro MUI_FUNCTION_DESCRIPTION_END
