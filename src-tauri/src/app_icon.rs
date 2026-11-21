//! App 图标：J2（默认，打包用的）、A3。设置 → 外观里切了，Dock 里马上换；访达里的图标还是打包时的 J2。
//! 选哪个存在界面设置里，App 打开时界面再设一次。

/// 换 Dock 里的图标。`icon` 是 `j2` 或 `a3`。不是 macOS 什么都不做。
#[tauri::command]
pub async fn app_icon_set(icon: String, app: tauri::AppHandle) -> Result<(), String> {
    let png = png_for(&icon)?;
    #[cfg(target_os = "macos")]
    app.run_on_main_thread(move || set_dock_icon(png)).map_err(|e| e.to_string())?;
    #[cfg(not(target_os = "macos"))]
    let _ = (app, png);
    Ok(())
}

fn png_for(icon: &str) -> Result<&'static [u8], String> {
    match icon {
        "j2" => Ok(include_bytes!("../icons/j2.png")),
        "a3" => Ok(include_bytes!("../icons/a3.png")),
        _ => Err("App 图标只有 J2、A3 两种。".into()),
    }
}

#[cfg(target_os = "macos")]
fn set_dock_icon(png: &'static [u8]) {
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;
    let Some(mtm) = MainThreadMarker::new() else { return };
    let data = NSData::with_bytes(png);
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else { return };
    let app = NSApplication::sharedApplication(mtm);
    // SAFETY：传的是一张有效的图（不是 None），在主线程上设。
    unsafe { app.setApplicationIconImage(Some(&image)) };
}

#[cfg(test)]
mod tests {
    #[test]
    fn two_icons_and_both_are_png() {
        for icon in ["j2", "a3"] {
            assert!(super::png_for(icon).unwrap().starts_with(b"\x89PNG"), "{icon}");
        }
        assert!(super::png_for("peck").is_err());
    }
}
