# 与官方 DSH rc.3 的逐项对齐记录

对照对象：`deepseek-harness` 标签 `dsh-v0.2.2-rc.3`
（commit `38a6a5c965aa99ab525161efb7cc5cf1fe62fced`，快照位于 `/tmp/dsh-rc3`）。
KWork 是 Rust/GPUI 客户端，通过 ACP 与 Host 通信；官方客户端是 React/Web
（`packages/client/*`）。两边共享同一 Host 协议，差别集中在**客户端渲染与本地能力**。

本文记录三类内容：

- **已对齐并验证**：行为与官方一致，且有真机或测试证据；
- **近似实现**：官方能力依赖 Web/浏览器或第三方库，GPUI 客户端用等价降级方案；
- **不可达 / 不适用**：ACP 协议没有暴露所需数据，或功能依赖官方 Web 端专有能力。

## 2026-11-15 收尾合入（本次提交）

上一节记录的主分支复核由 Claude Code 桌面会话整夜完成主要实现；本次收尾在结束后接手：
修正模块 `cfg` 门控（`workspace_tree` 依赖 `sessions`、`workflow_runs` 被原生宿主共享）并统一格式，
跑完自动化检查后把整批改动合入 `main`。

**本次合入的能力**（全部带单元/脚本测试覆盖）：

- 工作区层级嵌套与折叠、工作区/会话拖拽排序（平铺/树模式、置顶分区、顺序持久化与恢复）；
- 会话已读/未读状态：Host 权威标记、侧栏指示、快捷键、失败红点与前端同步；
- 分叉/继承历史分隔线「以上为继承的历史消息」（跨页切点与过程组切分）；
- 轨迹视图（标签页、记录检查器、时间概览）；
- 网页搜索/抓取的计费设置（自动/账号/API Key 及高级覆盖，Host 校验）；
- Agent 终端：ACP 终端生命周期桥、安装控制器、六个模型工具、自动 attach、Agent/沙箱徽标；
- Team 模式、工作流运行卡片（历史与实时）、插件状态与扩展徽标、独立字体设置；
- 文档与对话数学公式（MathJax 排版为实测 SVG，含行内/块级）、Shiki 代码高亮、文本搜索；
- 工具结果呈现：编辑差异卡、`read_image` 图片卡、网页抓取失败原因、Shell 结束状态；
- 宿主侧：会话阅读状态、Agent 终端桥、历史投影、终端、偏好设置的配套命令。

**本次验证（2026-11-15）**：

- `cargo test --workspace`：**59 宿主 + 290 UI** 全部通过（2 忽略）；
- `bun test scripts/*.test.mjs`：**43 项 / 10 个文件** 全部通过；
- `cargo check -p dsh-gpui-ui --target wasm32-unknown-unknown`、`cargo check -p dsh-gpui`、`bun run ui:build`、`cargo fmt --all --check`、`git diff --check` 均通过；
- `cargo clippy --workspace --all-targets -- -D warnings`：**通过**；`bun run check`（fmt + 严格 clippy + 全部测试）绿色。原生 target 下 wasm-only渲染代码通过 crate 级 `cfg_attr(not(wasm), allow(dead_code))` 说明其仅为测试编译，其余告警已逐条修复或就地注明原因。

**仍待真机复核（不影响合入，记录在案）**：

- 轨迹详情与原生标签页的真实交互；
- 树/平铺模式下拖拽的完整真机回归、取消操作与重启恢复；
- 继承历史分隔线在 Turn 内切点、完整 prepend 后的页尾稳定性；
- Agent 终端在原生 WKWebView、显式关闭与后台 jobs 场景；
- 插件徽标完整呈现、阅读状态的多前端同步。

上表「不可达 / 不适用」中的 ACP 协议缺口仍然成立。

## 2026-11-14 主分支复核（过程记录）

最新对照快照为上游主分支 commit `39163dd30f703a79079e2d2446545cff3c453738`（2026-11-13），本地只读检出位于 `.tools/reference/deepseek-harness`。该快照相对原 rc.3 基线在客户端与桌面端有 669 个文件变化；原“已对齐”结论只作为历史证据，相关包必须重新验证后才能转入最终验收。

当前差距队列（不代表每一项都尚未开始）：

| 功能 | 上游来源 | 当前已确认差距 |
| --- | --- | --- |
| 工作区层级 | `ui-workspace` | 已实现最近可见祖先嵌套、每层 12px 缩进、子工作区优先、后代折叠水位线清理、祖先高亮与搜索展开。临时父/子/同级工作区已实测嵌套缩进、父级整组折叠、展开和重启恢复；会话“展开其余”已加载更多记录，折叠父级清空后代水位线仍待真机触发。 |
| 会话拖拽排序 | `ui-workspace` | 已实现平铺/工作区/树的同账户拖拽、4px 阈值、2px 插入线、源行半透明、Escape/指针取消、隐藏/折叠/归档成员保留、置顶分区、置顶联动与空白会话首位；`sessionOrderByAccount` 持久化及模式切换清理。工作区分组内真实拖拽、恢复原顺序及重启保留已通过，树和平铺模式仍待验收。 |
| 工作区拖拽排序 | `ui-workspace` | 已实现平铺/树的同级拖拽、4px 阈值、整组区域命中、后代落点向兼容祖先归一化、2px 插入线、源行半透明、Escape/指针取消；移动父工作区保留后代顺序，隐藏项保留完整顺序。当前 ACP 无 Host 重排接口，使用 `workspaceOrder` 本地持久化，独立于会话排序。树内同级和父级整组拖拽、顺序及折叠重启恢复已通过；平铺和取消操作仍待验收。 |
| 会话阅读状态 | `ui-session-status`、`ui-workspace` | 已接线 Host 权威阅读状态、非乐观标记、主视图确认、失败红点、Mod+Shift+U 与前端同步；58 项宿主测试、259 项 UI 测试及 16 项脚本测试通过。真实 Host 的快捷键、跨会话提醒、进入确认、菜单未读/已读切换和模态框快捷键限制已通过；右栏焦点、失败状态及多前端同步仍待真机验收。 |
| 继承历史分隔线 | `ui-chat` | 已在最后一个继承的 `session/end-seed` 处显示分支图标与“以上为继承的历史消息”，支持跨页连接、普通 seed 排除、页尾切点及过程组切分。`gpui-pre 0.3.10` 无 ARIA separator 对应角色，暂以有标签的非交互 Group 表达。真实分叉的深色/浅色图标、细线、文案及加载更早记录已通过；Turn 内切点和完整 prepend 后页尾稳定性仍待验收。 |
| 轨迹视图 | `ui-trajectory` | 已有轨迹标签页、记录检查器和时间概览；隔离 Chromium 中无 ACP 离线历史已打开，未尝试 writer，也未发送消息；记录详情交互和原生验收仍待完成。 |
| 网页搜索与获取计费 | `ui-settings-web-search` | 已实现 Search/Fetch 的自动、账号、API Key 计费及高级引用覆盖，Host 同时校验 provider 与覆盖关系；专项测试通过，设置页 UIUX 尚未验收。 |
| Agent 终端标识 | `ui-sidebar-terminal` | 已实现三层官方终端插件、六个模型工具、自动 attach、用户接管、Agent/沙箱徽标及会话侧栏；真实 ACP executor smoke 与安装/生命周期测试通过。Chromium WASM 已验证 snapshot、物理输入及 attach 幂等；显式关闭、后台 jobs、原生 WKWebView 和模型自主调用仍待验收。 |
| 插件状态与扩展徽标 | `ui-plugin-manager` | 需复核实验性、异常、未运行、可更新及详情贡献徽标的完整呈现。 |
| 数学公式 | `ui-primitives` | 仍显示 TeX 源码，不是 KaTeX 排版。 |
| 代码高亮 | `ui-primitives` | 仍是轻量语法着色，需用现有 Shiki 资产或等价完整实现替换。 |
| 文件差异 | `ui-deliverables` | 仍只有变更文件列表，没有官方 diff 卡及对比页签。 |
| 图片工具结果 | `ui-tool` | 仍需补齐 `read_image` 图片卡及通用结果中的图片引用摘要。 |
| Shell 结束状态 | `ui-tool` | 需统一非零退出码、终止信号和无退出码的胶囊展示。 |
| 网络抓取失败 | `ui-tool` | 需按已分类网络错误显示本地化原因与建议，同时保留原始输出。 |

本轮基线：`cargo test --workspace` 通过（56 宿主 + 231 UI，2 忽略），`bun test scripts/*.test.mjs` 通过（12 项）。随后修复两处遗漏的 `#[test]` 与一处重复注册，UI 测试变为 232 项且全部通过。严格原生 Clippy 仍失败：其中大量 `dead_code` 来自原生目标不使用 wasm 渲染入口，另有独立 lint；最终验收需分别检查原生 Host 与 wasm UI，不能用全局 allow 掩盖。

工作区树改动后：`cargo test --workspace` 通过（56 宿主 + 237 UI，2 忽略），`bun test scripts/*.test.mjs` 通过（12 项），wasm 编译及 `git diff --check` 通过。严格 wasm Clippy 报告 47 项存量 lint（不涉及新增树逻辑模块），待集中清理；树模式尚未完成真机交互验收。

会话拖拽改动后：`cargo test --workspace` 通过（57 宿主 + 244 UI，2 忽略），`bun test scripts/*.test.mjs` 通过（12 项），wasm 编译及 `git diff --check` 通过。严格 wasm Clippy 仍报告同样的 47 项存量 lint，不涉及新增排序或拖拽模块；拖拽和置顶联动尚未完成真机交互验收。

工作区拖拽改动后：`cargo test --workspace` 通过（58 宿主 + 250 UI，2 忽略），`bun test scripts/*.test.mjs` 通过（12 项），wasm 编译、`cargo fmt --all -- --check` 及 `git diff --check` 通过。新增 6 项顺序/落点测试及 1 项偏好兼容测试；工作区拖拽和本地顺序恢复尚未完成真机交互验收。

继承历史分隔线改动后：`cargo test --workspace` 通过（58 宿主 + 259 UI，2 忽略），新增 4 项继承切点测试；wasm 编译通过。尚未把此结果视作真实分叉会话的 UI 验收。

## 已对齐并验证（原 rc.3 快照，待按最新主分支复核）

| 领域 | 官方来源 | 说明与验证方式 |
| --- | --- | --- |
| 会话转录 | `ui-chat` | 用户/助手/思考/工具/系统行、轮次分隔、错误横幅、历史分页（加载更早/重新加载/超时保护）；真机 + 224 UI 测试 |
| Markdown 渲染 | `ui-primitives/markdown` | 标题、列表、表格、代码围栏与高亮、diff 围栏、图片、引用、链接、行内代码；未配对 `**` 死循环已修（`4bc8a33`） |
| 数学围栏 | `ui-primitives/markdown` KaTeX | 降级为居中 TeX 源码 + 「TeX 公式源码」标注（见“近似实现”） |
| 类型化工具卡 | `ui-tool-*` | shell（`$ 命令` + 描述）、read（行号 + 文件尾注）、grep（文件分组）、web_fetch（URL + HTTP 状态 + 正文）；暗色/浅色均验证 |
| 交付出物卡片 | `ui-sidebar` DeliverablesTail | 路径 + 描述、2 列网格、全部/收起；点击预览在侧栏打开文档、默认应用打开、Finder 显示；真机验证 |
| 已编辑文件卡 | `changed-files` | 文件列表行点击在侧栏打开；行数 `+N/-M` 不可达（见下） |
| 插话与排队 | `ui-chat` PendingSteering / QueueDock | 运行中插话的行内回显为转录区末尾的用户气泡（「插话 · 待回应」），Host 受理后变为持久消息；排队消息仍在输入区上方的 QueueDock 卡片中编辑/插话/删除；真机验证 |
| 审批流 | `ui-approvals` | 命令摘要、工具标签本地化、拒绝/允许一次/始终允许；真实审批流程验证过 |
| 计划/待办 | `ui-chat` plan card、`todo/write` | 计划卡与待办行；ACP 事件回放验证 |
| 子代理 | `subagent/*` | 子代理目录、描述行、只读子会话视图；回放与真实会话验证 |
| 后台任务 | `ui-jobs` | 注册/结算行、时长、退出码；真实会话验证 |
| 模型重试 | `llm/retry*` | 单行 upsert、延迟/失败原因；回放不显示永动等待点 |
| 工作区侧栏 | `ui-sidebar` | 工作区分组、置顶/隐藏/折叠、会话归档与重命名、排序、分组切换 |
| 文件侧栏 | `ui-sidebar-files` | 搜索、按工作区持久化的排除规则、显示隐藏项、忽略文件开关 |
| 内置浏览器 | `ui-sidebar-browser` | 右栏标签、地址栏、前进后退刷新、恢复上次页面 |
| 设置 | `ui-settings` | API Key / 通用 / 外观 / 模型 / 模型服务 / 内置插件 / Agent 预设 / 会话日志各页真机走查；弹窗内容可滚动（性能与用量、版本行可达），输入、选择器与状态文案正常 |
| 文件类型图标 | `ui-primitives` FileTypeIcon | Files 树与文档页签按扩展名显示类型图标（Markdown 用 MD 标记、图片用显示器图标、其余通用文档图标；官方每种语言有全彩图标，此处为可辨识的近似）；真机验证 |
| API Key 显隐 | `ui-settings-models` ApiKeyInput | 编辑草稿时眼睛按钮切换明文/掩码（仅未保存草稿，保存后凭据仍为只写）；真机验证 |
| 外观偏好 | `ui-theme` | 浅色 / 深色 / 跟随系统三选一，选中项跟随**已存偏好**而非解析结果；跟随系统时实时响应 macOS 外观切换（轮询）；偏好持久化 `themeMode`；真机验证 |
| 命令面板 | `ui-command-picker` | 命令检索与执行 |
| 输入区统计 | `ui-composer` | 轮/步/Token/缓存命中/耗时，点击展开统计弹窗；真机验证 |
| 快捷键 | `ui-shortcuts` | 可录制、平台适配、冲突提示 |
| 空/错误状态 | 多处 | 欢迎页、历史失败与超时、文件不可读、目录不存在等文案对齐 |

## 近似实现

| 功能 | 官方实现 | KWork 方案 | 原因 |
| --- | --- | --- | --- |
| 数学公式 | KaTeX 排版 | 居中 TeX 源码 + 标注 | GPUI 无 TeX 引擎，引入 KaTeX 需 WebView/JS 桥 |
| 内置浏览器 | Electron WebContentsView | WKWebView（Tauri） | 平台差异；标签/恢复/前进后退已对齐 |
| 外部链接 | 系统浏览器 | 本机 open_url（http/https/file） | 已按官方“侧栏或外部”偏好分流 |
| Markdown 图片 | `<img>` 懒加载 | 原生 image 元素 + 本地相对路径解析 | 依赖本机文件读取 |
| 代码高亮 | shiki | 轻量关键词/字符串/数字/注释着色 | 不引入 JS 依赖；主要语言可读性足够 |
| 会话列表实时性 | 服务端事件推送 | Host 事件 + 本地 projcache 扫描合并 | ACP 列表接口不含全部本地会话 |

## 不可达 / 不适用（ACP 协议或 Host 限制）

| 功能 | 官方数据来源 | 现状 | 说明 |
| --- | --- | --- | --- |
| 文件 diff 卡 | `meta.diffs` / Host 路由 | 只显示变更文件列表与打开动作 | 实时 ACP 工具更新不带 `meta.diffs`，也没有 diff 路由 |
| bash 退出码胶囊 | 工具 meta | 部分工具结果文本含 `exit code`，无独立胶囊 | ACP 实时 meta 只有 `cwd` 等有限字段 |
| read_image 图片卡 | 工具 meta 图片数据 | 图片类工具按普通文本行展示 | ACP 不传图片数据 |
| Agent 终端徽标 | `ui-sidebar-terminal` | 不支持 | KWork 无 agent/沙箱终端；本机 shell 仅限开发者工具 |
| 账号通知 | `ui-account-notifications` | 不适用 | 官方 Web 端账号体系能力 |
| 插件管理状态 | 设置 schema 下发 | 只展示 Host 提供的信息 | Host 未提供完整插件 schema/版本字段 |
| 上下文用量环 | 客户端估算 | 使用 Host 提供的 usage | 估算口径不同，仅展示 Host 数据 |
| 最小投递间隔提示 | 官方 Web 端 `schedule.deliveryLimits()` | 仅保留操作失败时的「等待最短投递间隔」提示 | delivery limits 走 Web remote 接口，ACP 没有对应方法；新增自动化时无法显示具体间隔秒数 |
| 会话日志上传 | 官方设置页上传本地 transcript | 不上传 | Host 未开放上传通道，设置页已明确说明 |
| 数学 KaTeX 排版 | KaTeX | 近似实现 | 见上表 |

## 验证方式

- **原生测试**：`cargo test --workspace`（56 宿主 + 225 UI）。
- **wasm 检查**：`cargo check -p dsh-gpui-ui --target wasm32-unknown-unknown`（0 warning）。
- **脚本测试**：`bun test scripts/*.test.mjs`（12 项）。
- **真机验证**：`scripts/dev-app.sh` 构建 dev app，配合：
  - 真实会话（只读回放、分页、审批、作业、子代理）；
  - debug 构建的渲染演示快捷键 `Cmd/Ctrl+Shift+D`（数学围栏、类型化工具卡、
    交付卡片、已编辑文件卡、Markdown 链接与 web 卡片），可在浅色/深色主题下截图核对；
  - 窗口级截图 + OCR 坐标自动化（`/tmp/kwork-att/*` 工具，非仓库内容）。
- **CI**：GitHub Actions `windows.yml` 在每次 push 到 main 后构建 Windows 安装包。

## 验收快照（第 28 轮，macOS 真机 debug 构建）

- **冷启动/欢迎页**：侧栏、工作区、输入区正常，无 panic（原生日志仅启动行）；
- **真实回放会话**：1877 事件会话打开后正常渲染、可滚动，WebContent CPU ~1%（渲染窗口化生效，第 15 轮修复）；
- **实时发送**：新建会话发送提示词，1 秒完成并渲染回复，CPU ~2%；
- **归档/撤销**：归档提示含「撤销 / 筛选已归档会话」（第 26 轮），撤销后侧栏行即时恢复、随后出现「已取消归档」（第 27 轮）；
- **主题**：浅色 / 深色 / 跟随系统三档，OS 切换后 ≤2s 跟随（第 21 轮）；
- **渲染卡片**：数学围栏、类型化工具卡（shell/read/grep/web）、交付物、已编辑文件、统计弹窗、web 卡 URL 打开（第 17–20 轮）；
- **插话回显**：运行中插话以「插话 · 待回应」气泡出现在转录区末尾（第 23 轮）；
- **API Key 显隐**、**文件/页签扩展名图标**（第 22、24 轮）。

自动化：`cargo test --workspace`（56 宿主 + 229 UI）、`bun test scripts/*.test.mjs`（12）、
`cargo clippy --workspace --all-targets`（0 error）、`cargo fmt --all --check`、
`cargo check -p dsh-gpui-ui --target wasm32-unknown-unknown`（0 warning）；
GitHub Actions `windows.yml` 对 main 每次 push 构建 Windows 安装包。

### 尚未真机覆盖的细节

- 图片类型文件图标的真机点击（分类器有单测，Markdown 图标已实测）；
- QueueDock 的 next-turn 排队卡路径未能端到端触发（当前 Host 把普通 Enter 也投递到 next-step）；
- 深浅主题按像素采样判断，未做逐表面自动化对比度校验。

### 结论

在 ACP 协议与 GPUI/WKWebView 的能力边界内，官方 rc.3 客户端中可对齐的交互与渲染已逐项实现并验证；
剩余项为「不可达（见上表）」或需要新增大块能力的独立功能（工作区树拖拽重排、轨迹视图、网络搜索计费、终端徽标、插件卡片徽标）。

最后更新：第 29 轮（2026-11-15 收尾合入，对照上游主分支 `39163dd` 复核记录）。
