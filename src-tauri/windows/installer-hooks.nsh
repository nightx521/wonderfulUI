; WonderfulUI Windows installer hooks.
;
; Quick Share always listens on 22357/TCP. The rule is intentionally narrow:
; one installed executable, one TCP local port, all Windows profiles, and only
; peers in the local subnet. This is a best-effort installer action: GPO or a
; third-party firewall may reject it, but the application must still install.

!define WUI_FIREWALL_RULE_NAME "WonderfulUI Quick Share"
!define WUI_FIREWALL_PORT "22357"
!define WUI_FIREWALL_PROGRAM "$INSTDIR\wonderful-ui.exe"

!macro WUI_REMOVE_FIREWALL_RULE
  ; netsh emits OEM multibyte text. Disable nsExec's UTF-16 guess before
  ; decoding it, so Chinese status output is readable in the Unicode log.
  nsExec::ExecToLog /OEM /MBCS '"$SYSDIR\netsh.exe" advfirewall firewall delete rule name="${WUI_FIREWALL_RULE_NAME}"'
  Pop $0
  nsExec::ExecToLog /OEM /MBCS '"$SYSDIR\netsh.exe" advfirewall firewall delete rule name="WonderfulUI Mobile Access"'
  Pop $0
!macroend

!macro WUI_INSTALL_FIREWALL_RULE
  DetailPrint "正在配置 WonderfulUI 快传防火墙规则（TCP ${WUI_FIREWALL_PORT}）..."

  ; The installer is already elevated. Reuse the fixed Rust helper so install,
  ; reinstall, and /UPDATE create the same exact rule as runtime repair.
  ; Helper mode exits before Tauri initialization and removes stale rules itself.
  nsExec::Exec '"${WUI_FIREWALL_PROGRAM}" --wui-firewall-helper'
  Pop $0
  ${If} $0 == 0
    DetailPrint "快传防火墙规则配置成功。"
  ${Else}
    DetailPrint "快传防火墙规则配置失败（返回代码：$0），安装将继续。"
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; The template invokes this after files are copied for normal installs and
  ; Tauri's passive /UPDATE path, before the updated app is relaunched.
  !insertmacro WUI_INSTALL_FIREWALL_RULE
  DetailPrint "正在配置手机访问防火墙规则..."
  nsExec::Exec '"${WUI_FIREWALL_PROGRAM}" --wui-mobile-firewall-helper'
  Pop $0
  ${If} $0 != 0
    DetailPrint "手机访问规则暂未配置（返回代码：$0），可在应用中重试授权。"
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Keep the rule while an updater is replacing files. A complete uninstall
  ; removes it so Windows Firewall is not left with an orphaned exception.
  ${If} $UpdateMode <> 1
    DetailPrint "正在移除 WonderfulUI 快传防火墙规则..."
    !insertmacro WUI_REMOVE_FIREWALL_RULE
  ${EndIf}
!macroend
