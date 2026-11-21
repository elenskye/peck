use tauri::WebviewWindow;

/// Show a native save panel attached to `window`, then write `bytes` to the chosen path.
///
/// Returns `Ok(None)` when the user cancels the panel.
pub async fn save_export(
    window: &WebviewWindow,
    title: &str,
    file_name: &str,
    filter_name: &str,
    ext: &str,
    bytes: Vec<u8>,
    error_prefix: &str,
) -> Result<Option<String>, String> {
    let handle = rfd::AsyncFileDialog::new()
        .set_parent(window)
        .set_title(title)
        .set_file_name(file_name)
        .add_filter(filter_name, &[ext])
        .save_file()
        .await;
    let Some(handle) = handle else {
        return Ok(None);
    };
    let path = handle.path().to_path_buf();
    std::fs::write(&path, bytes).map_err(|e| format!("{error_prefix}：{e}"))?;
    Ok(Some(path.to_string_lossy().into_owned()))
}
