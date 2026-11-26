# Kwork

Kwork 是基于 Rust 和 GPUI 的原生桌面工作台。可以在同一个项目里继续 Kar 对话、审查文件差异、打开终端和编辑 Office 文档。新聊天默认选择 Aster，重开已有聊天会沿用其运行时。[English](README.md)

## 启动

安装稳定版 Rust。macOS 还需安装 Xcode 和命令行工具。首次构建时 Cargo 会下载锁定版本的 Kar 引擎。

```sh
cargo run --locked -p kar_desktop -- --workspace /path/to/project
```

在智能体面板安装 Aster ACP，并在 Aster 配置中选择可用的模型服务。输入 `/` 可以查看当前会话支持的操作。Codex 会话的 `/goal` 可以设置、暂停和恢复持续目标。`/plan` 先在只读规划线程中起草方案，确认实施后回到原对话执行。Aster 会话使用它自身提供的 ACP 模式和命令。切换智能体交接后，对话保留原有记录，由 Kar 控制接手的智能体。命令菜单也按新的控制能力更新。

远程面板提供 Kar 会话分享和远程桌面邀请两个入口。打开桌面邀请需要本机安装兼容客户端。Windows 和 Linux 可使用 KRemote，macOS 可使用已注册的 UU 远程客户端。

```sh
cargo test --locked -p kar_desktop
cargo build --locked -p kar_desktop
./scripts/macos-kwrok-app.sh target/debug/kar-desktop target
open target/Kwrok.app
```
