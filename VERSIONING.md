# 版本管理

WonderfulUI 使用语义化版本 2.0.0（`主版本.次版本.修订号`）。发布产物只面向 Windows
x64，当前格式是 NSIS 安装程序，不是 MSI 或便携压缩包。

## 唯一事实源与同步文件

`src-tauri/tauri.conf.json` 的 `version` 是版本事实源。当前同步检查还会核对：

- `src-tauri/Cargo.toml`；
- 根目录 `package.json`；
- `packages/parser/package.json` 和 `packages/gui/package.json`；
- `packages/parser/cli.ts`；
- `packages/gui/src/utils/version.ts`；
- `versions.json`；
- `packages/parser/tests/cli.test.ts` 中的 CLI 版本断言。

使用 `bun run scripts/check-versions.ts` 检查版本、Release tag 和签名更新配置。

## 版本脚本

```bash
bun run version:patch
bun run version:minor
bun run version:major
```

`scripts/version-bump.ts` 会从 Tauri 配置计算新版本，更新上述文件，写入
`versions.json`，创建 `chore(release): vX.Y.Z` 提交和 tag，并执行：

```text
git push origin main
git push origin vX.Y.Z
```

因此它只能由明确进行正式发布的维护者在干净工作区运行。普通 Agent、文档整理和
日常贡献不得运行 `bun run version:*`。

## GitHub Actions 发布流程

`.github/workflows/release.yml` 在 `v*` tag 或手动触发时运行：

1. `validate` 在 Windows runner 上安装锁定的 Bun/Rust，执行版本检查、资源检查、类型检查、
   Bun/Vitest 测试和 Rust lib 测试；
2. `build` 生成签名的 Tauri Windows x64 NSIS 产物；
3. tag 发布时，`publish` 创建 GitHub Release，并附上 Windows x64 NSIS 安装器、.exe.sig 和 latest.json。

发布文件为 `WonderfulUI_<version>_x64-setup.exe`、配套 `.exe.sig` 和 `latest.json`。
GitHub Actions 使用本修改版自己的签名 secret。私密仓库中的文件仅向获授权用户开放，
公开更新需仓库可匿名访问并已发布版本。更新线路与覆盖安装约定见 [更新与安装](docs/UPDATER.md)。

## 版本说明

`versions.json` 保存历史 release notes，供发布 workflow 生成应用内更新说明。预览版本的 release entry 设置 `prerelease: true`，workflow 发布为 `<version> Pre`，
使用对应版本的 `v<version>` 或 `v<version>-pre.N` 标签和签名产物；编号预览标签
仅在该版本声明为预览版时接受。GitHub 稳定 latest 入口不包含预览版。新增版本
时可以在运行 bump 脚本前设置 `WUI_RELEASE_NOTES`；不要为了修改文档而改动已有版本历史。

## 平台边界

macOS 上的类型检查、单元测试、浏览器 mock 和 Vite 构建不等于 Windows WebView2、NSIS
安装、更新安装、Firewall/UAC 或真实 ACLOS 扫描已通过。发布前必须由 Windows workflow
或 Windows 本机完成对应验证。
