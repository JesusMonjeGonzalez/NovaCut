Unicode True

!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "Sections.nsh"

!define APP_NAME "NovaCut"
; build-windows.ps1 pasa la version de Cargo.toml con /DAPP_VERSION=x.y.z
!ifndef APP_VERSION
    !define APP_VERSION "0.0.0"
!endif
!define APP_EXE "novacut-windows.exe"
!define APP_PUBLISHER "Jesus Monje Gonzalez"
!define APP_URL "https://github.com/JesusMonjeGonzalez/NovaCut"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\NovaCut"

Name "${APP_NAME}"
OutFile "..\build\installer\NovaCut-Windows-Setup.exe"
InstallDir "$LOCALAPPDATA\Programs\NovaCut"
InstallDirRegKey HKCU "Software\NovaCut" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
VIProductVersion "${APP_VERSION}.0"
VIAddVersionKey "ProductName" "NovaCut"
VIAddVersionKey "FileDescription" "NovaCut Windows Installer"
VIAddVersionKey "FileVersion" "${APP_VERSION}"
VIAddVersionKey "ProductVersion" "${APP_VERSION}"
VIAddVersionKey "LegalCopyright" "Copyright (c) 2026 ${APP_PUBLISHER}"
VIAddVersionKey "CompanyName" "${APP_PUBLISHER}"

!define MUI_ICON "..\assets\icon.ico"
!define MUI_UNICON "..\assets\icon.ico"
BrandingText "NovaCut ${APP_VERSION}"

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
    File "..\LICENSE"
    File "..\THIRD_PARTY_NOTICES.md"
    File "..\docs\licenses\THIRD_PARTY_LICENSES-Windows.html"
    File "ffmpeg-install.ps1"
    WriteUninstaller "$INSTDIR\Desinstalar-NovaCut.exe"
    WriteRegStr HKCU "Software\NovaCut" "InstallDir" "$INSTDIR"
    ; Ficha completa en Configuracion > Aplicaciones instaladas.
    WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "NovaCut"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${APP_VERSION}"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "${APP_PUBLISHER}"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "URLInfoAbout" "${APP_URL}"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "HelpLink" "${APP_URL}/issues"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\${APP_EXE}"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\Desinstalar-NovaCut.exe"'
    WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\Desinstalar-NovaCut.exe" /S'
    WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
    WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
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
        IfSilent ffmpeg_silent_failure
        MessageBox MB_ICONEXCLAMATION|MB_OK "No se pudo descargar FFmpeg (revisa la conexion). NovaCut se ha instalado igualmente: al abrirlo te ofrecera instalar FFmpeg con un boton."
        Goto ffmpeg_failure_done
        ffmpeg_silent_failure:
        DetailPrint "No se pudo instalar FFmpeg."
        SetErrorLevel 1
        ffmpeg_failure_done:
    ${EndIf}
SectionEnd

; Tamaño real (con FFmpeg, si se instalo) para la ficha de Aplicaciones.
Section "-Tamano"
    ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
    IntFmt $0 "0x%08X" $0
    WriteRegDWORD HKCU "${UNINSTALL_KEY}" "EstimatedSize" "$0"
SectionEnd

; NovaCut abierto bloquearia la sustitucion del ejecutable.
!macro EXIGIR_NOVACUT_CERRADO un
Function ${un}ExigirNovaCutCerrado
    reintentar:
    nsExec::ExecToStack 'cmd /c tasklist /FI "IMAGENAME eq ${APP_EXE}" /NH | find /I "${APP_EXE}"'
    Pop $0
    Pop $1
    ${If} $0 == 0
        IfSilent app_abierta_silent
        MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "NovaCut esta abierto. Cierralo (guardando tu trabajo) y pulsa Reintentar." IDRETRY reintentar
        app_abierta_silent:
        SetErrorLevel 1
        Abort
    ${EndIf}
FunctionEnd
!macroend
!insertmacro EXIGIR_NOVACUT_CERRADO ""
!insertmacro EXIGIR_NOVACUT_CERRADO "un."

Function .onInit
    Call ExigirNovaCutCerrado
    ${GetParameters} $0
    ClearErrors
    ${GetOptions} $0 "/NOFFMPEG" $1
    ${IfNot} ${Errors}
        !insertmacro UnselectSection ${SEC_FFMPEG}
    ${EndIf}
FunctionEnd

Function un.onInit
    Call un.ExigirNovaCutCerrado
FunctionEnd

Section "Uninstall"
    Delete "$DESKTOP\NovaCut.lnk"
    RMDir /r "$SMPROGRAMS\NovaCut"
    DeleteRegKey HKCU "Software\Classes\.ncrough"
    DeleteRegKey HKCU "Software\Classes\NovaCut.RoughProject"
    DeleteRegKey HKCU "${UNINSTALL_KEY}"
    DeleteRegKey HKCU "Software\NovaCut"
    Delete "$INSTDIR\${APP_EXE}"
    Delete "$INSTDIR\LEEME-WINDOWS.md"
    Delete "$INSTDIR\LICENSE"
    Delete "$INSTDIR\THIRD_PARTY_NOTICES.md"
    Delete "$INSTDIR\THIRD_PARTY_LICENSES-Windows.html"
    Delete "$INSTDIR\ffmpeg-install.ps1"
    Delete "$INSTDIR\ffmpeg.exe"
    Delete "$INSTDIR\ffprobe.exe"
    Delete "$INSTDIR\ffplay.exe"
    Delete "$INSTDIR\FFmpeg-LICENSE.txt"
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
