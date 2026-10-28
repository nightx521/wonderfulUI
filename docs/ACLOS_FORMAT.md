# ACLOS 数据格式说明

本文档记录解析器和 GUI 需要长期维护的 ACLOS/WonderfulDb 事实。它是技术参考，不是
普通用户的产品介绍。

## 只读边界

- `WonderfulDb` 和 `snapshot<openid>` 是 ACLOS 的本地缓存输入；解析器只读，不写入、
  重命名、截断或删除它们。
- 不要读取、修改或删除 Riot、WeGame、Valorant、Vanguard 或游戏安装文件来“修复”解析。
- ACLOS 写入期间可能出现不完整文件；遇到解析失败应保留错误并允许用户稍后重试，不能
  用破坏源文件的方式恢复。

## 输入位置和账户枚举

GUI 后端的本机扫描从用户 profile 推导 ACLOS 默认目录：

```text
%USERPROFILE%\AppData\Roaming\ACLOS\WonderfulDb
```

`HOME` 是非 Windows 测试环境的回退。本机扫描目录固定；历史导入可以通过原生对话框
选择已复制的 WonderfulDb 文件夹和当前视频目录，预览后写入应用资料库。CLI 的
`scan`、`show` 和 `scan-all` 可以接收显式路径，便于开发和测试。

目录中的账户主文件是没有扩展名、basename 为非空 ASCII 十进制 openid 的普通文件。
目录、符号链接、隐藏说明文件和其它非数字文件不能成为账户。相邻的
`snapshot<openid>` 是可选输入，缺失、为空、损坏或无法解密都不能阻塞高光列表。

与具体用户机器有关的路径、openid、昵称、标签和视频位置不属于格式契约，不能写进
产品逻辑、文档示例或测试特判。

## 编码和解密

账户主文件完成解密后，合法空对象 `{}` 与合法的空高光数组均表示当前没有高光记录，
不属于读取失败。只有完整解密且 JSON 解析成功的空对象享有此规则；空文件、截断密文、
非对象 JSON，以及非空对象中缺少高光列表或字段类型不兼容仍需报告错误。
非空对象必须包含当前账户的高光列表键，不使用其他数组猜测列表。输入起始位置的
UTF-8 BOM 可忽略。允许缺失的统计、对象和列表字段接受显式 `null`，仍保留统计缺失与
胜负未知标记；非法对象/数组类型不能被当作空列表。
snapshot 的身份和对局摘要不能代替高光主记录，也不能据此判断录制当时是否开启。

已观察到的 ACLOS 数据（包括 2.15.3.449 时代的格式）具有以下形态；版本号是兼容性
观察，不是用户必须安装的 WonderfulUI 版本要求：

1. 文件正文是 `0-9`/`a-f` 的 ASCII 十六进制文本；
2. 十六进制解码后得到 AES-256-CBC 密文，使用 PKCS#7 padding；
3. key/IV 由 account openid 的 SHA-256 十六进制结果派生，具体实现见
   `src-tauri/src/parser/crypto.rs` 和 `packages/parser/src/crypto.ts`；
4. 主文件明文包含 `key_wonderful_list_<openid>`，值是对局数组；
5. snapshot 明文包含 `key_snapshot_list<openid>`，字段子集见 parser model。

Rust parser 是 GUI 的运行时实现；TypeScript parser 仍用于 CLI 和 Bun 测试。两者应
保持相同的字段语义和错误处理，不要让 Tauri command handler 直接绕过 parser 读取源文件。

## 昵称、标签和成就

账户显示名优先来自 ACLOS 身份缓存和 `snapshot<openid>` 的 `ss_nick`/`ss_nick_id`，
再回退到日志/原始 ID 的通用解析，最后才显示 openid 或匿名占位。优先选择带最新
`matches_time`（没有时用 `ss_time`）的记录，避免旧昵称覆盖新昵称。

MVP/SVP 来自 snapshot 的 `ss_type === "match"` 记录：

- `ss_achieve_type` 为 `mvp` 或 `svp`；
- `ss_type_str` 是显示标签；
- `matches_id` 与账户 ID 一起用于连接对局。兼容旧格式的明确 MVP/SVP 标记也保留。
- 普通只来自明确 `ss_type === "match"` 且 `ss_achieve_type === ""` 的记录；缺少字段
  或 snapshot 时归缺失成就，不能把没有信息解释成普通。
- `ss_result_infos.IsDraw` / `HasWon` 只从 match 记录读取，接受布尔值、0/1 及其字符串形式。
  每项事实使用最新已知值，缺失字段不覆盖旧的有效事实；非法标记保持缺失。

主记录的 `stats.result_known` / `score_known` 由 reader 根据原始胜负字段及两个比分字段
是否齐全生成，避免默认 false/0 被当作真实结果。团队模式使用明确平局或完整有效的
非零相同比分；0:0 隐藏比分，已知胜负只显示胜/负，否则用灰色“—”。个人乱斗和异变轮战
不套用比分平局规则、不显示回合比分。特殊模式的主记录 false 与回合比分不足以确认失败；
使用 match 摘要的明确 HasWon，或主记录明确 true，否则保持未知。当前没有可靠最终名次字段，
不通过 KDA 或分数推算名次。

reader 的 `stats.missing_stats` 保存缺失的 kills/deaths/assists/score 字段名，区别于明确零值。
缺失统计显示“—”；特殊模式全零且缺少可靠结算时保留数值并提示统计可能不完整。
旧索引若已经把缺失字段默认成零，无法单凭该索引还原源字段是否缺失；重新解析可补充标记。

事件里的 KillerPlayerName / KilledPlayerName 用于事件列表和时间轴提示。
PlayerName 可能保留本人编号，而击杀双方名称没有编号，不能将本人编号补给其他人。
这些局部记录不代表组队或完整阵容；AssistInfos 的角色 ID 也不能当作昵称。

快照读取失败时，本机索引保留已有昵称、成就与对局补充信息；文件存在但损坏或读取失败
时会在后续扫描重试。只有成功解析的快照才替换成就及摘要，失败不会阻止主高光列表读取。

主列表可能混有缺少 `matches_id` 的片段，包含战技测试等记录；有视频不等于具有完整
对局身份。这些记录单独跳过，不生成虚构对局编号，也不阻塞同账户的正常对局更新。
重复对局编号、冲突视频编号或明确归属其它账户的记录同样隔离，来源顺序不能决定
冲突视频的归属。部分读取不清理该账户旧索引；补全后可重试。有效对局省略 `openID`
时，数字源文件名可提供账户归属，但不能覆盖源记录中明确不同的账户标识。

snapshot 是可选且有历史缺口的数据源。不能把缺失成就解释成没有成就，也不能把它用于
账户统计、胜率、排序、导出或跨对局聚合；UI 只允许在资料存在时显示并筛选。

成就筛选提供 MVP、SVP、其他。其他是普通与缺少成就信息的展示分组，
不会改变原始 normal/unknown 事实；旧筛选偏好的 normal/unknown 均迁移为 other 并去重。

房间类型筛选只读取主记录 mode，去首尾空白并忽略大小写后，customgame 归自定义，
其余归非自定义，包括空值或缺失。该二分是应用采用的分类规则，缺失标记可能造成遗漏。
不通过 stats.mode_name 中的 CustomGame 片段或 career.game_mode 补全分类；
靶场路径也可能带该片段。房间类型与玩法模式为独立条件，可叠加筛选。

## 展示字段和离线资源

竞技段位只读取主记录 `tier.tier_before` / `tier.tier_after`。合法编号 0 表示未定级；
字段缺失、空值、非法编号以及当前体系未使用的 1/2 表示未知，不能通过默认值补零。
原生 Rust serde 将该对象保留在顶层；TypeScript CLI reader 将未知字段保留在 extras，
GUI helper 兼容两种表示，明确存在的主字段优先。snapshot 升段子树的默认零值
不用于覆盖或补全该对局的竞技段位，账号当前段位也不能替代历史记录。

四杀/五杀标签分别来自明确的 `video_type === "四杀时刻" / "五杀时刻"`。
同局两种记录可同时显示，同类多个视频不重复显示或累计为发生次数。
没有标签仅表示缺少对应时刻视频；整场击杀数、视频标题和局部事件数量不用于推断。

已知地图、英雄和模式通过生成的本地 registry 解析名称和图片，避免相同实体受单条
ACLOS 记录影响而出现不同标签。未知实体可以显示 ACLOS 的通用 fallback，但不能为单个
账户或样本添加分支。

轮战机器人的 AbilityDraftAgent 使用同一 registry 与角色头像显示流程。
元数据维护时读取完整 agents 列表，并纳入该非普通可选角色；仅请求
isPlayableCharacter=true 会漏掉它。来源目前与 K/O 共用头像文件，按内容哈希去重，
角色身份与“轮战机器人”描述性中文标签分别保留。

- 资源 registry：`packages/gui/src/utils/generated/valorant-metadata.zh-CN.ts`；
- 段位 registry：`packages/gui/src/utils/generated/valorant-ranks.zh-CN.ts`；
- 统一解析：`packages/gui/src/utils/valorant-assets.ts`；
- 维护来源：`packages/gui/assets/valorant-source/`；
- 构建输出：忽略版本控制的 `packages/gui/public/valorant/` WebP。

`bun run update:valorant-metadata` 是维护者显式运行的联网更新命令；
`bun run update:valorant-ranks` 单独更新固定标准段位体系及徽章，按内容哈希保存来源 PNG。
段位源徽章可为非正方形，构建时透明留边完整缩放到 128×128 WebP；
`bun run assets:build` 和 `bun run assets:check` 使用已提交来源在本地构建和校验。应用
运行时不应依赖图片 CDN；未知 HTTP(S)、协议相对和外部 SVG 资源应退化为文字/占位。

用户可见字段优先级：

- 英雄：canonical agent id/name → `career.hero_name` → raw agent name；
- 地图：canonical map id → `career.map_name`/raw map name → path segment；
- 模式：`career.game_mode` 和本地 registry；
- 统计：击杀、助攻、死亡、比分来自 `stats.*`；
- 对局时间：`gameStartTime`/`gameEndTime`。

不要把 raw `map_id`、`agent_id`、`stats.mode_name` 或 `career.battle_id` 直接当成用户标签。

## 视频语义

- `video_type` 为 `击杀集锦` 或 `死亡集锦` 时是 montage；其它类型是 moment，例如
  三杀、四杀、五杀或其它时刻。
- 不要按数组位置或时长猜视频类型；ACLOS 可以调整顺序和时长。
- 清晰度 chip 使用 `video_level`；`video_resolution` 可能含换行或回车，应在显示前规范化。
- Rust `VideoItem.video_is_processing` 必须序列化为 WebView 需要的 `video_isProcessing`。

## 对局、片段和事件

视频事件位于 `videos[].rounds[].round_clips[].clip_events[]`：

- `round_sTime`、`clip_sTime`、`event_sTime` 和 duration 都是毫秒；
- `event_type` 是 `kill` 或 `death`；
- `event_ext` 可能包含 `EventName`、玩家名、`AgentName`、`EventTime`、武器和本地玩家标记。

事件时间必须按视频类型使用共享状态机：

| 视频 | 可见类型 | 视频时间 |
|---|---|---|
| `击杀集锦` | `kill` | `event_sTime` |
| `死亡集锦` | `death` | `event_sTime` |
| 其它 moment | `kill` | `clip_sTime + event_sTime` |

可见事件还必须具备 `EventName=Shot`、可解析且落在对局时间窗口内的 `EventTime`、玩家
名、AgentName、本地玩家标记、受击部位和落在视频时长内的 seek 时间。缺少证据、互相矛盾
或跨对局 Agent 不匹配的行应隔离，不创建列表行、统计数或进度条标记。

ACLOS 的 `KillerIsMe` 在高光视频中可能把队友事件也标成 1，因此事件数量不是本场 K/D。
K/D 必须使用 `m.stats.*`；事件列表只展示通过状态机的可播放事件。共享状态机由
`packages/gui/src/utils/event-state-machine.ts` 与 `src-tauri/src/library/events.rs`
共同维护，UI 负责跨视频去重，parser 仍应忠实返回源数据。

## 本地索引和 IPC

WonderfulUI 将完整 `matches.raw_json` 保留为可回放的权威源，同时把规范化事件写入
SQLite `events` 以便快速读取和未来扩展。批量 `scan_all`/`load_library` 会去掉 rounds，
打开单场详情时通过 `get_match_rounds` 按需读取完整数据。

本地视频相关 command 只能接受 SQLite `videos.path` 中登记、扩展名受支持、当前仍是普通
文件且不是符号链接的路径。播放、资源管理器定位、截图和快传共用这条后端校验；不要用
前端字符串检查替代它。

## 维护规则

- 新增 ACLOS 字段时先更新 parser model、TS/Rust 对齐测试和本文件；
- 不把一次性本机观察、个人账户样本、性能快照或历史方案写入当前契约；
- 对不确定的新字段使用安全 fallback，并保留原始数据供后续分析；
- 任何涉及源文件写入、目录选择或外部网络的新能力都必须重新审查安全策略和产品边界。
