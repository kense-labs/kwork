//! Own the account renderer process and place its native window inside KWork.

use super::{Bounds, Page, PlatformGrant};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap, os::windows::io::AsRawHandle, path::PathBuf, process::Stdio,
    time::Duration,
};
use tauri::{Manager, Runtime, Webview};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, WriteHalf},
    net::windows::named_pipe::{NamedPipeServer, ServerOptions},
    process::{Child, Command},
    sync::oneshot,
    time::timeout,
};
use windows::Win32::{
    Foundation::{HANDLE, HWND},
    System::Pipes::GetNamedPipeClientProcessId,
    UI::WindowsAndMessaging::{
        GWL_STYLE, GetParent, GetWindowLongPtrW, GetWindowThreadProcessId, HWND_TOP,
        SWP_FRAMECHANGED, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetParent,
        SetWindowLongPtrW, SetWindowPos, WS_CHILD, WS_CLIPSIBLINGS, WS_POPUP,
    },
};

const WAIT: Duration = Duration::from_secs(20);
const CLOSE_WAIT: Duration = Duration::from_secs(7);
const OPEN_ID: u64 = 1;
const CLOSE_ID: u64 = 2;

pub(super) struct PlatformView {
    child: Child,
    input: WriteHalf<NamedPipeServer>,
    handle: usize,
    pid: u32,
    _temporary: Option<tempfile::TempDir>,
}

fn hwnd(raw: usize) -> HWND {
    HWND(raw as *mut std::ffi::c_void)
}

fn owns_window(raw: usize, pid: u32) -> bool {
    let mut owner = 0;
    // SAFETY: the handle is tested through User32 before any mutation.
    let thread = unsafe { GetWindowThreadProcessId(hwnd(raw), Some(&mut owner)) };
    thread != 0 && pid != 0 && pid == owner
}

fn decode_handle(value: &str) -> Option<usize> {
    if value.len() != std::mem::size_of::<usize>() * 2 {
        return None;
    }
    let mut bytes = [0; std::mem::size_of::<usize>()];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(value.get(index * 2..index * 2 + 2)?, 16).ok()?;
    }
    let raw = usize::from_le_bytes(bytes);
    (raw != 0).then_some(raw)
}

impl PlatformView {
    pub(super) async fn spawn<R: Runtime>(
        owner: &Webview<R>,
        storage: Option<[u8; 16]>,
        report: impl Fn(bool) + Send + 'static,
    ) -> Result<Self, String> {
        let app = owner.app_handle();
        let bundle = app
            .path()
            .resource_dir()
            .map_err(|_| "找不到用量与充值组件。")?
            .join("account-platform");
        let executable = bundle.join("electron.exe");
        let entry = bundle.join("resources").join("app");
        if !executable.is_file() || !entry.join("main.cjs").is_file() {
            return Err("用量与充值组件未安装，请重新安装 KWork。".into());
        }
        let (directory, temporary): (PathBuf, _) = match storage {
            Some(id) => (
                app.path()
                    .app_local_data_dir()
                    .map_err(|_| "找不到独立的平台页面存储。")?
                    .join("account-platform-electron")
                    .join(uuid::Uuid::from_bytes(id).to_string()),
                None,
            ),
            None => {
                let temporary = tempfile::Builder::new()
                    .prefix("kwork-platform-")
                    .tempdir()
                    .map_err(|_| "未能创建独立的平台页面存储。")?;
                (temporary.path().to_owned(), Some(temporary))
            }
        };
        std::fs::create_dir_all(&directory).map_err(|_| "未能创建独立的平台页面存储。")?;
        let mut data_arg = std::ffi::OsString::from("--kwork-platform-data=");
        data_arg.push(&directory);
        let pipe_path = format!(r"\\.\pipe\kwork-account-{}", uuid::Uuid::new_v4());
        let pipe = ServerOptions::new()
            .first_pipe_instance(true)
            .reject_remote_clients(true)
            .create(&pipe_path)
            .map_err(|_| "无法创建平台页面的通信通道。")?;
        let mut child = Command::new(executable)
            .arg(entry)
            .arg(data_arg)
            .arg(format!("--kwork-platform-pipe={pipe_path}"))
            .arg(concat!(
                "--kwork-client-version=",
                env!("CARGO_PKG_VERSION")
            ))
            .current_dir(bundle)
            .env_remove("ELECTRON_RUN_AS_NODE")
            .env_remove("ELECTRON_ENABLE_LOGGING")
            .env_remove("NODE_OPTIONS")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| "无法启动用量与充值组件。")?;
        let pid = child.id().ok_or("无法验证用量与充值组件进程。")?;
        tokio::select! {
            connected = timeout(WAIT, pipe.connect()) => {
                connected.map_err(|_| "连接用量与充值组件超时。")?
                    .map_err(|_| "无法连接用量与充值组件。")?;
            }
            _ = child.wait() => return Err("用量与充值组件提前关闭。".into()),
        }
        let mut peer = 0;
        // SAFETY: this is the connected server handle and peer outlives the call.
        let peer_result =
            unsafe { GetNamedPipeClientProcessId(HANDLE(pipe.as_raw_handle()), &mut peer) };
        if peer_result.is_err() || peer != pid {
            return Err("无法验证用量与充值组件的连接。".into());
        }
        let (output, input) = tokio::io::split(pipe);
        let mut lines = BufReader::new(output).lines();
        let (ready, receive) = oneshot::channel();
        tauri::async_runtime::spawn(async move {
            let mut ready = Some(ready);
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(frame) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if frame["event"] == "ready"
                    && let Some(ready) = ready.take()
                {
                    let handle = (frame["pid"].as_u64() == Some(u64::from(pid)))
                        .then(|| frame["handle"].as_str().and_then(decode_handle))
                        .flatten();
                    let _ = ready.send(handle);
                }
                if frame["event"] == "failed" || frame["id"] == OPEN_ID {
                    report(frame["ok"] == true);
                }
            }
            report(false);
        });
        let handle = timeout(WAIT, receive)
            .await
            .ok()
            .and_then(Result::ok)
            .flatten()
            .filter(|handle| owns_window(*handle, pid))
            .ok_or("用量与充值组件启动失败。")?;
        let parent = owner.window().hwnd().map_err(|_| "无法定位 KWork 窗口。")?;
        // SAFETY: the child process just returned its verified HWND, while Tauri
        // owns the parent. SetParent does not change the window style itself.
        unsafe {
            let child = hwnd(handle);
            let style = GetWindowLongPtrW(child, GWL_STYLE);
            SetWindowLongPtrW(
                child,
                GWL_STYLE,
                (style & !(WS_POPUP.0 as isize)) | WS_CHILD.0 as isize | WS_CLIPSIBLINGS.0 as isize,
            );
            // The previous desktop parent is legitimately null.
            let _ = SetParent(child, Some(parent));
            if GetParent(child).ok() != Some(parent) {
                return Err("无法在 KWork 窗口内显示平台页面。".into());
            }
        }
        Ok(Self {
            child,
            input,
            handle,
            pid,
            _temporary: temporary,
        })
    }

    pub(super) async fn open(
        &mut self,
        page: Page,
        bounds: Bounds,
        grant: PlatformGrant,
        headers: BTreeMap<String, String>,
    ) -> Result<(), String> {
        self.send(json!({
            "id":OPEN_ID, "action":"open", "locale":"zh_CN",
            "page":match page { Page::Usage => "usage", Page::TopUp => "top-up" },
            "width":bounds.width.max(1.), "height":bounds.height.max(1.),
            "account":{
                "origin":grant.origin, "token":grant.token, "userId":grant.user_id,
                "embeddedPageDist":grant.embedded_page_dist, "requestHeaders":headers,
            },
        }))
        .await
    }

    pub(super) fn present<R: Runtime>(
        &self,
        owner: &Webview<R>,
        bounds: Bounds,
    ) -> Result<(), String> {
        let scale = owner
            .window()
            .scale_factor()
            .map_err(|_| "未能取得平台页面缩放。")?;
        self.place(bounds, scale, bounds.width >= 1. && bounds.height >= 1.)
    }

    fn place(&self, bounds: Bounds, scale: f64, visible: bool) -> Result<(), String> {
        if !owns_window(self.handle, self.pid) {
            return Err("用量与充值组件已关闭。".into());
        }
        let px = |value: f64| (value * scale).round() as i32;
        let visibility = if visible {
            SWP_SHOWWINDOW
        } else {
            SWP_HIDEWINDOW
        };
        // SAFETY: the handle still belongs to the owned helper process.
        unsafe {
            SetWindowPos(
                hwnd(self.handle),
                Some(HWND_TOP),
                px(bounds.x),
                px(bounds.y),
                px(bounds.width).max(1),
                px(bounds.height).max(1),
                SWP_FRAMECHANGED | SWP_NOACTIVATE | visibility,
            )
        }
        .map_err(|_| "未能调整平台页面布局。".into())
    }

    async fn send(&mut self, payload: Value) -> Result<(), String> {
        let mut bytes = serde_json::to_vec(&payload).map_err(|_| "平台页面请求无效。")?;
        bytes.push(b'\n');
        timeout(Duration::from_secs(3), self.input.write_all(&bytes))
            .await
            .map_err(|_| "平台页面没有响应。")?
            .map_err(|_| "无法向平台页面发送请求。".into())
    }

    pub(super) async fn close(mut self) -> Result<(), String> {
        let _ = self.place(
            Bounds {
                x: 0.,
                y: 0.,
                width: 1.,
                height: 1.,
            },
            1.,
            false,
        );
        if self.child.try_wait().ok().flatten().is_some() {
            return Ok(());
        }
        let _ = self.send(json!({"id":CLOSE_ID, "action":"close"})).await;
        match timeout(CLOSE_WAIT, self.child.wait()).await {
            Ok(Ok(status)) if status.success() => Ok(()),
            _ => {
                let _ = self.child.kill().await;
                Err("未能正常关闭平台页面。".into())
            }
        }
    }
}
