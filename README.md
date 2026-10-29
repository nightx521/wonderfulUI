<div align="center">
  <img src="packages/gui/src/assets/logo.svg" width="112" height="112" alt="WonderfulUI">
  <h1>WonderfulUI</h1>
  <p>Windows 无畏契约个人高光资料库</p>
  <p>找到对局，回看高光，分享精彩时刻。</p>
  <a href="https://github.com/nightx521/wonderfulUI/releases">下载安装包</a> ·
  <a href="docs/TROUBLESHOOTING.md">使用帮助</a> ·
  <a href="CONTRIBUTING.md">参与开发</a>
</div>

## 功能

- **整理对局**：管理多个账户，按英雄、地图、模式、胜负、日期、MVP/SVP 和个人表现筛选。
- **查看战绩**：显示来源记录中的段位、KDA、四杀和五杀时刻。
- **回看视频**：内置播放器、击杀与死亡事件跳转、逐帧查看和画面截图。
- **迁移历史**：导入旧电脑的对局数据与视频，按来源设备筛选，重复对局合并展示。
- **局域网快传**：手机扫码下载当前视频；关闭时释放端口，下载未完成时先确认。
- **手机访问**：电脑开启开关，手机扫码并经确认后，可在浏览器中筛选对局、查看详情和播放回放。
- **覆盖更新**：保留资料和设置，可选择代理加速或 GitHub 直连。

## 下载与使用

在 [Releases](https://github.com/nightx521/wonderfulUI/releases) 下载
`WonderfulUI_<version>_x64-setup.exe`，支持 Windows 10/11 x64。
更新时直接运行新版安装包，沿用原目录覆盖安装即可。

### 本机高光

先使用腾讯“无畏时刻”生成高光，再打开 WonderfulUI。应用会自动读取本机数据，
你可以在左侧选择账户，筛选对局并播放视频。回看时无需启动游戏。

本机数据位置：

```text
%USERPROFILE%\AppData\Roaming\ACLOS\WonderfulDb
```

数据文件保持只读。WonderfulUI 自己的索引、设置和日志保存在
`%LOCALAPPDATA%\wonderful-ui\`。

### 从旧电脑迁移

1. 从旧电脑复制 `WonderfulDb` 文件夹和对应的视频文件。
2. 打开“设置 → 资料库 → 导入旧电脑数据”。
3. 分别选择复制的数据文件夹和视频在当前电脑的位置。
4. 检查预览后确认导入。

导入记录会保留在资料库中。对局信息来自数据文件，仅有视频时无法还原完整战绩。

### 快传到手机

电脑和手机连接同一局域网，在播放器中点击“快传”，用手机扫描二维码下载。
关闭二维码会停止服务；仍有设备正在下载时，可以继续等待或确认中断。
遇到端口冲突时，按提示处理占用程序后重试。

找不到高光、视频无法播放或快传无法连接时，查看[故障排查](docs/TROUBLESHOOTING.md)。

### 用手机浏览回放

1. 保持电脑开机并运行 WonderfulUI，手机与电脑连接可以互通的同一局域网。
2. 在电脑“设置 → 手机访问”开启开关，用手机浏览器扫描二维码或打开复制的完整链接。
3. 手机上请求连接后，在电脑中点击“允许连接”。
4. 手机可独立搜索、筛选、查看详情并播放原视频；电脑扫描更新资料后，在手机点击“刷新”。

访问默认关闭，每次重新开启需要重新配对。关闭设置窗口会继续提供访问；关闭开关或
退出应用会断开连接。仅在信任的局域网中使用，网页和媒体通过本机 HTTP 传输。
部分录制编码可能不受手机浏览器支持；建议使用较新的安卓 Chrome，原文件不会自动转码。

## 开发

技术栈：Tauri 2、Vue 3、Pinia、Rust 和 SQLite。
Windows 桌面开发使用 Bun **1.3.14**、Rust **1.88.0**，并安装
Tauri 所需的 Windows SDK 与 WebView2 开发环境。

```bash
git clone https://github.com/nightx521/wonderfulUI.git
cd wonderfulUI
bun install --frozen-lockfile
bun run dev
```

使用固定示例数据预览前端：

```bash
bun run dev:browser
```

检查与构建：

```bash
bun run typecheck
bun run test:all
bun run assets:check
bun run build:mobile
cargo test --manifest-path src-tauri/Cargo.toml --lib
bun run build
```

安装包输出到 `target/release/bundle/nsis/`。签名构建需要配置
`TAURI_SIGNING_PRIVATE_KEY`；具体流程见[更新与安装](docs/UPDATER.md)。
更多信息见[贡献指南](CONTRIBUTING.md)、[架构说明](docs/ARCHITECTURE.md)
和[文档索引](docs/README.md)。

## 关于

原作者：© 2026 [WizisCool](https://github.com/WizisCool/WonderfulUI)。
二次开发：© 2026 murainy。

项目采用 [GPL-3.0-or-later](LICENSE)，素材来源和版权说明见 [NOTICE.md](NOTICE.md)。
游戏名称、标识与相关素材归各自权利人所有；本项目与游戏官方无隶属关系。
