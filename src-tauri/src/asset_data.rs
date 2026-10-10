//! Image assets as `data:` URLs for the Runtime image control, and text assets
//! for the Studio preview (Settings › Assets).
//!
//! Read-only: uses the asset list and path helpers from assets.rs.
use crate::archive::Attachment;
use crate::manager::AppError;
use base64::Engine as _;

/// Largest image the Runtime inlines as a data URL.
pub const MAX_IMAGE_BYTES: u64 = 5 * 1024 * 1024;

/// Builds the data URL for an image asset, refusing other media types and oversized files.
pub fn image_data_url(
    asset: &Attachment,
    read: impl FnOnce() -> Result<Vec<u8>, AppError>,
) -> Result<String, AppError> {
    let media = asset.media_type.trim().to_ascii_lowercase();
    if !media.starts_with("image/") {
        return Err(AppError::new(
            "UNSUPPORTED_ASSET",
            format!("{} is not an image ({media})", asset.display_name),
        ));
    }
    if asset.size > MAX_IMAGE_BYTES {
        return Err(AppError::new(
            "ASSET_TOO_LARGE",
            format!(
                "{} is larger than {} MB",
                asset.display_name,
                MAX_IMAGE_BYTES / 1024 / 1024
            ),
        ));
    }
    let bytes = read()?;
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Err(AppError::new(
            "ASSET_TOO_LARGE",
            format!("{} is too large", asset.display_name),
        ));
    }
    Ok(format!(
        "data:{media};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

/// An image asset of the open document as a `data:<mime>;base64,…` URL (at most 5 MB).
#[tauri::command]
pub fn read_asset_data_url(window_label: String, id: String) -> Result<String, AppError> {
    let m = crate::manager()?;
    let asset = m
        .asset_list(&window_label)?
        .into_iter()
        .find(|a| a.id == id)
        .ok_or_else(|| AppError::new("ATTACHMENT_FAILURE", "Attachment not found"))?;
    image_data_url(&asset, || {
        std::fs::read(m.asset_path(&window_label, &id)?).map_err(|e| AppError::new("IO_ERROR", e))
    })
}

/// Largest text asset Studio previews.
pub const MAX_TEXT_BYTES: u64 = 2 * 1024 * 1024;

/// The text of a `text/*` asset, invalid UTF-8 replaced, refusing other media types and large files.
pub fn asset_text(
    asset: &Attachment,
    read: impl FnOnce() -> Result<Vec<u8>, AppError>,
) -> Result<String, AppError> {
    let media = asset.media_type.trim().to_ascii_lowercase();
    if !media.starts_with("text/") {
        return Err(AppError::new(
            "UNSUPPORTED_ASSET",
            format!("{} is not text ({media})", asset.display_name),
        ));
    }
    if asset.size > MAX_TEXT_BYTES {
        return Err(AppError::new(
            "ASSET_TOO_LARGE",
            format!(
                "{} is larger than {} MB; export it to read it",
                asset.display_name,
                MAX_TEXT_BYTES / 1024 / 1024
            ),
        ));
    }
    Ok(String::from_utf8_lossy(&read()?).into_owned())
}

/// A text asset of the open document, for the Studio preview (developers only).
#[tauri::command]
pub fn read_asset_text(window_label: String, id: String) -> Result<String, AppError> {
    crate::authz::require_unrestricted(&window_label, "read assets")?;
    let m = crate::manager()?;
    let asset = m
        .asset_list(&window_label)?
        .into_iter()
        .find(|a| a.id == id)
        .ok_or_else(|| AppError::new("ATTACHMENT_FAILURE", "Attachment not found"))?;
    asset_text(&asset, || {
        std::fs::read(m.asset_path(&window_label, &id)?).map_err(|e| AppError::new("IO_ERROR", e))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(media: &str, size: u64) -> Attachment {
        Attachment {
            id: "a".into(),
            display_name: "logo.png".into(),
            media_type: media.into(),
            checksum: String::new(),
            size,
            created_at: String::new(),
            updated_at: String::new(),
            contents: vec![],
        }
    }

    #[test]
    fn asset_data_encodes_images() {
        let url = image_data_url(&asset("image/png", 3), || Ok(vec![1, 2, 3])).unwrap();
        assert_eq!(url, "data:image/png;base64,AQID");
    }

    #[test]
    fn asset_data_refuses_other_types_and_large_files_without_reading() {
        let never = || -> Result<Vec<u8>, AppError> { panic!("must not read") };
        let err = image_data_url(&asset("application/pdf", 3), never).unwrap_err();
        assert_eq!(err.code, "UNSUPPORTED_ASSET");
        let err = image_data_url(&asset("image/png", MAX_IMAGE_BYTES + 1), never).unwrap_err();
        assert_eq!(err.code, "ASSET_TOO_LARGE");
    }

    #[test]
    fn text_assets_are_read_and_others_refused() {
        let text = asset("text/plain; charset=utf-8", 3);
        assert_eq!(
            asset_text(&text, || Ok(b"ab\xff".to_vec())).unwrap(),
            "ab\u{fffd}"
        );
        let err = asset_text(&asset("image/png", 3), || Ok(vec![])).unwrap_err();
        assert_eq!(err.code, "UNSUPPORTED_ASSET");
        let err = asset_text(&asset("text/plain", MAX_TEXT_BYTES + 1), || Ok(vec![])).unwrap_err();
        assert_eq!(err.code, "ASSET_TOO_LARGE");
    }
}
