//! `screenshot_app`（v0.8 Phase 12）：截一个 App 的窗口给模型看（比如正在开发的 App、模拟器、浏览器里的页面）。
//! 只在 Mac 上、只给能看图的模型。用系统自带的 `screencapture -l <窗口号>`；窗口号用 JXA 调
//! `CGWindowListCopyWindowInfo` 找（App 名字对得上、在最前面的那一个普通窗口）。截图要「屏幕录制」的权限：
//! 没给时 macOS 会弹窗问，截出来的只有桌面——这时照实说。图缩到最长边 1568 像素（`sips`），存进这个任务的
//! 输出目录（界面能看），再作为图交给模型。

use super::ToolContext;
use crate::gateway::ImageInput;
use serde_json::Value;

/// 一个窗口。
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    pub id: u64,
    pub owner: String,
    pub title: String,
}

/// 从窗口列表里挑：App 名字（不分大小写，包含就算）对得上；给了窗口标题再按标题挑；都在最前面的优先（列表本来就是从前往后）。
pub fn pick<'a>(windows: &'a [Window], app: &str, title: Option<&str>) -> Option<&'a Window> {
    let app = app.trim().to_lowercase();
    let title = title.map(|t| t.trim().to_lowercase()).filter(|t| !t.is_empty());
    windows
        .iter()
        .filter(|w| w.owner.to_lowercase().contains(&app))
        .find(|w| title.as_ref().is_none_or(|t| w.title.to_lowercase().contains(t)))
}

/// 截一个 App 的窗口：返回给模型的一句话和图。
pub fn screenshot(ctx: &ToolContext, input: &Value) -> Result<(String, ImageInput), String> {
    let app = input.get("app").and_then(Value::as_str).map(str::trim).filter(|a| !a.is_empty()).ok_or("缺 app：要截哪个 App 的窗口（Dock 里的名字）。")?;
    let title = input.get("window").and_then(Value::as_str);
    capture(ctx, app, title)
}

#[cfg(target_os = "macos")]
fn windows() -> Result<Vec<Window>, String> {
    // 在屏幕上的普通窗口（layer 0），从前往后。
    const JXA: &str = r#"ObjC.import('CoreGraphics');
var list = ObjC.deepUnwrap(ObjC.castRefToObject($.CGWindowListCopyWindowInfo($.kCGWindowListOptionOnScreenOnly | $.kCGWindowListExcludeDesktopElements, $.kCGNullWindowID)));
var out = [];
for (var i = 0; i < list.length; i++) { var w = list[i]; if (w.kCGWindowLayer === 0) out.push({ id: w.kCGWindowNumber, owner: w.kCGWindowOwnerName || '', title: w.kCGWindowName || '' }); }
JSON.stringify(out);"#;
    let out = std::process::Command::new("/usr/bin/osascript")
        .args(["-l", "JavaScript", "-e", JXA])
        .output()
        .map_err(|e| format!("读不到窗口列表：{e}"))?;
    if !out.status.success() {
        return Err(format!("读不到窗口列表：{}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let list: Vec<Value> = serde_json::from_slice(&out.stdout).map_err(|_| "窗口列表看不懂。")?;
    Ok(list
        .into_iter()
        .filter_map(|w| {
            Some(Window {
                id: w.get("id")?.as_u64()?,
                owner: w.get("owner").and_then(Value::as_str).unwrap_or("").to_string(),
                title: w.get("title").and_then(Value::as_str).unwrap_or("").to_string(),
            })
        })
        .collect())
}

#[cfg(target_os = "macos")]
fn capture(ctx: &ToolContext, app: &str, title: Option<&str>) -> Result<(String, ImageInput), String> {
    let all = windows()?;
    let Some(w) = pick(&all, app, title) else {
        let mut owners: Vec<&str> = all.iter().map(|w| w.owner.as_str()).filter(|o| !o.is_empty()).collect();
        owners.sort();
        owners.dedup();
        return Err(format!("屏幕上没有「{app}」的窗口（它要开着、没最小化）。现在开着的：{}。", owners.join("、")));
    };
    std::fs::create_dir_all(&ctx.spill_dir).map_err(|e| format!("存不了截图：{e}"))?;
    let n = ctx.spill_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    let path = ctx.spill_dir.join(format!("{n}-shot.png"));
    let shot = std::process::Command::new("/usr/sbin/screencapture")
        .args(["-x", "-o", "-t", "png", "-l", &w.id.to_string()])
        .arg(&path)
        .output()
        .map_err(|e| format!("截不了图：{e}"))?;
    if !shot.status.success() || !path.exists() {
        return Err("截不了图：Peck 可能还没有「屏幕录制」的权限（系统设置 → 隐私与安全性 → 屏幕录制）。".into());
    }
    let _ = std::process::Command::new("/usr/bin/sips").args(["-Z", "1568"]).arg(&path).output();
    let bytes = std::fs::read(&path).map_err(|e| format!("读不了截图：{e}"))?;
    let data = crate::files::base64(&bytes);
    let label = if w.title.is_empty() { w.owner.clone() } else { format!("{} · {}", w.owner, w.title) };
    Ok((
        format!("截了「{label}」的窗口（{} KB），图在下面。截图存在 {}。没给屏幕录制权限时截出来的只有桌面背景，看到这样就请站主去系统设置里给 Peck 权限。", bytes.len() / 1024, path.display()),
        ImageInput { media_type: "image/png".into(), data },
    ))
}

#[cfg(not(target_os = "macos"))]
fn capture(_ctx: &ToolContext, _app: &str, _title: Option<&str>) -> Result<(String, ImageInput), String> {
    Err("只有 Mac 上能截窗口。".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_front_window_of_the_app_and_by_title() {
        let ws = vec![
            Window { id: 3, owner: "Finder".into(), title: "下载".into() },
            Window { id: 7, owner: "Peck Dev".into(), title: "设置".into() },
            Window { id: 9, owner: "Peck Dev".into(), title: "Peck".into() },
        ];
        assert_eq!(pick(&ws, "peck dev", None).unwrap().id, 7);
        assert_eq!(pick(&ws, "Peck", Some("peck")).unwrap().id, 9);
        assert!(pick(&ws, "Simulator", None).is_none());
    }
}
