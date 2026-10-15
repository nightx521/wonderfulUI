# WonderfulUI 架构说明

本文档描述当前代码的运行形态和边界。产品定位、用户文案和 ACLOS 字段分别见
`PRODUCT.md`、`DESIGN.md` 和 [ACLOS 数据格式](ACLOS_FORMAT.md)。

## 当前形态

- 目标平台：Windows 10/11 x64；桌面壳是 Tauri 2，运行时 WebView2。
- 前端：Vue 3 `<script setup lang="ts">`、Pinia、Vue Router 和 Vite。
- 后端：Rust 进程内解析器、Tauri command 和本地 SQLite（`rusqlite` bundled）。
- 工具链：Bun 1.3.14，Rust 1.88.0；版本分别由 `.bun-version` 和
  `rust-toolchain.toml` 固定。
- 运行索引：`%LOCALAPPDATA%\wonderful-ui\library.db`；日志：
  `%LOCALAPPDATA%\wonderful-ui\logs\wonderful-ui.log`。
- 构建：`bunx tauri build`/`bun run build`，当前 Windows bundle 目标只有 NSIS。
- Parser：GUI 使用 Tauri 内的 Rust parser；TypeScript parser 保留给 CLI 和 Bun 测试。

## 浏览器调试边界

`bun run dev:browser` 只启动 `packages/gui` 的 Vite 开发服务器。没有 Tauri runtime 时，
`packages/gui/src/tauri-adapter.ts` 提供固定的 mock 账户、对局、统计、日志和安全假路径，
用于检查 DOM、样式、动画和组件状态。

浏览器 debug 不读取 ACLOS、不播放真实视频、不检查 Windows 防火墙，也不执行真实安装升级
或快传；它不能被描述成 Windows Tauri smoke test。

## 数据流

```text
ACLOS WonderfulDb / snapshot<openid> (read-only)
        │
        ▼
Rust parser + library scraper
        │
        ▼
SQLite library.db (WonderfulUI-owned index and preferences)
        │
        ├── rounds-stripped library view → Vue/Pinia
        └── full matches.raw_json → get_match_rounds (on-demand)
```

后端从用户 profile 推导默认的 `%USERPROFILE%\AppData\Roaming\ACLOS\WonderfulDb`。GUI
本机扫描不提供自定义目录。历史导入通过 Rust 原生对话框选择拷贝的 WonderfulDb 和当前
视频目录，renderer 只持有临时选择令牌，不能传入任意源路径。预览不写入历史记录，确认
后以一个 SQLite 事务写入独立 history 表；本机扫描清理仅作用于原本的本机表。

扫描器会把完整 `matches.raw_json` 保留为原始回放数据，把通过共享事件状态机规范化的
事件写入 `events` 表。SQLite 还保存账户顺序、WonderfulUI 内部账户显示名、源文件
size/mtime、解析错误和资源缓存元数据；这些内容不写回 ACLOS。

## Parser 和 library 层

- `src-tauri/src/parser/`：hex 解码、AES 解密、WonderfulDb/snapshot model 和 reader；
- `packages/parser/`：等价的 TypeScript parser、CLI 和测试；
- `src-tauri/src/library/scraper.rs`：本机 ACLOS source adapter；
- `src-tauri/src/library/history.rs`：历史导入、媒体重定位、合并及来源标记；
- `src-tauri/src/library/db.rs`：数据库路径、迁移、library view、raw JSON 和视频路径授权；
- `src-tauri/src/library/events.rs`：事件过滤、去重键和播放时间的 Rust mirror；
- `src-tauri/src/library/aclos_identity.rs`：从 ACLOS 身份缓存、snapshot 和日志中读取显示名，
  必须使用临时只读副本打开 LevelDB；
- `src-tauri/src/library/model.rs`：发送给 GUI 的 IPC shape。

批量加载会省略每个视频的 `rounds`，详情打开时再读取单个 match。解析失败的账户保留
错误状态；损坏的 raw match 不应把其余资料库伪装成空库。

schema 6 在 `accounts` 增加可选 `parse_error_code`，与原始 `parse_error` 一同保存。
schema 7 增加可选 `match_details_json`，保存 snapshot 的对局成就与结果事实。
旧账户该列为 NULL 时，下一次增量扫描会重新读取一次；历史导入同样保存可选
`matchDetails`，合并时保持账户与对局关联，并重映射导入对局 ID。
library view 合并本机/导入记录后移除 rounds；完整事件通过详情 IPC 按需加载。
Rust 在错误仍为类型化值时分类，library view 通过可选 `errorCode` 传给 GUI；旧行无分类
时显示通用提示，下一次成功扫描会清除两项错误。应用扫描的账户失败详情写入
`account-read` 日志范围，包含账户标识、分类和底层错误，不记录完整解密内容。
合法空高光数据保留账户身份和显示偏好，有昵称或应用内名称的空账户可见，匿名空壳
不显示；本机旧记录清理与历史表继续分离。

## 主要 Tauri commands

### 启动和扫描

- `check_project_update(channel)`：按 proxy/github 枚举选择固定项目更新地址，返回官方 updater 资源句柄；
- `aclos_status`：只读检查默认 WonderfulDb 目录和数字账户文件，用于首次使用页面；
- `scan_shell`：读取已有 SQLite 账户壳并启动后台源刷新；
- `scan_all`：启动时的增量刷新并返回 rounds-stripped view；
- `scrape_library(mode)`：用户触发的 `incremental` 或 `full` 扫描；
- `load_library`：只读现有 SQLite view，不重新读取 WonderfulDb；
- `get_match_rounds(openid, match_id)`：从 SQLite 读取单场完整 rounds。
- `select_import_directory(kind)`：原生对话框选择目录并返回临时令牌；
- `preview_history_import(database_token, video_token, device_name)`：只读解析并保留确认草稿；
- `commit_history_import(token)` / `cancel_history_import`：提交最新预览或丢弃导入会话。

schema 5 的 `history_devices/accounts/matches/videos` 保存导入来源及已重定位的完整记录。
账户主文件内容指纹识别相同档案，路径或设备名称变更不产生重复记录。导入目录最多
128 个账户、128 MB 源数据；媒体遍历最多 100,000 个条目、32 层，跳过符号链接和
Windows reparse points。文件名、大小和路径后缀必须得到唯一候选，否则保留空媒体路径。
同账户、同原始对局编号只显示一条：本机资料优先，合并不同视频、补充缺失 rounds 和
可用媒体路径，保留所有来源标记。导入独有对局使用账户与原始编号的稳定哈希标识。
批量 view 不携带 rounds；详情只读取对应账户、对局的完整记录。来源筛选作用于合并后的
对局，详情显示该对局的全部已合并视频。

扫描模式和触发来源由后端校验，不信任 renderer 传入的审计文字。启动、手动刷新和全量
扫描串行化 SQLite 写入，避免后台任务覆盖较新的手动结果。

### 本地资料库和视频

- `save_account_order`：以事务保存账户顺序；合成的“全部账户”不写入数据库；
- `rename_account`：保存 WonderfulUI 内部的显示名覆盖；
- `get_library_stats`：读取账户、对局、视频和应用-owned storage 统计；
- `play_video`：Windows 上调用 `ShellExecuteW` 交给默认播放器；
- `reveal_in_explorer`：用 `explorer.exe /select` 定位文件；
- `capture_video_frame`：Windows Media Foundation 读取当前视频帧，返回 PNG base64；
- `get_log_status`/`reveal_logs_dir`：读取有限日志尾部或打开应用日志目录。

播放、资源管理器、截图和快传 command 都要求路径精确匹配 SQLite `videos.path` 或
`history_videos.path`，扩展名
受支持、当前是非符号链接普通文件。asset protocol 的可见性不能替代 command 授权。

### 外链、资源和网络

- `open_external_url`：只允许明确的 HTTPS GitHub/许可证地址；
- `cache_asset(s)`：保留的兼容资源缓存边界，只允许固定资源类型和允许的 HTTPS 来源；
  当前 canonical 地图、英雄和模式资源由仓库构建，正常扫描通常不产生 CDN 下载；
- `start_share_server`/`stop_share_server`/`share_server_status`：见下文快传边界；
- `inspect_share_port`/`release_share_port(ticket)`：诊断固定快传端口并凭短期原生票据结束符合条件的占用程序。

当前应用没有遥测、账号同步或后台上传 command。更新通过 Rust updater 读取版本与下载签名安装包，
不发送资料库内容。应用内项目链接由外链边界校验后打开。

## 快传架构

用户从播放器主动开启“快传”后，Rust 在 `0.0.0.0:22357/TCP` 启动一次性 HTTP 服务：

- 只接受已登记的本地视频；
- 随机生成 URL-safe token，路径为 `/w/<token>`；
- QR URL 使用启动时选定的 LAN IPv4；Host 必须匹配 localhost、loopback 或该 LAN IP；
- 传输完成后才计数并发出 `wui://share_downloaded`；
- 直接持有非阻塞 TCP listener，最多 4 个连接、8 KB 请求头；保留 token、Host 和 GET 校验；
- 关闭先查询并原子检查活动下载，未完成时用户可继续或确认中断；非阻塞发送支持取消；
- 会话启动/关闭串行，停止等待线程退出与 listener 释放后返回；前端新启动等待旧关闭确认；
- 前端模态挂载时启动、卸载时兜底清理；3 分钟无有效活动时关闭，下载进行中不判为空闲；
- session 使用 UUID v4，旧 session 的 stop/event 不能影响替代服务；
- 关闭服务不会立即删除持久防火墙规则，完整卸载才清理规则。

Windows 端口诊断只查询 22357/TCP 的 IPv4/IPv6 listener。结束操作使用单次、120 秒有效的
后端票据，再次核对端口归属、进程创建时间、映像路径、同用户和非关键状态；不提升权限。
系统目录、游戏/客户端/反作弊及本应用进程仅显示恢复建议。使用绑定身份的原生进程句柄，
防止端口变化或 PID 重用导致结束错误程序。

Windows NSIS 安装、覆盖安装和安装器 `/UPDATE` 会幂等配置名为
`WonderfulUI Quick Share` 的窄范围入站规则：已安装 exe、TCP 22357、LocalSubnet。规则
不绕过企业组策略、第三方防火墙或路由器 AP 隔离；缺失或不正确时，首次使用可能请求一次
受限 UAC 修复。

## 安装与升级

版本交付为 Windows x64 NSIS 安装器，沿用原目录覆盖升级并保留资料和设置。
打包时附带 LICENSE 与 NOTICE.md。默认代理更新，可选 GitHub 直连；
检查与下载共用选定线路，官方 updater 负责签名校验和安装。具体流程见 [更新与安装](UPDATER.md)。

## Tauri 安全边界

- 生产 CSP 明确限制脚本、样式、字体、图片、媒体和 IPC；devCsp 为空只是开发调试需要；
- asset protocol 不使用全局 `"**"` scope，应用缓存和经 SQLite 授权的媒体分别受限；
- screenshot Save As 只用对话框提供的单路径写权限，不授予 renderer 全盘读取；
- LevelDB 身份读取在临时副本上进行，避免在 ACLOS 活跃目录中产生 LOCK/日志副作用；
- 前端日志不写入原始 WonderfulDb、raw match JSON、完整事件树或视频路径库存；
- Media Foundation frame capture 对 buffer 长度、stride、乘法和时间戳转换执行边界检查。

## 前端结构

```text
packages/gui/src/
├── App.vue                 # boot、更新检查、模态层
├── components/
│   ├── common/             # onboarding、账户栏、boot、toast、图标
│   ├── event/              # 事件列表
│   ├── layout/             # top bar
│   ├── match/              # 卡片、筛选、日期
│   ├── player/             # 播放器、控制条、进度条
│   ├── settings/           # 资料库概览、日志、关于
│   ├── share/              # 快传模态
│   └── update/             # 更新模态
├── stores/                 # account、detail、filter、player、settings、share、update、ui
├── utils/                  # 纯逻辑、事件状态机、资源解析、平台适配
└── views/                  # Home、Detail、Settings
```

稳定 DOM、虚拟滚动、模态层级、播放器状态和图标约定见
[前端约定](FRONTEND_CONVENTIONS.md)。

## 开发和验证命令

依赖和本地前端检查：

```bash
bun install --frozen-lockfile
bun run typecheck
bun run test:all
bun run assets:check
bun run --cwd packages/gui build
```

浏览器 mock：

```bash
bun run dev:browser
```

Rust lib 测试：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

Windows Tauri 开发和完整 NSIS 构建：

```bash
bun run dev
bun run build
```

`.github/workflows/ci.yml` 是手动的 Windows `Manual Check`；`release.yml` 在 `v*` tag
上运行发布验证、NSIS 构建和安装包发布。macOS 的浏览器、类型、单测或
Vite 证据不能替代这些 Windows 行为验证。
