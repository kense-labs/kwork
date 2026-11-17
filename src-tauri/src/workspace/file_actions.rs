//! Main-window original-file actions for the official Markdown context menus.
use serde_json::{Value, json};
use std::io::Write;
use std::path::PathBuf;
use tauri::Manager;

fn owner(webview: &tauri::Webview) -> Result<(), String> {
    if webview.label() == "main" {
        Ok(())
    } else {
        Err("文件操作只能由主界面发起。".into())
    }
}

#[tauri::command]
pub(crate) async fn document_file(
    webview: tauri::Webview,
    path: String,
    read: bool,
) -> Result<Value, String> {
    owner(&webview)?;
    tauri::async_runtime::spawn_blocking(move || {
        if !PathBuf::from(&path).is_absolute() {
            return Err("文件路径必须是绝对路径。".into());
        }
        let path =
            super::document::document_path("", &path).map_err(|error| format!("{error:?}"))?;
        let file =
            super::document::open_regular_file(&path).map_err(|error| format!("{error:?}"))?;
        let metadata = file.metadata().map_err(|error| error.to_string())?;
        if read {
            let data =
                super::document::read_original(&path).map_err(|error| format!("{error:?}"))?;
            Ok(json!({"absolutePath":path,"data":data}))
        } else {
            Ok(json!({"absolutePath":path,"size":metadata.len()}))
        }
    })
    .await
    .map_err(|error| error.to_string())?
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !matches!(name, "." | "..")
        && name.len() <= 255
        && !name.chars().any(|ch| {
            ch.is_ascii_control()
                || matches!(ch, '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*')
        })
}

#[tauri::command]
pub(crate) async fn save_file_bytes(
    webview: tauri::Webview,
    name: String,
    data: Vec<u8>,
    mode: String,
) -> Result<Value, String> {
    owner(&webview)?;
    if !valid_name(&name)
        || data.len() > 64 * 1024 * 1024
        || !matches!(mode.as_str(), "download" | "saveAs")
    {
        return Err("文件保存请求无效。".into());
    }
    let download = mode == "download";
    let destination = if download {
        Some(
            webview
                .app_handle()
                .path()
                .download_dir()
                .map_err(|error| error.to_string())?
                .join(name),
        )
    } else {
        #[cfg(target_os = "macos")]
        {
            crate::history::export::archive::macos::choose_destination(webview, &name).await?
        }
        #[cfg(not(target_os = "macos"))]
        {
            return Err("当前平台不支持另存为。".into());
        }
    };
    let Some(destination) = destination else {
        return Ok(json!({"status":"cancelled"}));
    };
    tauri::async_runtime::spawn_blocking(move || {
        let directory = destination.parent().ok_or("文件保存位置无效。")?;
        let mut pending =
            tempfile::NamedTempFile::new_in(directory).map_err(|error| error.to_string())?;
        pending
            .write_all(&data)
            .map_err(|error| error.to_string())?;
        pending
            .as_file()
            .sync_all()
            .map_err(|error| error.to_string())?;
        let path =
            crate::history::export::archive::persist_download(pending, &destination, !download)?;
        Ok(json!({"status":"saved","path":path}))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub(crate) async fn copy_image_bytes(webview: tauri::Webview, data: Vec<u8>) -> Result<(), String> {
    owner(&webview)?;
    if data.len() > 64 * 1024 * 1024 {
        return Err("图片过大。".into());
    }
    #[cfg(target_os = "macos")]
    {
        let png = tauri::async_runtime::spawn_blocking(move || {
            crate::history::export::archive::images::clipboard_png(data)
        })
        .await
        .map_err(|error| error.to_string())??;
        crate::history::export::archive::macos::copy_image(webview, png).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("当前平台不支持原生图片复制。".into())
    }
}

#[cfg(target_os = "macos")]
#[tauri::command]
pub(crate) async fn document_applications(
    webview: tauri::Webview,
    path: String,
) -> Result<Value, String> {
    owner(&webview)?;
    let (send, receive) = tokio::sync::oneshot::channel();
    webview
        .window()
        .run_on_main_thread(move || {
            use objc2_app_kit::{NSWorkspace, NSBitmapImageRep, NSBitmapImageFileType};
            use objc2_foundation::{NSDictionary, NSFileManager, NSString, NSURL};
            let workspace = NSWorkspace::sharedWorkspace();
            let url = NSURL::fileURLWithPath(&NSString::from_str(&path));
            let application = workspace.URLForApplicationToOpenURL(&url);
            let result = application.and_then(|app| app.path()).map(|path| {
                let name = NSFileManager::defaultManager().displayNameAtPath(&path).to_string();
                let icon = workspace.iconForFile(&path).TIFFRepresentation()
                    .and_then(|data| NSBitmapImageRep::imageRepWithData(&data))
                    // SAFETY: an empty dictionary has no incorrectly typed image properties.
                    .and_then(|image| unsafe { image.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) })
                    .map(|data| format!("data:image/png;base64,{}", crate::history::base64_encode(&data.to_vec())));
                let mut app = json!({"id":path.to_string(),"name":name.trim_end_matches(".app"),"default":true});
                if let Some(icon) = icon { app["icon"] = json!(icon); }
                app
            });
            let _ = send.send(result.into_iter().collect::<Vec<_>>());
        })
        .map_err(|error| error.to_string())?;
    receive
        .await
        .map(|apps| json!(apps))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn open_document_application(
    webview: tauri::Webview,
    path: String,
    reveal: bool,
    application: Option<String>,
) -> Result<(), String> {
    owner(&webview)?;
    #[cfg(target_os = "macos")]
    if let Some(application) = application.filter(|_| !reveal) {
        let path = PathBuf::from(path);
        let app = PathBuf::from(application);
        if !path.is_absolute()
            || !app.is_absolute()
            || app.extension().is_none_or(|ext| ext != "app")
        {
            return Err("打开请求无效。".into());
        }
        return tauri::async_runtime::spawn_blocking(move || {
            let status = std::process::Command::new("/usr/bin/open")
                .arg("-a")
                .arg(app)
                .arg("--")
                .arg(path)
                .status()
                .map_err(|error| error.to_string())?;
            if status.success() {
                Ok(())
            } else {
                Err("打开失败，请重试。".into())
            }
        })
        .await
        .map_err(|error| error.to_string())?;
    }
    super::open_path(path, reveal).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_read_preserves_binary_and_rejects_oversized_or_non_regular_files() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("source.bin");
        let bytes = [0, 255, 13, 10, 128];
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(super::super::document::read_original(&path).unwrap(), bytes);
        assert!(super::super::document::read_original(root.path()).is_err());
        #[cfg(unix)]
        {
            let link = root.path().join("link");
            std::os::unix::fs::symlink(&path, &link).unwrap();
            assert!(super::super::document::read_original(&link).is_err());
            assert!(
                super::super::document::read_original(std::path::Path::new("/dev/zero")).is_err()
            );
        }
        std::fs::File::create(&path)
            .unwrap()
            .set_len(32 * 1024 * 1024 + 1)
            .unwrap();
        assert!(matches!(
            super::super::document::read_original(&path),
            Err(super::super::document::ReadFailure::TooLarge)
        ));
    }

    #[test]
    fn save_names_cannot_select_a_directory_or_inject_path_components() {
        for name in [
            "",
            ".",
            "..",
            "../source.png",
            "/source.png",
            "a\\b.png",
            "x\0.png",
        ] {
            assert!(!valid_name(name));
        }
        assert!(valid_name("截图 #1.png"));
    }
}
