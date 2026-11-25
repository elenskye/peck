//! 菜单栏小图标：空闲、在跑（三颗点轮流亮，每帧 300ms）、等你。
//! 模板图在 `icons/menubar/`（`scripts/menubar-icons.mjs` 从 `src/vendor/peck/marks/menubar/` 出的 @2x PNG，
//! 裁掉四周的空、一格 1.5pt；托盘会把图片高度压成 18pt）。
//!
//! **托盘的东西只在主线程上碰**（v1.2.0）：tauri 的托盘对象里是不带锁的 `Rc`，以前动画线程和异步命令在后台线程上
//! 取、换图、放掉它，计数被并发改乱、提前归零，在后台线程上移除了菜单栏图标，macOS 当场把 App 杀了（v1.2.0 真跑时
//! 发新任务那一下崩过）。现在动画线程只计时，换图交给 `run_on_main_thread`；对外的两个函数也在主线程上做。

use std::sync::Mutex;
use std::time::Duration;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

const TRAY_ID: &str = "peck";
const IDLE: &[u8] = include_bytes!("../icons/menubar/idle.png");
const WAIT: &[u8] = include_bytes!("../icons/menubar/wait.png");
const RUN: [&[u8]; 3] = [
    include_bytes!("../icons/menubar/run-1.png"),
    include_bytes!("../icons/menubar/run-2.png"),
    include_bytes!("../icons/menubar/run-3.png"),
];
const FRAME: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    Idle,
    Running,
    Waiting,
}

impl Activity {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "idle" => Some(Self::Idle),
            "running" => Some(Self::Running),
            "waiting" => Some(Self::Waiting),
            _ => None,
        }
    }
}

/// 现在的状态和一个代数：状态每变一次代数加一，旧的动画线程看到代数变了就停。
#[derive(Default)]
pub struct TrayState {
    current: Mutex<(Option<Activity>, u64)>,
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// 只在主线程上调。
fn set_icon(app: &AppHandle, bytes: &[u8]) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    if let Ok(image) = Image::from_bytes(bytes) {
        let _ = tray.set_icon(Some(image));
        // 换图以后模板标记会丢，每次补上，系统才会按菜单栏明暗上色。
        let _ = tray.set_icon_as_template(true);
    }
}

pub fn init(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "打开 Peck", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 Peck", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(IDLE)?)
        .icon_as_template(true)
        .tooltip("Peck")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// 界面说现在是什么状态、在跑几个（异步命令在后台线程上，这里转到主线程做）。
pub fn update(app: &AppHandle, activity: Activity, count: u32) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        set_activity(&handle, activity);
        set_running_count(&handle, count);
    });
}

/// 在跑几个（v0.7，不同项目能同时跑）：两个以上时菜单栏图标旁边写个数，提示里也写。只在主线程上调。
fn set_running_count(app: &AppHandle, count: u32) {
    // 提示每次都按现在的状态和个数重写（个数掉到两个以下、或者变成等你，都不留旧的字）。
    let activity = app.state::<TrayState>().current.lock().ok().and_then(|c| c.0);
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_title(if count >= 2 { Some(count.to_string()) } else { None });
        let tip = match activity {
            Some(Activity::Waiting) => "Peck · 等你回答".to_string(),
            Some(Activity::Running) if count >= 2 => format!("Peck · {count} 个任务在跑"),
            Some(Activity::Running) => "Peck · 有任务在跑".to_string(),
            _ => "Peck".to_string(),
        };
        let _ = tray.set_tooltip(Some(tip));
    }
}

/// 有任务在跑、停下来等站主、都没有。只在主线程上调。
fn set_activity(app: &AppHandle, activity: Activity) {
    let state = app.state::<TrayState>();
    let generation = {
        let Ok(mut current) = state.current.lock() else {
            return;
        };
        if current.0 == Some(activity) {
            return;
        }
        current.0 = Some(activity);
        current.1 += 1;
        current.1
    };
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(match activity {
            Activity::Idle => "Peck",
            Activity::Running => "Peck · 有任务在跑",
            Activity::Waiting => "Peck · 等你回答",
        }));
    }
    match activity {
        Activity::Idle => set_icon(app, IDLE),
        Activity::Waiting => set_icon(app, WAIT),
        Activity::Running => {
            let app = app.clone();
            std::thread::spawn(move || {
                let mut frame = 0usize;
                loop {
                    let still_running = app
                        .state::<TrayState>()
                        .current
                        .lock()
                        .map(|current| current.1 == generation)
                        .unwrap_or(false);
                    if !still_running {
                        break;
                    }
                    let bytes = RUN[frame % RUN.len()];
                    let handle = app.clone();
                    // 排队到主线程的这段时间里可能已经切到空闲、等你：到时再核对一次，不然会停在跑动的那一帧上。
                    let still = move |app: &AppHandle| app.state::<TrayState>().current.lock().map(|c| c.1 == generation).unwrap_or(false);
                    if app.run_on_main_thread(move || if still(&handle) { set_icon(&handle, bytes) }).is_err() {
                        break;
                    }
                    frame += 1;
                    std::thread::sleep(FRAME);
                }
            });
        }
    }
}
