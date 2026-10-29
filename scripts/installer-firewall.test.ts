import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const root = resolve(import.meta.dir, '..');
const config = JSON.parse(
  readFileSync(resolve(root, 'src-tauri/tauri.conf.json'), 'utf8'),
);
const hooks = readFileSync(
  resolve(root, 'src-tauri/windows/installer-hooks.nsh'),
  'utf8',
);
const template = readFileSync(resolve(root, 'src-tauri/installer.nsi'), 'utf8');
const policy = readFileSync(resolve(root, 'src-tauri/src/share_policy.rs'), 'utf8');
const firewall = readFileSync(resolve(root, 'src-tauri/src/firewall.rs'), 'utf8');
const app = readFileSync(resolve(root, 'src-tauri/src/lib.rs'), 'utf8');

const rustPort = policy.match(/QUICK_SHARE_PORT:\s*u16\s*=\s*([\d_]+)/)?.[1]
  ?.replaceAll('_', '');
const rustRuleName = policy.match(
  /QUICK_SHARE_FIREWALL_RULE_NAME:\s*&str\s*=\s*"([^"]+)"/,
)?.[1];
const rustHelperArgument = policy.match(
  /FIREWALL_HELPER_ARGUMENT:\s*&str\s*=\s*"([^"]+)"/,
)?.[1];

describe('Windows Quick Share firewall installer contract', () => {
  test('manual and passive upgrades default to replacing the existing installation', () => {
    const page = template.slice(template.indexOf('compare_version:'), template.indexOf('Function PageReinstallUpdateSelection'));
    const upgradeCopy = page.split('${ElseIf} $R0 = 1')[1]!.split('${ElseIf} $R0 = -1')[0]!;
    expect(upgradeCopy).toContain('StrCpy $R2 "覆盖升级（保留资料和设置）"');
    expect(upgradeCopy).toContain('StrCpy $R3 "先卸载旧版再安装"');
    expect(page).toContain('SendMessage $R2 ${BM_SETCHECK} ${BST_CHECKED} 0');
    const leave = template.slice(template.indexOf('Function PageLeaveReinstall'), template.indexOf('; 5. Choose install directory page'));
    const upgrade = leave.split('${ElseIf} $R0 = 1')[1]!.split('${ElseIf} $R0 = -1')[0]!;
    expect(upgrade).toMatch(/\$R1 = 1[^\n]*\r?\n\s*Goto reinst_done/);
    expect(upgrade).toMatch(/\$\{Else\}\s*Goto reinst_uninstall/);
    expect(leave).toMatch(/\$PassiveMode = 1\s*StrCpy \$R1 1/);
    expect(leave).toMatch(/\$UpdateMode = 1\s*Goto reinst_done/);
    expect(template).toMatch(/Function RestorePreviousInstallLocation[\s\S]*?StrCpy \$INSTDIR \$4/);
  });

  test('runs supported hooks from an elevated fresh install and passive update', () => {
    const nsis = config.bundle.windows.nsis;
    expect(nsis.installMode).toBe('perMachine');
    expect(nsis.installerHooks).toBe('windows/installer-hooks.nsh');
    expect(resolve(root, 'src-tauri', nsis.installerHooks)).toBe(
      resolve(root, 'src-tauri/windows/installer-hooks.nsh'),
    );
    expect(config.plugins.updater.windows.installMode).toBe('passive');
    expect(config.bundle.createUpdaterArtifacts).toBe(true);

    expect(template).toContain('RequestExecutionLevel admin');
    expect(template).toContain('${GetOptions} $CMDLINE "/UPDATE" $UpdateMode');
    expect(template).toContain('!insertmacro NSIS_HOOK_POSTINSTALL');
    expect(template).toContain('!insertmacro NSIS_HOOK_POSTUNINSTALL');
  });

  test('delegates rule creation to the fixed installed helper before Tauri initialization', () => {
    expect(rustHelperArgument).toBe('--wui-firewall-helper');
    expect(hooks.match(/nsExec::Exec '"\$\{WUI_FIREWALL_PROGRAM\}" --wui-firewall-helper'/g)).toHaveLength(1);
    expect(hooks).not.toContain('advfirewall firewall add rule');
    expect(hooks).toContain('name="${WUI_FIREWALL_RULE_NAME}"');
    expect(hooks).toContain('!define WUI_FIREWALL_RULE_NAME "WonderfulUI Quick Share"');
    expect(hooks).toContain('!define WUI_FIREWALL_PROGRAM "$INSTDIR\\wonderful-ui.exe"');
    expect(firewall).toContain('install_exact_rule(&executable, kind)');
    expect(hooks.match(/nsExec::Exec '"\$\{WUI_FIREWALL_PROGRAM\}" --wui-mobile-firewall-helper'/g)).toHaveLength(1);
    expect(app.indexOf('firewall::run_helper_if_requested()')).toBeLessThan(
      app.indexOf('tauri::Builder::default()'),
    );

    expect(rustPort).toBe('22357');
    expect(rustRuleName).toBe('WonderfulUI Quick Share');
    expect(hooks).toContain(`!define WUI_FIREWALL_PORT "${rustPort}"`);

    const start = hooks.indexOf('!macro NSIS_HOOK_POSTINSTALL');
    const installHook = hooks.slice(start, hooks.indexOf('!macroend', start));
    expect(installHook).toContain('!insertmacro WUI_INSTALL_FIREWALL_RULE');
  });

  test('uses helper rule replacement and preserves the rule only during updates', () => {
    const start = hooks.indexOf('!macro WUI_INSTALL_FIREWALL_RULE');
    const installRule = hooks.slice(start, hooks.indexOf('!macroend', start));
    const helperCall = installRule
      .split(/\r?\n/)
      .find(line => line.includes('nsExec::Exec'))?.trim() ?? '';
    expect(helperCall).toBe(`nsExec::Exec '"\${WUI_FIREWALL_PROGRAM}" ${rustHelperArgument}'`);
    expect(installRule).not.toContain('!insertmacro WUI_REMOVE_FIREWALL_RULE');
    expect(firewall.indexOf('rules.Remove(&name)')).toBeLessThan(
      firewall.indexOf('rules.Add(&rule)'),
    );
    expect(installRule).toContain('${If} $0 == 0');

    const uninstallHook = hooks.slice(hooks.indexOf('!macro NSIS_HOOK_POSTUNINSTALL'));
    expect(uninstallHook).toContain('${If} $UpdateMode <> 1');
    expect(uninstallHook).toContain('!insertmacro WUI_REMOVE_FIREWALL_RULE');
    expect(template).toContain('!insertmacro NSIS_HOOK_POSTINSTALL');
    expect(template).toContain('!insertmacro NSIS_HOOK_POSTUNINSTALL');
  });
});
