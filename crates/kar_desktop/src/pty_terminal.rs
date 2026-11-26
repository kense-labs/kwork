//! VT100 display for a terminal owned by the Kar workbench.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::engine::Engine;
use anyhow::{Context as _, Result};
use axum::http::Method;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use gpui::{Hsla, rgb};
use kar::chat::ChatStore;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq)]
pub struct PtyStyledSpan {
    pub text: String,
    pub fg: Option<Hsla>,
    pub bold: bool,
}

#[derive(Clone, Debug, Default)]
pub struct PtyRenderSnapshot {
    pub lines: Vec<Vec<PtyStyledSpan>>,
    pub cursor_row: u16,
    pub cursor_col: u16,
    pub rows: u16,
    pub cols: u16,
    pub running: bool,
    pub detected_ports: Vec<u16>,
    pub plain_tail: String,
    pub has_error_hint: bool,
}

struct SharedPtyScreen {
    parser: vt100::Parser,
    detected_ports: BTreeSet<u16>,
    recent_raw: String,
    exited: bool,
}

enum TerminalInput {
    Bytes(Vec<u8>),
    Resize(u16, u16),
    Clear,
    Close,
}

#[derive(Deserialize)]
struct OutputPage {
    data_base64: String,
    next: u64,
    missed: bool,
    terminal: TerminalMeta,
}
#[derive(Deserialize)]
struct TerminalMeta {
    status: TerminalStatus,
}
#[derive(Deserialize)]
struct TerminalStatus {
    state: String,
}

pub struct InteractivePtySession {
    input: mpsc::Sender<TerminalInput>,
    screen: Arc<Mutex<SharedPtyScreen>>,
    pub cwd: PathBuf,
}

impl SharedPtyScreen {
    fn reset(&mut self) {
        let (rows, cols) = self.parser.screen().size();
        self.parser = vt100::Parser::new(rows, cols, 1200);
        self.recent_raw.clear();
        self.detected_ports.clear();
    }

    fn append(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
        let text = String::from_utf8_lossy(bytes);
        self.recent_raw.push_str(&text);
        self.detected_ports
            .extend(extract_localhost_ports(&self.recent_raw));
        if self.recent_raw.len() > 8192 {
            let boundary = self
                .recent_raw
                .ceil_char_boundary(self.recent_raw.len() - 4096);
            self.recent_raw.drain(..boundary);
        }
    }
}

fn drive_terminal(
    chat_dir: &Path,
    cwd: &Path,
    rows: u16,
    cols: u16,
    input: mpsc::Receiver<TerminalInput>,
    screen: &Mutex<SharedPtyScreen>,
) -> Result<()> {
    let engine = Engine::open(ChatStore::open(chat_dir)?, cwd)?;
    let id = Uuid::new_v4();
    let collection = format!("/projects/{}/terminals", engine.project_id());
    let terminal = format!("{collection}/{id}");
    let call = |method, path: &str, payload| {
        futures::executor::block_on(engine.request(method, path, payload))
    };
    call(
        Method::POST,
        &collection,
        Some(json!({"id":id, "rows":rows, "cols":cols})),
    )?;
    let result = (|| {
        let mut cursor = 0;
        loop {
            match input.recv_timeout(Duration::from_millis(65)) {
                Ok(TerminalInput::Close) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Ok(TerminalInput::Bytes(bytes)) => {
                    let body = json!({"id":Uuid::new_v4(), "data_base64":STANDARD.encode(bytes)});
                    call(Method::POST, &format!("{terminal}/input"), Some(body))?;
                }
                Ok(TerminalInput::Resize(rows, cols)) => {
                    call(
                        Method::PUT,
                        &format!("{terminal}/size"),
                        Some(json!({"rows":rows, "cols":cols})),
                    )?;
                    screen
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .parser
                        .screen_mut()
                        .set_size(rows, cols);
                }
                Ok(TerminalInput::Clear) => {
                    let receipt = call(Method::POST, &format!("{terminal}/clear"), None)?;
                    cursor = receipt["cursor"].as_u64().context("missing clear cursor")?;
                    screen.lock().unwrap_or_else(|e| e.into_inner()).reset();
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            let page: OutputPage = serde_json::from_value(call(
                Method::GET,
                &format!("{terminal}/output?after={cursor}"),
                None,
            )?)?;
            cursor = page.next;
            let bytes = STANDARD.decode(page.data_base64)?;
            let mut screen = screen.lock().unwrap_or_else(|e| e.into_inner());
            if page.missed {
                screen.reset();
            }
            screen.append(&bytes);
            if !matches!(
                page.terminal.status.state.as_str(),
                "starting" | "running" | "closing"
            ) {
                break;
            }
        }
        Ok(())
    })();
    let _ = call(Method::DELETE, &terminal, None);
    result
}

impl InteractivePtySession {
    pub fn spawn(chat_dir: &Path, cwd: &Path, rows: u16, cols: u16) -> Result<Arc<Self>> {
        let rows = rows.clamp(8, 120);
        let cols = cols.clamp(32, 300);
        let screen = Arc::new(Mutex::new(SharedPtyScreen {
            parser: vt100::Parser::new(rows, cols, 1200),
            detected_ports: BTreeSet::new(),
            recent_raw: String::new(),
            exited: false,
        }));
        let (tx, rx) = mpsc::channel();
        let session = Arc::new(Self {
            input: tx,
            screen: screen.clone(),
            cwd: cwd.into(),
        });
        let chat_dir = chat_dir.to_owned();
        let cwd = cwd.to_owned();
        std::thread::Builder::new()
            .name("kar-terminal-viewer".into())
            .spawn(move || {
                let result = drive_terminal(&chat_dir, &cwd, rows, cols, rx, &screen);
                let mut screen = screen.lock().unwrap_or_else(|e| e.into_inner());
                if let Err(error) = result {
                    screen.append(format!("\r\nKar: {error}\r\n").as_bytes());
                }
                screen.exited = true;
            })?;
        Ok(session)
    }

    pub fn write_bytes(&self, bytes: &[u8]) -> Result<()> {
        self.input
            .send(TerminalInput::Bytes(bytes.to_owned()))
            .context("终端已关闭")
    }

    pub fn send_line(&self, command: &str) -> Result<()> {
        let mut payload = command.as_bytes().to_vec();
        payload.push(b'\r');
        self.write_bytes(&payload)
    }

    pub fn send_interrupt(&self) -> Result<()> {
        self.write_bytes(&[0x03])
    }

    pub fn clear_screen(&self) -> Result<()> {
        self.input.send(TerminalInput::Clear).context("终端已关闭")
    }

    pub fn resize(&self, rows: u16, cols: u16) {
        let _ = self.input.send(TerminalInput::Resize(
            rows.clamp(8, 120),
            cols.clamp(32, 300),
        ));
    }

    pub fn is_alive(&self) -> bool {
        self.screen.lock().is_ok_and(|s| !s.exited)
    }

    pub fn terminate(&self) {
        let _ = self.input.send(TerminalInput::Close);
    }

    pub fn snapshot(&self, dark_theme: bool) -> PtyRenderSnapshot {
        let alive = self.is_alive();
        let Ok(guard) = self.screen.lock() else {
            return PtyRenderSnapshot::default();
        };
        let scr = guard.parser.screen();
        let (rows, cols) = scr.size();
        let (cursor_row, cursor_col) = scr.cursor_position();

        let mut lines: Vec<Vec<PtyStyledSpan>> = Vec::with_capacity(rows as usize);
        let mut non_empty_last = 0usize;
        for r in 0..rows {
            let mut row_spans: Vec<PtyStyledSpan> = Vec::new();
            let mut cur_text = String::new();
            let mut cur_fg: Option<Hsla> = None;
            let mut cur_bold = false;

            for c in 0..cols {
                let Some(cell) = scr.cell(r, c) else {
                    continue;
                };
                if cell.is_wide_continuation() {
                    continue;
                }
                let ch = cell.contents();
                let rendered = if ch.is_empty() { " " } else { ch };
                let fg = vt100_color_to_hsla(cell.fgcolor(), dark_theme);
                let bold = cell.bold();

                if cur_text.is_empty() {
                    cur_fg = fg;
                    cur_bold = bold;
                    cur_text.push_str(rendered);
                } else if cur_fg == fg && cur_bold == bold {
                    cur_text.push_str(rendered);
                } else {
                    row_spans.push(PtyStyledSpan {
                        text: std::mem::take(&mut cur_text),
                        fg: cur_fg,
                        bold: cur_bold,
                    });
                    cur_fg = fg;
                    cur_bold = bold;
                    cur_text.push_str(rendered);
                }
            }
            if !cur_text.is_empty() {
                let trimmed_len = cur_text.trim_end_matches(' ').len();
                cur_text.truncate(trimmed_len);
                if !cur_text.is_empty() {
                    row_spans.push(PtyStyledSpan {
                        text: cur_text,
                        fg: cur_fg,
                        bold: cur_bold,
                    });
                }
            } else if let Some(last) = row_spans.last_mut() {
                let trimmed_len = last.text.trim_end_matches(' ').len();
                last.text.truncate(trimmed_len);
                if last.text.is_empty() {
                    row_spans.pop();
                }
            }
            if !row_spans.is_empty() || r == cursor_row {
                non_empty_last = (r as usize) + 1;
            }
            lines.push(row_spans);
        }
        lines.truncate(non_empty_last.max(1));

        let plain = scr.contents();
        let has_error_hint = plain.contains("error[")
            || plain.contains("Error:")
            || plain.contains("FAILED")
            || plain.contains("command not found")
            || plain.contains("panicked at");

        PtyRenderSnapshot {
            lines,
            cursor_row,
            cursor_col,
            rows,
            cols,
            running: alive,
            detected_ports: guard.detected_ports.iter().copied().collect(),
            plain_tail: plain,
            has_error_hint,
        }
    }
}

impl Drop for InteractivePtySession {
    fn drop(&mut self) {
        self.terminate();
    }
}

/// Convert a GPUI `KeyDownEvent` into VT100 terminal input bytes when the PTY input surface is focused.
pub fn keystroke_to_pty_bytes(ks: &gpui::Keystroke) -> Option<Vec<u8>> {
    let m = ks.modifiers;
    if m.platform {
        return None;
    }
    let k = ks.key.as_str();
    if m.control && k.len() == 1 {
        let ch = k.as_bytes()[0].to_ascii_lowercase();
        if ch.is_ascii_lowercase() {
            return Some(vec![ch - b'a' + 1]);
        }
    }
    match k {
        "enter" => Some(vec![b'\r']),
        "backspace" => Some(vec![0x7f]),
        "tab" => Some(vec![b'\t']),
        "escape" => Some(vec![0x1b]),
        "space" => Some(vec![b' ']),
        "up" => Some(b"\x1b[A".to_vec()),
        "down" => Some(b"\x1b[B".to_vec()),
        "right" => Some(b"\x1b[C".to_vec()),
        "left" => Some(b"\x1b[D".to_vec()),
        "home" => Some(b"\x1b[H".to_vec()),
        "end" => Some(b"\x1b[F".to_vec()),
        "delete" => Some(b"\x1b[3~".to_vec()),
        _ => {
            if let Some(ref kc) = ks.key_char
                && !kc.is_empty()
            {
                Some(kc.as_bytes().to_vec())
            } else if k.len() == 1 {
                let s = if m.shift {
                    k.to_ascii_uppercase()
                } else {
                    k.to_owned()
                };
                Some(s.into_bytes())
            } else {
                None
            }
        }
    }
}

pub fn extract_localhost_ports(text: &str) -> Vec<u16> {
    let mut out = Vec::new();
    for needle in ["localhost:", "127.0.0.1:", "0.0.0.0:", "[::1]:"] {
        let mut rest = text;
        while let Some(idx) = rest.find(needle) {
            let after = &rest[idx + needle.len()..];
            let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(port) = digits.parse::<u16>()
                && port >= 1024
                && !out.contains(&port)
            {
                out.push(port);
            }
            rest = &after[digits.len()..];
        }
    }
    out
}

fn vt100_color_to_hsla(color: vt100::Color, dark: bool) -> Option<Hsla> {
    match color {
        vt100::Color::Default => None,
        vt100::Color::Idx(idx) => {
            let hex = match idx {
                0 | 8 => {
                    if dark {
                        0x52525b
                    } else {
                        0x27272a
                    }
                }
                1 | 9 => {
                    if dark {
                        0xf87171
                    } else {
                        0xdc2626
                    }
                }
                2 | 10 => {
                    if dark {
                        0x4ade80
                    } else {
                        0x16a34a
                    }
                }
                3 | 11 => {
                    if dark {
                        0xfacc15
                    } else {
                        0xca8a04
                    }
                }
                4 | 12 => {
                    if dark {
                        0x60a5fa
                    } else {
                        0x2563eb
                    }
                }
                5 | 13 => {
                    if dark {
                        0xc084fc
                    } else {
                        0x9333ea
                    }
                }
                6 | 14 => {
                    if dark {
                        0x2dd4bf
                    } else {
                        0x0d9488
                    }
                }
                7 | 15 => {
                    if dark {
                        0xf4f4f5
                    } else {
                        0x18181b
                    }
                }
                16..=231 => {
                    let n = idx - 16;
                    let r = (n / 36) * 51;
                    let g = ((n % 36) / 6) * 51;
                    let b = (n % 6) * 51;
                    ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
                }
                232..=255 => {
                    let v = 8 + (idx - 232) as u32 * 10;
                    (v << 16) | (v << 8) | v
                }
            };
            Some(rgb(hex).into())
        }
        vt100::Color::Rgb(r, g, b) => {
            let hex = ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
            Some(rgb(hex).into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_localhost_ports_from_dev_server_output() {
        let log = "VITE v6.0 ready in 140ms\n  ➜  Local:   http://localhost:5173/\n  ➜  API: http://127.0.0.1:8787/health";
        assert_eq!(extract_localhost_ports(log), vec![5173, 8787]);
    }
}
