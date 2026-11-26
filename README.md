# Kwork

Kwork is a native Rust and GPUI desktop for Kar coding sessions. The same workspace holds your conversation, terminal, file changes, and Office editors. New chats start with Aster. Existing conversations keep their original runtime. [简体中文](README.zh.md)

## Run

Install a stable Rust toolchain and the platform build tools. On macOS, install Xcode and its command-line tools. From a checkout of this repository, run:

```sh
cargo run --locked -p kar_desktop -- --workspace /path/to/project
```

To build a macOS application bundle:

```sh
cargo build --locked -p kar_desktop
./scripts/macos-kwrok-app.sh target/debug/kar-desktop target
open target/Kwrok.app
```

Kar is pinned to a Git revision in the workspace manifest. Cargo fetches the engine on the first build. Select an installed provider or install Aster ACP in the Agents panel, then configure its model provider in Aster.

Type `/` in the composer to see the actions supported by the selected session. With Codex, `/goal` manages a persistent native objective, and `/plan` drafts a plan in a read-only planning thread. Review the proposal and select **Implement plan** to start work in the original conversation. Aster uses its own ACP session modes and advertised commands. After a runtime handoff, the original chat retains its history and Kar owns the new controller. The command picker then offers the actions that Kar exposes for that controller.

The Remote panel offers Kar chat sharing and a separate remote desktop invitation launcher. On Windows and Linux the latter uses an installed KRemote client. On macOS it opens the registered compatible UU Remote client.

## Development

```sh
cargo test --locked -p kar_desktop
```

The desktop code is in [crates/kar_desktop](crates/kar_desktop). See the [Kantor documentation](KANTOR.md) for the embedded office suite. The runtime and remote client have their own distributions. [Kar](https://github.com/kense-labs/Kar) · [Aster](https://github.com/aster-ai-lab/aster) · [KRemote](https://github.com/kense-labs/kremote)

## Licenses

Kense licenses its original code in Kwrok and the Kar runtime under MIT. [Kwrok MIT license](LICENSE-MIT) · [Kar license](https://github.com/kense-labs/Kar/blob/main/LICENSE)

The desktop application incorporates the Kantor office suite and is distributed as a whole under AGPL-3.0-or-later. Third-party components retain their own licenses. [Desktop license](LICENSE) · [Third-party notices](NOTICE)
