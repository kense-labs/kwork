# KWork 真机验收清单（2026-11-15）

对照上游主分支 `39163dd` 的对齐成果；本次验收在合并提交 `91b10d5` + `b57db86` 之后执行。
本清单分三块：**A. 已自动化验证**、**B. 自动脚本待修/待人工**、**C. 必须真机人工操作**。
所有验证命令都可在仓库根目录复跑；headless 验证使用 Chromium（不占用鼠标/键盘/主屏）。

## A. 已自动化验证（本次已复跑通过）

| 项 | 验证方式 | 结果 |
| --- | --- | --- |
| 全部 Rust 单测 | `cargo test --workspace` | 59 宿主 + 290 UI，全过（2 忽略） |
| 全部脚本测试 | `bun test scripts/*.test.mjs` | 43 项 / 10 文件，全过 |
| 质量门 | `bun run check`（fmt + 严格 clippy `-D warnings` + 测试） | 通过 |
| 宿主/wasm 编译 | `cargo check -p dsh-gpui`、`cargo check -p dsh-gpui-ui --target wasm32-unknown-unknown` | 通过 |
| 前端产物 | `bun run ui:build` | 通过 |
| Markdown/工具卡（公式、diff、图片、抓取失败） | `bun scripts/markdown-tools-browser.mjs --mode image` | 通过，产物 `.tools/verification/markdown-tools-browser/image/` |
| 流式公式（live → settled） | `--mode stream` | 通过 |
| 轨迹视图（打开、折叠、搜索、检查器、快捷键） | `--mode trajectory` | 通过 |
| 插件状态/徽标 | `--mode plugins` | 通过 |
| 工作流运行卡片 | `--mode workflow` | 通过 |
| 外观（主题+字号） | `--mode appearance` | 通过 |
| 计费设置只读保护 | `--mode settings-readonly` | 通过 |
| 计费设置搜索/服务商 | `--mode settings-search` | 通过 |
| Fetch 计费 | `--mode settings-fetch` | 通过 |
| Agent 终端 UI（attach/输入/关闭） | `bun scripts/agent-terminal-browser.mjs` | 通过 |

复跑示例：

```bash
bun run ui:build
bun run check
bun test scripts/*.test.mjs
bun scripts/markdown-tools-browser.mjs --mode image
bun scripts/agent-terminal-browser.mjs
```

## B. 自动脚本待修 / 待人工确认（不一定是产品缺陷）

| 项 | 计划脚本 | 现象 | 处理 |
| --- | --- | --- | --- |
| 外观主题断言（`all` 模式的深浅两档） | `--mode all` | 等待 `configure_shell_window` 深色偏好 20s 超时；`PREFERENCE_WAIT []` | 脚本的 fixture 不注入原生外观；改用 `--mode appearance` 已通过。可修 fixture |
| `--mode theme` | 同上 | 该模式没有独立分支，实际跑进 Markdown 断言后超时 | 脚本遗留模式，建议删除或并入 appearance |
| Team 模式启用 | `--mode team` | 面板/菜单截图成功，最后一步 `team_state enabled=true` 超时 | 点击坐标需按当前布局重钉；属脚本维护 |
| 计费设置保存/部分保存 | `--mode settings`、`--mode settings-partial` | 卡在 `save-search` / `save-partial` 的保存按钮点击（截图见失败目录） | 保存按钮布局/坐标已变，需重钉或人工确认保存逻辑 |

失败产物：`.tools/verification/markdown-tools-browser/<mode>/failed-step.png` 与 `failed-step.json`。

## C. 必须真机人工清单（headless 覆盖不到）

逐项打开 KWork（dev 构建）操作并勾选：

### C1. 会话拖拽排序
- [ ] 会话列表切到「手动排序」，拖动一个会话到另一位置：出现 4px 阈值、2px 插入线、源行半透明
- [ ] 拖到顶部松开后顺序保持；重启应用后顺序保留
- [ ] Escape / 指针取消拖拽后恢复原顺序
- [ ] 置顶会话只能在其分区内调整

### C2. 工作区拖拽排序
- [ ] 平铺模式拖拽工作区分组，落点显示插入线
- [ ] 树模式下拖动父工作区：整组移动且后代顺序不变
- [ ] 重启后工作区顺序保留
- [ ] 隐藏/归档成员不丢失顺序

### C3. 分叉/继承历史
- [ ] 打开一个有继承历史的真实分叉会话：显示「以上为继承的历史消息」分隔线
- [ ] 加载更早记录后，分隔线仍在最后一个 `session/end-seed` 处、位置稳定
- [ ] Turn 内部正好是切点时不出现重复/错位行
- [ ] 深色/浅色两套主题下图标与文案正常

### C4. 轨迹视图（原生窗口）
- [ ] 顶栏「轨迹」页签可打开，记录列表/时间概览正常
- [ ] 点击记录打开检查器，折叠/搜索/方向键与 Space 键可用
- [ ] 原生窗口尺寸变化（1280×820 / 最大化）后布局不溢出

### C5. Agent 终端（原生 WKWebView）
- [ ] 打开终端并让 Agent 执行命令：出现 Agent/沙箱徽标
- [ ] 用户接管后输入、粘贴、resize、关闭正常
- [ ] 关闭终端后重开、后台 jobs 场景正常
- [ ] 失败场景（无终端模块）应用不崩溃，只提示降级

### C6. 会话阅读状态（多前端同步）
- [ ] 侧栏未读标记出现；进入会话后消失
- [ ] 失败红点出现/清除
- [ ] `Mod+Shift+U` 快捷键生效
- [ ] 另一端（Web 端/第二个窗口）操作后本端状态同步

### C7. 基础聊天与工具（真机回归）
- [ ] 新建会话发送提示词：流式输出正常
- [ ] 审批卡（bash/write）允许一次/始终允许/拒绝
- [ ] `/goal` 创建目标：GoalBar 显示，目标轮次卡片正确（不带原始 `<goal_round>` 标题）
- [ ] 插话（Cmd+Enter）显示「插话 · 待回应」，Host 受理后变持久气泡
- [ ] 归档→撤销：提示条「撤销 / 筛选已归档会话」正常，撤销即时恢复
- [ ] 交付物卡、已编辑文件卡、统计弹窗可用

### C8. 窗口与设置
- [ ] 首次启动引导/欢迎页正常
- [ ] 「跟随系统」主题：切换 macOS 深浅色时 ≤2s 跟随
- [ ] API Key 显隐、连接服务、模型选择、快捷键设置正常
- [ ] 设置弹窗可滚动到底部

## D. 已知不可达（ACP 协议限制，接受差异）

工具 diff meta、独立退出码胶囊的 meta、`read_image` 图片数据（当前用引用/附件替代）、agent 终端徽标的部分 Host 数据、
账号通知、插件完整状态字段、最小投递间隔、会话日志上传。详见 `docs/parity-rc3.md`。

## 验收记录

- 执行人：______
- 日期：______
- 结果：______（全部通过 / 例外项：______）
