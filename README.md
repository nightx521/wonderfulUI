<div align="center">
  <img src="packages/gui/src/assets/logo.svg" width="128" height="128" alt="WonderfulUI Logo">
  <h1>WonderfulUI</h1>
  <p>Windows 无畏契约个人高光资料库</p>
  <p>把散落在本机的高光整理起来，随时查找和回看。</p>
  <p>
    <a href="LICENSE">
      <img src="https://img.shields.io/badge/License-GPL--3.0--or--later-blue?style=flat" alt="License: GPL-3.0-or-later">
    </a>
    <a href="https://github.com/nightx521/wonderfulUI/releases">
      <img src="https://img.shields.io/badge/Platform-Windows%20x64-0078D4?style=flat" alt="Platform: Windows x64">
    </a>
  </p>
</div>

---

WonderfulUI 把已经生成在电脑里的无畏契约高光整理成一个本地资料库。你可以按账户和
对局查找视频，直接播放、查看事件时间轴、截图，或把当前视频快传到同一局域网的设备。
回看时不需要启动游戏或录制客户端。

## 能做什么

- 整理多个本地账户，调整顺序并设置应用内显示名称。
- 按搜索词、英雄、地图、模式、房间类型、胜负/平局、视频类型、MVP/SVP/其他、日期、击杀和 KDA 筛选对局。房间类型可选全部、自定义或非自定义；其他成就包含普通和缺少记录的对局。
- 竞技对局显示记录中的段位与本地图标，区分未定级和未知；四杀、五杀时刻用不同颜色标签显示，可同时出现。同类多个视频只显示一个标签。
- 在应用内播放视频，按击杀和死亡事件跳转，并支持预卷播放。
- 截取当前画面，或生成临时二维码把视频传到同一局域网的设备。
- 查看账户、对局和视频概览，按需增量刷新或全量重建本地索引。
- 导入旧电脑的 WonderfulDb 和已复制的视频，预览后写入资料库；按本机或导入设备筛选。

WonderfulUI 只整理和读取已有高光，不会修改 ACLOS 内容。目前也不提供高光删除或编辑、
资料库导出或备份、切换本机扫描目录、便携版和云同步。

## 下载与首次使用

从 [Releases](https://github.com/nightx521/wonderfulUI/releases) 下载最新的 Windows x64
NSIS 安装程序：

```text
WonderfulUI_<version>_x64-setup.exe
```

目前没有 MSI 或解压即用版本。

高光来自腾讯 ACLOS/“无畏时刻”。请先让它在本机生成过高光，WonderfulUI 会读取默认
目录：

```text
%USERPROFILE%\AppData\Roaming\ACLOS\WonderfulDb
```

`WonderfulDb` 和可选的 `snapshot<openid>` 始终按只读数据处理。本机扫描使用上述默认目录。
迁移旧电脑数据时，在“设置 → 资料库 → 导入旧电脑数据”选择拷贝的 WonderfulDb 文件夹，
再选择视频在当前电脑的文件夹，检查预览后确认导入。首次使用页面也提供这个入口。
导入历史在后续扫描和重启后保留；重复对局合并显示。仅复制视频无法恢复对局信息。
没有发现高光、视频被移动或网络盘未挂载时，请查看
[故障排查](docs/TROUBLESHOOTING.md)。

## 网络和本地数据

- 高光索引、账户偏好和诊断日志保存在 `%LOCALAPPDATA%\wonderful-ui\`。
- 当前代码没有遥测、账号同步或后台上传。
- 版本升级使用项目提供的 NSIS 安装器，沿用原安装目录覆盖安装。
- 快传只在播放器中由用户主动开启。它会在本机临时监听 `22357/TCP`，用带令牌的
  局域网地址提供当前视频下载。
- 资料库浏览读取本地索引和随包资源；快传和用户打开的外部网站链接会使用网络。

## 从源码运行

桌面端面向 Windows 10/11 x64。开发环境还需要：

- [Bun](https://bun.sh) 1.3.14，版本由 `.bun-version` 固定；
- [Rust](https://rustup.rs) 1.88.0，版本由 `rust-toolchain.toml` 固定；
- Tauri 2 所需的 Windows SDK 和 WebView2 环境。

安装依赖并运行检查：

```bash
git clone https://github.com/nightx521/wonderfulUI.git
cd WonderfulUI

bun install --frozen-lockfile
bun run typecheck
bun run test:all
bun run assets:check
```

只查看使用固定 mock 数据的前端界面：

```bash
bun run dev:browser
```

在 Windows 上运行桌面开发壳或构建 NSIS 安装程序：

```bash
bun run dev
bun run build
```

本地构建产物位于 `target/release/bundle/nsis/`。浏览器 mock、macOS 类型检查和 Vite
构建不能替代 Windows WebView2、安装器或真实 ACLOS 数据验证。更多开发说明见
[贡献指南](CONTRIBUTING.md)。

## 文档

- [文档索引](docs/README.md)
- [故障排查](docs/TROUBLESHOOTING.md)
- [贡献指南](CONTRIBUTING.md)
- [安全策略](SECURITY.md)
- [贡献者公约](CODE_OF_CONDUCT.md)

## 许可与声明

WonderfulUI 使用 [GNU General Public License v3.0 or later](LICENSE)（`GPL-3.0-or-later`）发布。

原作者：© 2026 WizisCool；二次开发：© 2026 murainy。原项目和许可说明见 [NOTICE.md](NOTICE.md)。
当前开发仓库为私密仓库。向他人分发本修改版安装包时，须按 GPL 向接收者提供对应版本源码；
私密开发仓库本身不替代源码提供安排。

本项目与 Riot Games、腾讯、ACLOS、WeGame 或无畏契约官方没有隶属关系。游戏名称、
标识和相关内容归各自权利人所有。
