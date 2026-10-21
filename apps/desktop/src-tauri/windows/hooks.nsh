; «Открыть с помощью Сиртона» (T6.8, FR-WIN-008, D-117) — Microsoft's «Open With» registration,
; not a default-program change: the ProgID goes into each extension's OpenWithProgids and
; Applications\sirtone-app.exe lists the SupportedTypes, so Сиртон appears in «Открыть с помощью»
; while the user's default player stays as it was. Per user (HKCU), like the installer. The
; installed app plays a file it is given without importing it (FR-SHELL-004). The portable copy
; never registers anything (D-115). Removed on uninstall (not on an update).

!define SIRTONE_PROGID "SirTone.Audio"
!define SIRTONE_EXE "sirtone-app.exe"

!macro SIRTONE_OPENWITH EXT
  WriteRegStr HKCU "Software\Classes\.${EXT}\OpenWithProgids" "${SIRTONE_PROGID}" ""
  WriteRegStr HKCU "Software\Classes\Applications\${SIRTONE_EXE}\SupportedTypes" ".${EXT}" ""
!macroend

!macro SIRTONE_OPENWITH_REMOVE EXT
  DeleteRegValue HKCU "Software\Classes\.${EXT}\OpenWithProgids" "${SIRTONE_PROGID}"
  DeleteRegKey /ifempty HKCU "Software\Classes\.${EXT}\OpenWithProgids"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  WriteRegStr HKCU "Software\Classes\${SIRTONE_PROGID}" "" "Аудиофайл (Сиртон)"
  WriteRegStr HKCU "Software\Classes\${SIRTONE_PROGID}\DefaultIcon" "" "$INSTDIR\${SIRTONE_EXE},0"
  WriteRegStr HKCU "Software\Classes\${SIRTONE_PROGID}\shell\open" "" "Слушать в Сиртоне"
  WriteRegStr HKCU "Software\Classes\${SIRTONE_PROGID}\shell\open\command" "" '"$INSTDIR\${SIRTONE_EXE}" "%1"'
  WriteRegStr HKCU "Software\Classes\Applications\${SIRTONE_EXE}" "FriendlyAppName" "Сиртон"
  WriteRegStr HKCU "Software\Classes\Applications\${SIRTONE_EXE}\shell\open\command" "" '"$INSTDIR\${SIRTONE_EXE}" "%1"'
  !insertmacro SIRTONE_OPENWITH "mp3"
  !insertmacro SIRTONE_OPENWITH "flac"
  !insertmacro SIRTONE_OPENWITH "m4a"
  !insertmacro SIRTONE_OPENWITH "ogg"
  !insertmacro SIRTONE_OPENWITH "oga"
  !insertmacro SIRTONE_OPENWITH "opus"
  !insertmacro SIRTONE_OPENWITH "wav"
  !insertmacro SIRTONE_OPENWITH "aif"
  !insertmacro SIRTONE_OPENWITH "aiff"
  !insertmacro SIRTONE_OPENWITH "wma"
  !insertmacro SIRTONE_OPENWITH "ape"
  !insertmacro SIRTONE_OPENWITH "wv"
  ; Explorer refreshes its «Open With» lists.
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    !insertmacro SIRTONE_OPENWITH_REMOVE "mp3"
    !insertmacro SIRTONE_OPENWITH_REMOVE "flac"
    !insertmacro SIRTONE_OPENWITH_REMOVE "m4a"
    !insertmacro SIRTONE_OPENWITH_REMOVE "ogg"
    !insertmacro SIRTONE_OPENWITH_REMOVE "oga"
    !insertmacro SIRTONE_OPENWITH_REMOVE "opus"
    !insertmacro SIRTONE_OPENWITH_REMOVE "wav"
    !insertmacro SIRTONE_OPENWITH_REMOVE "aif"
    !insertmacro SIRTONE_OPENWITH_REMOVE "aiff"
    !insertmacro SIRTONE_OPENWITH_REMOVE "wma"
    !insertmacro SIRTONE_OPENWITH_REMOVE "ape"
    !insertmacro SIRTONE_OPENWITH_REMOVE "wv"
    DeleteRegKey HKCU "Software\Classes\${SIRTONE_PROGID}"
    DeleteRegKey HKCU "Software\Classes\Applications\${SIRTONE_EXE}"
    System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
  ${EndIf}
!macroend
