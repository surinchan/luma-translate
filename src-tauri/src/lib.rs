#[cfg(target_os = "windows")]
mod native_button;
mod selection;
mod settings;
mod translator;

use selection::{diagnostic, start_selection_watcher};
use settings::{load_settings, save_settings, PublicSettings, SettingsInput};
use std::sync::Mutex;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Listener, LogicalSize, Manager, PhysicalPosition, Position, Size, State,
    WindowEvent,
};
use tauri_plugin_autostart::ManagerExt;
use translator::{translate, translate_to};

#[derive(Default)]
struct AppState {
    selected_text: Mutex<String>,
    selection_point: Mutex<(i32, i32)>,
    // TrayIcon 会在 Drop 时从系统托盘移除，必须与应用保持相同生命周期。
    tray: Mutex<Option<TrayIcon>>,
}

const PANEL_WIDTH: f64 = 440.0;
const PANEL_HEIGHT: f64 = 320.0;

fn clamp_window(app: &AppHandle, x: i32, y: i32, width: i32, height: i32) -> (i32, i32) {
    let point = PhysicalPosition::new(x, y);
    if let Ok(Some(monitor)) = app.monitor_from_point(point.x as f64, point.y as f64) {
        let area = monitor.work_area();
        let left = area.position.x + 8;
        let top = area.position.y + 8;
        let right = area.position.x + area.size.width as i32 - width - 8;
        let bottom = area.position.y + area.size.height as i32 - height - 8;
        return (
            x.clamp(left, right.max(left)),
            y.clamp(top, bottom.max(top)),
        );
    }
    (x, y)
}

fn prepare_translation_panel(
    app: &AppHandle,
    panel: &tauri::WebviewWindow,
    x: i32,
    y: i32,
) -> Result<(), String> {
    panel.set_zoom(1.0).map_err(|error| error.to_string())?;

    let logical_size = Size::Logical(LogicalSize::new(PANEL_WIDTH, PANEL_HEIGHT));
    panel
        .set_size(logical_size)
        .map_err(|error| error.to_string())?;

    // Move the hidden panel to the target monitor first, then apply the logical
    // size again so mixed-DPI monitors cannot retain the previous scale/size.
    let (rough_x, rough_y) = clamp_window(app, x, y, PANEL_WIDTH as i32, PANEL_HEIGHT as i32);
    panel
        .set_position(Position::Physical(PhysicalPosition::new(rough_x, rough_y)))
        .map_err(|error| error.to_string())?;
    panel
        .set_size(logical_size)
        .map_err(|error| error.to_string())?;

    let physical_size = panel.outer_size().map_err(|error| error.to_string())?;
    let (panel_x, panel_y) = clamp_window(
        app,
        x,
        y,
        physical_size.width as i32,
        physical_size.height as i32,
    );
    panel
        .set_position(Position::Physical(PhysicalPosition::new(panel_x, panel_y)))
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "windows")]
fn configure_button_no_activate(app: &AppHandle) -> Result<(), String> {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    };

    let window = app
        .get_webview_window("button")
        .ok_or("翻译按钮窗口不存在")?;
    window
        .set_focusable(false)
        .map_err(|error| error.to_string())?;
    let hwnd = window.hwnd().map_err(|error| error.to_string())?;
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(
            hwnd,
            GWL_EXSTYLE,
            style | WS_EX_NOACTIVATE.0 as isize | WS_EX_TOOLWINDOW.0 as isize,
        );
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn configure_button_no_activate(app: &AppHandle) -> Result<(), String> {
    app.get_webview_window("button")
        .ok_or_else(|| "翻译按钮窗口不存在".to_string())?
        .set_focusable(false)
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "windows")]
pub(crate) fn hide_button(window: &tauri::WebviewWindow) {
    use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};

    if let Ok(hwnd) = window.hwnd() {
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
    let _ = window.hide();
    native_button::hide();
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn hide_button(window: &tauri::WebviewWindow) {
    let _ = window.hide();
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn position_and_show_button(
    app: &AppHandle,
    window: &tauri::WebviewWindow,
    x: i32,
    y: i32,
) -> Result<(i32, i32, bool), String> {
    const BUTTON_SIZE: f64 = 42.0;

    window.set_zoom(1.0).map_err(|error| error.to_string())?;
    window
        .set_size(Size::Logical(LogicalSize::new(BUTTON_SIZE, BUTTON_SIZE)))
        .map_err(|error| error.to_string())?;
    let physical_size = window.outer_size().map_err(|error| error.to_string())?;
    let (x, y) = clamp_window(
        app,
        x,
        y,
        physical_size.width as i32,
        physical_size.height as i32,
    );
    window
        .set_position(Position::Physical(PhysicalPosition::new(x, y)))
        .map_err(|error| error.to_string())?;
    window.show().map_err(|error| error.to_string())?;
    Ok((x, y, window.is_visible().unwrap_or(true)))
}

#[tauri::command]
fn get_settings(app: AppHandle) -> Result<PublicSettings, String> {
    load_settings(&app)
        .map(PublicSettings::from)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn set_settings(app: AppHandle, input: SettingsInput) -> Result<PublicSettings, String> {
    let settings = save_settings(&app, input).map_err(|e| e.to_string())?;
    let autostart = app.autolaunch();
    if settings.launch_at_login {
        autostart
            .enable()
            .map_err(|e| format!("无法启用开机启动：{e}"))?;
    } else if autostart.is_enabled().unwrap_or(false) {
        autostart
            .disable()
            .map_err(|e| format!("无法关闭开机启动：{e}"))?;
    }
    Ok(PublicSettings::from(settings))
}

#[tauri::command]
fn open_settings(app: AppHandle) -> Result<(), String> {
    let window = app.get_webview_window("settings").ok_or("设置窗口不存在")?;
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())
}

#[tauri::command]
fn close_panel(app: AppHandle) -> Result<(), String> {
    app.get_webview_window("panel")
        .ok_or("翻译窗口不存在")?
        .hide()
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn copy_translation(text: String) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard.set_text(text).map_err(|e| e.to_string())
}

#[tauri::command]
async fn translate_text(
    app: AppHandle,
    text: String,
    target_language: String,
) -> Result<String, String> {
    let text = text.trim();
    let target_language = target_language.trim();
    if text.is_empty() {
        return Err("请输入需要翻译的文本".into());
    }
    if text.chars().count() > 50_000 {
        return Err("输入文本过长，请控制在 50000 个字符以内".into());
    }
    if target_language.is_empty() || target_language.chars().count() > 64 {
        return Err("请选择有效的目标语言".into());
    }

    diagnostic(&format!(
        "translate source=manual status=started chars={} target={target_language}",
        text.chars().count(),
    ));
    match translate_to(&app, text, Some(target_language)).await {
        Ok(value) => {
            diagnostic("translate source=manual status=finished");
            Ok(value)
        }
        Err(error) => {
            diagnostic("translate source=manual status=failed");
            Err(error.to_string())
        }
    }
}

#[tauri::command]
async fn translate_selected_inner(app: AppHandle) -> Result<String, String> {
    diagnostic("translate status=button_clicked");
    let state = app.state::<AppState>();
    let text = state
        .selected_text
        .lock()
        .map_err(|_| "无法读取选中文字")?
        .clone();
    if text.is_empty() {
        diagnostic("translate status=rejected reason=empty_selection");
        return Err("没有检测到选中的文字".into());
    }

    let (x, y) = *state
        .selection_point
        .lock()
        .map_err(|_| "无法读取鼠标位置")?;
    let panel = app.get_webview_window("panel").ok_or("翻译窗口不存在")?;
    prepare_translation_panel(&app, &panel, x + 12, y + 12)?;
    panel
        .emit("selection-changed", &text)
        .map_err(|e| e.to_string())?;
    panel
        .emit("translation-loading", ())
        .map_err(|e| e.to_string())?;
    panel.show().map_err(|e| e.to_string())?;
    panel.set_focus().map_err(|e| e.to_string())?;
    if let Some(button) = app.get_webview_window("button") {
        hide_button(&button);
    }

    let translated = match translate(&app, &text).await {
        Ok(value) => value,
        Err(error) => {
            diagnostic("translate status=failed");
            let message = error.to_string();
            let _ = panel.emit("translation-error", &message);
            return Err(message);
        }
    };
    diagnostic("translate status=finished");
    panel
        .emit("translation-finished", &translated)
        .map_err(|e| e.to_string())?;
    Ok(translated)
}

#[tauri::command]
async fn translate_selected(app: AppHandle, _state: State<'_, AppState>) -> Result<String, String> {
    translate_selected_inner(app).await
}

fn show_settings_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn show_manual_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("manual") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        let _ = window.emit("manual-window-opened", ());
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<TrayIcon> {
    let manual = MenuItem::with_id(app, "manual", "输入文本翻译…", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "设置…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&manual, &settings, &quit])?;
    let tray_icon = tauri::image::Image::from_bytes(include_bytes!("../icons/32x32.png"))?;
    let builder = TrayIconBuilder::new()
        .icon(tray_icon)
        .tooltip("Luma Translate")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "manual" => {
                show_manual_window(app);
            }
            "settings" => {
                show_settings_window(app);
            }
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
                show_settings_window(tray.app_handle());
            }
        });
    builder.build(app)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "windows")]
    unsafe {
        // Keep GetPhysicalCursorPos/UIA coordinates and Tauri window positions
        // in the same virtual-screen coordinate space on mixed-DPI monitors.
        use windows::Win32::UI::HiDpi::{
            SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        };
        use windows::{core::w, Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID};
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = SetCurrentProcessExplicitAppUserModelID(w!("OpenAI.LumaTranslate.Selection.v2"));
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            get_settings,
            set_settings,
            open_settings,
            close_panel,
            copy_translation,
            translate_text,
            translate_selected
        ])
        .setup(|app| {
            let window_icon = tauri::image::Image::from_bytes(include_bytes!("../icons/icon.png"))?;
            for label in ["settings", "button", "panel", "manual"] {
                if let Some(window) = app.get_webview_window(label) {
                    window.set_icon(window_icon.clone())?;
                }
            }
            configure_button_no_activate(app.handle())?;
            if let Some(button) = app.get_webview_window("button") {
                hide_button(&button);
            }
            #[cfg(target_os = "windows")]
            {
                let native_app = app.handle().clone();
                app.listen("native-translate-click", move |_| {
                    let app = native_app.clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = translate_selected_inner(app).await;
                    });
                });
            }
            #[cfg(target_os = "windows")]
            native_button::start(app.handle().clone());
            let tray = build_tray(app.handle())?;
            let state = app.state::<AppState>();
            *state.tray.lock().expect("tray state lock poisoned") = Some(tray);
            start_selection_watcher(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if matches!(window.label(), "settings" | "manual") {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("failed to run Luma Translate");
}
