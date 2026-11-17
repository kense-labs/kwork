//! Native fallback for WebKit image clipboard writes, including AX menu actions.

#[tauri::command]
pub(crate) async fn copy_image_attachment(
    webview: tauri::Webview,
    attachment_id: String,
    bytes: u64,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        if webview.label() != "main" || bytes > 32 * 1024 * 1024 {
            return Err("无法复制图片。".into());
        }
        let digest = attachment_id
            .strip_prefix("sha256:")
            .filter(|digest| {
                digest.len() == 64
                    && digest
                        .bytes()
                        .all(|ch| ch.is_ascii_digit() || (b'a'..=b'f').contains(&ch))
            })
            .ok_or("附件标识无效。")?
            .to_owned();
        let png = tauri::async_runtime::spawn_blocking(move || {
            let root = super::dsh_home()
                .join("attachments/v1/objects")
                .canonicalize()
                .map_err(|error| error.to_string())?;
            let source = root
                .join(&digest[..2])
                .join(&digest)
                .canonicalize()
                .map_err(|error| error.to_string())?;
            if !source.starts_with(&root) {
                return Err("附件路径超出存储范围。".into());
            }
            let mut file = std::fs::File::open(source).map_err(|error| error.to_string())?;
            let mut original = Vec::new();
            super::copy_verified(&mut file, &mut original, bytes, &digest)?;
            clipboard_png(original)
        })
        .await
        .map_err(|error| error.to_string())??;
        super::macos::copy_image(webview, png).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (webview, attachment_id, bytes);
        Err("原生图片复制在此平台不可用。".into())
    }
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn clipboard_png(original: Vec<u8>) -> Result<Vec<u8>, String> {
    use image::{ImageFormat, ImageReader};
    use std::io::Cursor;
    let reader = ImageReader::new(Cursor::new(&original))
        .with_guessed_format()
        .map_err(|error| error.to_string())?;
    let format = reader.format();
    let decoded = reader.decode().map_err(|error| error.to_string())?;
    // Match upstream: PNG keeps its bytes; other formats become a pasteable PNG.
    if format == Some(ImageFormat::Png) {
        return Ok(original);
    }
    let mut png = Cursor::new(Vec::new());
    decoded
        .write_to(&mut png, ImageFormat::Png)
        .map_err(|error| error.to_string())?;
    Ok(png.into_inner())
}

#[cfg(test)]
mod tests {
    use super::clipboard_png;
    use image::{DynamicImage, ImageFormat};
    use std::io::Cursor;

    #[test]
    fn clipboard_preserves_png_and_converts_other_images() {
        for format in [ImageFormat::Png, ImageFormat::Jpeg] {
            let mut encoded = Cursor::new(Vec::new());
            DynamicImage::new_rgb8(3, 2)
                .write_to(&mut encoded, format)
                .unwrap();
            let original = encoded.into_inner();
            let png = clipboard_png(original.clone()).unwrap();
            assert_eq!(image::guess_format(&png).unwrap(), ImageFormat::Png);
            let image = image::load_from_memory(&png).unwrap();
            assert_eq!((image.width(), image.height()), (3, 2));
            if format == ImageFormat::Png {
                assert_eq!(png, original);
            }
        }
    }

    #[test]
    fn invalid_image_never_reaches_the_clipboard() {
        assert!(clipboard_png(b"not an image".to_vec()).is_err());
    }
}
