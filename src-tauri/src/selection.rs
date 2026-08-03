#[cfg(target_os = "windows")]
use crate::native_button;
#[cfg(not(target_os = "windows"))]
use crate::position_and_show_button;
use crate::{clamp_window, hide_button, AppState};
use arboard::Clipboard;
#[cfg(not(target_os = "windows"))]
use rdev::{listen, Button, Event, EventType};
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::process::Command;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, OnceLock,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{Emitter, Manager};

// Clipboard capture is slower than mouse input, so captures can overlap.
// Only the newest generation may publish a result and clipboard access is serialized.
static SELECTION_GENERATION: AtomicU64 = AtomicU64::new(0);
static CAPTURE_LOCK: Mutex<()> = Mutex::new(());
static DIAGNOSTIC_PATH: OnceLock<PathBuf> = OnceLock::new();
static DIAGNOSTIC_LOCK: Mutex<()> = Mutex::new(());

fn initialize_diagnostics(app: &tauri::AppHandle) {
    let Ok(directory) = app.path().app_log_dir() else {
        return;
    };
    if fs::create_dir_all(&directory).is_ok() {
        let _ = DIAGNOSTIC_PATH.set(directory.join("selection.log"));
    }
}

pub(crate) fn diagnostic(message: &str) {
    let Some(path) = DIAGNOSTIC_PATH.get() else {
        return;
    };
    let Ok(_guard) = DIAGNOSTIC_LOCK.lock() else {
        return;
    };
    let rotate = fs::metadata(path)
        .map(|metadata| metadata.len() > 256 * 1024)
        .unwrap_or(false);
    let mut options = OpenOptions::new();
    options.create(true).write(true);
    if rotate {
        options.truncate(true);
    } else {
        options.append(true);
    }
    let Ok(mut file) = options.open(path) else {
        return;
    };
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let _ = writeln!(file, "{timestamp} {message}");
}

#[cfg(not(target_os = "windows"))]
fn press_copy_shortcut() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let status = Command::new("osascript")
            .args([
                "-e",
                r#"tell application "System Events" to keystroke "c" using command down"#,
            ])
            .status()
            .map_err(|error| error.to_string())?;
        return status
            .success()
            .then_some(())
            .ok_or("无法触发复制快捷键".into());
    }
    #[cfg(target_os = "linux")]
    {
        let status = Command::new("xdotool")
            .args(["key", "--clearmodifiers", "ctrl+c"])
            .status()
            .map_err(|_| "Linux 需要安装 xdotool 才能读取选区".to_string())?;
        return status
            .success()
            .then_some(())
            .ok_or("无法触发复制快捷键".into());
    }
}

#[cfg(any())]
enum UiaSelection {
    Selected(String),
    None,
    Unsupported,
}

#[cfg(target_os = "windows")]
fn foreground_window_label() -> String {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetForegroundWindow, GetWindowTextW,
    };

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return "hwnd=none".to_owned();
        }
        let mut title = [0u16; 160];
        let mut class_name = [0u16; 160];
        let title_len = GetWindowTextW(hwnd, &mut title) as usize;
        let class_len = GetClassNameW(hwnd, &mut class_name) as usize;
        let title = String::from_utf16_lossy(&title[..title_len.min(title.len())]);
        let class_name = String::from_utf16_lossy(&class_name[..class_len.min(class_name.len())]);
        format!("class={class_name} title={title}")
    }
}

#[cfg(any())]
fn prefer_clipboard_capture(target: &str) -> bool {
    let lower = target.to_ascii_lowercase();
    lower.contains("codex")
        || lower.contains("chatgpt")
        || lower.contains("thunderbird")
        || lower.contains("mozillawindowclass")
        || lower.contains("wechat")
        || lower.contains("weixin")
        || target.contains("微信")
}

#[cfg(any())]
fn read_selected_text_uia(start_x: i32, start_y: i32, end_x: i32, end_y: i32) -> UiaSelection {
    use windows::Win32::{
        Foundation::POINT,
        System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
            COINIT_APARTMENTTHREADED,
        },
        UI::Accessibility::{
            CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern,
            TextPatternRangeEndpoint_End, TextPatternRangeEndpoint_Start, UIA_TextPatternId,
        },
    };

    unsafe {
        if CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_err() {
            return UiaSelection::Unsupported;
        }

        let result = (|| -> windows::core::Result<UiaSelection> {
            let automation: IUIAutomation =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
            let walker = automation.RawViewWalker()?;
            let selection_from = |element: &IUIAutomationElement| -> Option<UiaSelection> {
                let pattern: IUIAutomationTextPattern =
                    element.GetCurrentPatternAs(UIA_TextPatternId).ok()?;
                let ranges = pattern.GetSelection().ok()?;
                if ranges.Length().ok()? == 0 {
                    return Some(UiaSelection::None);
                }
                let range = ranges.GetElement(0).ok()?;
                let is_degenerate = range
                    .CompareEndpoints(
                        TextPatternRangeEndpoint_Start,
                        &range,
                        TextPatternRangeEndpoint_End,
                    )
                    .ok()?
                    == 0;
                if is_degenerate {
                    return Some(UiaSelection::None);
                }
                let text = range.GetText(-1).ok()?.to_string().trim().to_owned();
                if text.is_empty() {
                    Some(UiaSelection::None)
                } else {
                    Some(UiaSelection::Selected(text))
                }
            };

            let mut found_text_pattern = false;
            let mut roots = Vec::new();
            for point in [
                POINT { x: end_x, y: end_y },
                POINT {
                    x: start_x,
                    y: start_y,
                },
            ] {
                if let Ok(element) = automation.ElementFromPoint(point) {
                    roots.push(element);
                }
            }
            if let Ok(element) = automation.GetFocusedElement() {
                roots.push(element);
            }

            let mut walker_roots = 0;
            for root in roots {
                walker_roots += 1;
                let mut element = root;
                for _ in 0..16 {
                    if let Some(selection) = selection_from(&element) {
                        found_text_pattern = true;
                        if let UiaSelection::Selected(_) = selection {
                            return Ok(selection);
                        }
                    }
                    let Ok(parent) = walker.GetParentElement(&element) else {
                        break;
                    };
                    element = parent;
                }
            }
            if walker_roots == 0 {
                return Ok(UiaSelection::Unsupported);
            }

            Ok(if found_text_pattern {
                UiaSelection::None
            } else {
                UiaSelection::Unsupported
            })
        })();

        CoUninitialize();
        result.unwrap_or(UiaSelection::Unsupported)
    }
}

#[cfg(any())]
fn press_copy_shortcut_windows(paced: bool) -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
        KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL,
    };
    const KEY_C: VIRTUAL_KEY = VIRTUAL_KEY(0x43);

    fn keyboard_input(key: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: key,
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    unsafe {
        if !paced {
            let control_was_down = GetAsyncKeyState(VK_CONTROL.0 as i32) < 0;
            let mut inputs = Vec::with_capacity(4);
            if !control_was_down {
                inputs.push(keyboard_input(VK_CONTROL, KEYBD_EVENT_FLAGS(0)));
            }
            inputs.push(keyboard_input(KEY_C, KEYBD_EVENT_FLAGS(0)));
            inputs.push(keyboard_input(KEY_C, KEYEVENTF_KEYUP));
            if !control_was_down {
                inputs.push(keyboard_input(VK_CONTROL, KEYEVENTF_KEYUP));
            }
            let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
            return if sent == inputs.len() as u32 {
                Ok(())
            } else {
                Err("无法发送复制快捷键".into())
            };
        }

        let send_key = |input: INPUT| SendInput(&[input], std::mem::size_of::<INPUT>() as i32) == 1;
        let control_was_down = GetAsyncKeyState(VK_CONTROL.0 as i32) < 0;
        let mut success = true;
        if !control_was_down {
            success &= send_key(keyboard_input(VK_CONTROL, KEYBD_EVENT_FLAGS(0)));
            thread::sleep(Duration::from_millis(20));
        }
        success &= send_key(keyboard_input(KEY_C, KEYBD_EVENT_FLAGS(0)));
        thread::sleep(Duration::from_millis(15));
        success &= send_key(keyboard_input(KEY_C, KEYEVENTF_KEYUP));
        thread::sleep(Duration::from_millis(10));
        if !control_was_down {
            success &= send_key(keyboard_input(VK_CONTROL, KEYEVENTF_KEYUP));
        }
        if !success {
            return Err("无法发送复制快捷键".into());
        }
    }
    Ok(())
}

#[cfg(any())]
fn send_copy_message_to_focused_control() -> usize {
    use windows::Win32::{
        Foundation::{LPARAM, WPARAM},
        UI::WindowsAndMessaging::{
            GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, SendMessageTimeoutW,
            GUITHREADINFO, SMTO_ABORTIFHUNG, WM_COPY,
        },
    };

    unsafe {
        let foreground = GetForegroundWindow();
        let thread_id = GetWindowThreadProcessId(foreground, None);
        if thread_id == 0 {
            return 0;
        }
        let mut info = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        if GetGUIThreadInfo(thread_id, &mut info).is_err() {
            return 0;
        }
        let targets = [info.hwndFocus, info.hwndCaret, info.hwndActive, foreground];
        let mut sent = 0;
        for (index, target) in targets.iter().enumerate() {
            if target.0.is_null()
                || targets[..index]
                    .iter()
                    .any(|previous| previous.0 == target.0)
            {
                continue;
            }
            let _ = SendMessageTimeoutW(
                *target,
                WM_COPY,
                WPARAM(0),
                LPARAM(0),
                SMTO_ABORTIFHUNG,
                40,
                None,
            );
            sent += 1;
        }
        sent
    }
}

#[cfg(any())]
fn send_copy_keys_to_focused_control() -> usize {
    use windows::Win32::{
        Foundation::{LPARAM, WPARAM},
        UI::{
            Input::KeyboardAndMouse::VK_CONTROL,
            WindowsAndMessaging::{
                GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId,
                SendMessageTimeoutW, GUITHREADINFO, SMTO_ABORTIFHUNG, WM_KEYDOWN, WM_KEYUP,
            },
        },
    };

    unsafe {
        let foreground = GetForegroundWindow();
        let thread_id = GetWindowThreadProcessId(foreground, None);
        if thread_id == 0 {
            return 0;
        }
        let mut info = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        if GetGUIThreadInfo(thread_id, &mut info).is_err() {
            return 0;
        }
        let target = if info.hwndFocus.0.is_null() {
            foreground
        } else {
            info.hwndFocus
        };
        if target.0.is_null() {
            return 0;
        }
        let messages = [
            (WM_KEYDOWN, VK_CONTROL.0 as usize, 1_isize),
            (WM_KEYDOWN, 0x43_usize, 1_isize),
            (WM_KEYUP, 0x43_usize, 0xC0000001_u32 as isize),
            (WM_KEYUP, VK_CONTROL.0 as usize, 0xC0000001_u32 as isize),
        ];
        let mut sent = 0;
        for (message, key, flags) in messages {
            let _ = SendMessageTimeoutW(
                target,
                message,
                WPARAM(key),
                LPARAM(flags),
                SMTO_ABORTIFHUNG,
                80,
                None,
            );
            sent += 1;
        }
        sent
    }
}

#[cfg(any())]
fn wait_for_copied_text(
    clipboard: &mut Clipboard,
    marker: &str,
    attempts: usize,
) -> Option<String> {
    for _ in 0..attempts {
        thread::sleep(Duration::from_millis(20));
        if let Ok(selected) = clipboard.get_text() {
            let selected = selected.trim().to_owned();
            if selected != marker && !selected.is_empty() {
                return Some(selected);
            }
        }
    }
    None
}

#[cfg(any())]
fn read_selected_text_clipboard_fallback() -> Result<String, String> {
    let mut clipboard = Clipboard::new().map_err(|error| error.to_string())?;
    let previous = clipboard.get_text().unwrap_or_default();
    let marker = format!("__LUMA_SELECTION_{:?}__", SystemTime::now());
    clipboard
        .set_text(&marker)
        .map_err(|error| error.to_string())?;

    let copy_message_targets = send_copy_message_to_focused_control();
    diagnostic(&format!(
        "clipboard source=wm_copy targets={copy_message_targets}"
    ));
    if copy_message_targets > 0 {
        if let Some(selected) = wait_for_copied_text(&mut clipboard, &marker, 15) {
            diagnostic(&format!(
                "clipboard source=wm_copy status=selected chars={}",
                selected.chars().count()
            ));
            return Ok(selected);
        }
    }

    // Reset the marker because some controls clear the clipboard when WM_COPY
    // is unsupported. Electron and custom-rendered editors use this path.
    clipboard
        .set_text(&marker)
        .map_err(|error| error.to_string())?;
    if let Err(error) = press_copy_shortcut_windows(false) {
        diagnostic("clipboard source=ctrl_c_fast status=send_failed");
        let _ = clipboard.set_text(previous);
        return Err(error);
    }
    if let Some(selected) = wait_for_copied_text(&mut clipboard, &marker, 25) {
        diagnostic(&format!(
            "clipboard source=ctrl_c_fast status=selected chars={}",
            selected.chars().count()
        ));
        return Ok(selected);
    }

    clipboard
        .set_text(&marker)
        .map_err(|error| error.to_string())?;
    if let Err(error) = press_copy_shortcut_windows(true) {
        diagnostic("clipboard source=ctrl_c_paced status=send_failed");
        let _ = clipboard.set_text(previous);
        return Err(error);
    }
    if let Some(selected) = wait_for_copied_text(&mut clipboard, &marker, 25) {
        diagnostic(&format!(
            "clipboard source=ctrl_c_paced status=selected chars={}",
            selected.chars().count()
        ));
        return Ok(selected);
    }

    clipboard
        .set_text(&marker)
        .map_err(|error| error.to_string())?;
    let key_messages = send_copy_keys_to_focused_control();
    diagnostic(&format!(
        "clipboard source=focused_key_messages messages={key_messages}"
    ));
    if key_messages > 0 {
        if let Some(selected) = wait_for_copied_text(&mut clipboard, &marker, 25) {
            diagnostic(&format!(
                "clipboard source=focused_key_messages status=selected chars={}",
                selected.chars().count()
            ));
            return Ok(selected);
        }
    }

    diagnostic("clipboard status=empty_or_timeout");
    let _ = clipboard.set_text(previous);
    Ok(String::new())
}

#[cfg(target_os = "windows")]
fn read_selected_text(
    _start_x: i32,
    _start_y: i32,
    _end_x: i32,
    _end_y: i32,
) -> Result<String, String> {
    let target = foreground_window_label();
    diagnostic(&format!("capture status=started target={target}"));
    // Windows selection capture is intentionally clipboard-only. UIA support
    // varies by control framework and made capture behavior application-specific.
    read_selected_text_windows_reliable()
}

#[cfg(target_os = "windows")]
fn user_keyboard_input_active() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU,
        VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
    };

    const KEY_C: i32 = 0x43;
    let keys = [
        VK_CONTROL.0 as i32,
        VK_LCONTROL.0 as i32,
        VK_RCONTROL.0 as i32,
        VK_MENU.0 as i32,
        VK_LMENU.0 as i32,
        VK_RMENU.0 as i32,
        VK_LWIN.0 as i32,
        VK_RWIN.0 as i32,
        VK_SHIFT.0 as i32,
        VK_LSHIFT.0 as i32,
        VK_RSHIFT.0 as i32,
        KEY_C,
    ];
    keys.into_iter()
        .any(|key| unsafe { GetAsyncKeyState(key) < 0 })
}

#[cfg(target_os = "windows")]
fn press_copy_shortcut_windows_reliable() -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
        KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, VIRTUAL_KEY, VK_CONTROL,
        VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL, VK_RMENU, VK_RSHIFT,
        VK_RWIN, VK_SHIFT,
    };

    const KEY_C: VIRTUAL_KEY = VIRTUAL_KEY(0x43);

    fn keyboard_input(key: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS) -> INPUT {
        let extended = matches!(key, VK_RCONTROL | VK_RMENU);
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: key,
                    wScan: unsafe { (MapVirtualKeyW(key.0 as u32, MAPVK_VK_TO_VSC) & 0xff) as u16 },
                    dwFlags: if extended {
                        flags | KEYEVENTF_EXTENDEDKEY
                    } else {
                        flags
                    },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    let modifier_keys = [
        VK_CONTROL,
        VK_LCONTROL,
        VK_RCONTROL,
        VK_MENU,
        VK_LMENU,
        VK_RMENU,
        VK_LWIN,
        VK_RWIN,
        VK_SHIFT,
        VK_LSHIFT,
        VK_RSHIFT,
    ];

    // Match H.InputSimulator's behavior used by STranslate: each stale
    // modifier is released in its own SendInput call, then Ctrl+C is sent as
    // one uninterrupted four-event sequence with scan codes populated.
    for key in modifier_keys {
        if user_keyboard_input_active() {
            return Err("physical keyboard input became active".into());
        }
        let input = keyboard_input(key, KEYEVENTF_KEYUP);
        let sent = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
        if sent != 1 {
            return Err(format!("SendInput could not release modifier {}", key.0));
        }
    }

    if user_keyboard_input_active() {
        return Err("physical keyboard input became active".into());
    }
    let inputs = [
        keyboard_input(VK_CONTROL, KEYBD_EVENT_FLAGS(0)),
        keyboard_input(KEY_C, KEYBD_EVENT_FLAGS(0)),
        keyboard_input(KEY_C, KEYEVENTF_KEYUP),
        keyboard_input(VK_CONTROL, KEYEVENTF_KEYUP),
    ];

    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent == inputs.len() as u32 {
        Ok(())
    } else {
        Err(format!(
            "SendInput only delivered {sent}/{} keyboard events",
            inputs.len()
        ))
    }
}

#[cfg(target_os = "windows")]
struct OleClipboardSnapshot {
    data: Option<windows::Win32::System::Com::IDataObject>,
}

#[cfg(target_os = "windows")]
impl OleClipboardSnapshot {
    fn capture() -> Result<Self, String> {
        use windows::Win32::System::Ole::{OleGetClipboard, OleInitialize, OleUninitialize};

        unsafe { OleInitialize(None) }.map_err(|error| error.to_string())?;
        match unsafe { OleGetClipboard() } {
            Ok(data) => Ok(Self { data: Some(data) }),
            Err(error) => {
                unsafe { OleUninitialize() };
                Err(error.to_string())
            }
        }
    }

    fn restore(mut self) -> Result<(), String> {
        use windows::Win32::System::Ole::{OleFlushClipboard, OleSetClipboard};

        let data = self
            .data
            .as_ref()
            .ok_or_else(|| "clipboard snapshot is empty".to_owned())?;
        unsafe { OleSetClipboard(data) }.map_err(|error| error.to_string())?;
        unsafe { OleFlushClipboard() }.map_err(|error| error.to_string())?;
        // Release the COM object while its OLE apartment is still initialized.
        self.data.take();
        Ok(())
    }
}

#[cfg(target_os = "windows")]
impl Drop for OleClipboardSnapshot {
    fn drop(&mut self) {
        use windows::Win32::System::Ole::OleUninitialize;

        self.data.take();
        unsafe { OleUninitialize() };
    }
}

#[cfg(target_os = "windows")]
enum ClipboardFallback {
    Text(String),
    Empty,
    Unavailable,
}

#[cfg(target_os = "windows")]
struct WindowsClipboardSnapshot {
    ole: Option<OleClipboardSnapshot>,
    fallback: ClipboardFallback,
}

#[cfg(target_os = "windows")]
impl WindowsClipboardSnapshot {
    fn capture(clipboard: &mut Clipboard) -> Self {
        use windows::Win32::System::DataExchange::CountClipboardFormats;

        let fallback = match clipboard.get_text() {
            Ok(text) => ClipboardFallback::Text(text),
            Err(_) if unsafe { CountClipboardFormats() } == 0 => ClipboardFallback::Empty,
            Err(_) => ClipboardFallback::Unavailable,
        };
        let ole = match OleClipboardSnapshot::capture() {
            Ok(snapshot) => Some(snapshot),
            Err(error) => {
                diagnostic(&format!(
                    "clipboard snapshot=ole status=unavailable reason={error}"
                ));
                None
            }
        };
        Self { ole, fallback }
    }

    fn original_text(&self) -> &str {
        match &self.fallback {
            ClipboardFallback::Text(text) => text,
            ClipboardFallback::Empty | ClipboardFallback::Unavailable => "",
        }
    }

    fn restore(mut self, clipboard: &mut Clipboard) -> Result<&'static str, String> {
        let mut ole_error = None;
        if let Some(snapshot) = self.ole.take() {
            match snapshot.restore() {
                Ok(()) => return Ok("ole"),
                Err(error) => ole_error = Some(error),
            }
        }

        let fallback_result = match self.fallback {
            ClipboardFallback::Text(text) => clipboard.set_text(text),
            ClipboardFallback::Empty => clipboard.clear(),
            ClipboardFallback::Unavailable => {
                return Err(ole_error.unwrap_or_else(|| {
                    "original clipboard format could not be captured".to_owned()
                }))
            }
        };
        fallback_result.map_err(|error| {
            let fallback_error = error.to_string();
            match ole_error {
                Some(ole_error) => format!("OLE: {ole_error}; fallback: {fallback_error}"),
                None => fallback_error,
            }
        })?;
        Ok("fallback")
    }
}

fn should_restore_clipboard(
    synthetic_copy: bool,
    capture_changed_clipboard: bool,
    captured_version: u32,
    current_version: u32,
) -> bool {
    synthetic_copy && capture_changed_clipboard && captured_version == current_version
}

#[cfg(target_os = "windows")]
fn read_selected_text_windows_reliable() -> Result<String, String> {
    use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;

    let mut clipboard = Clipboard::new().map_err(|error| error.to_string())?;
    let original_clipboard = WindowsClipboardSnapshot::capture(&mut clipboard);
    let original_text = original_clipboard.original_text().to_owned();
    let original_sequence = unsafe { GetClipboardSequenceNumber() };

    let mut current_sequence = original_sequence;
    let mut sequence_changed = false;
    let mut capture_source = "synthetic_ctrl_c";
    let mut synthetic_copy = false;

    // Give a user's own Ctrl+C priority. In particular, never release real
    // modifier keys while the user is pressing them: doing so can turn their
    // pending Ctrl+C into a plain "c" that replaces the selected text.
    for tick in 0..30 {
        current_sequence = unsafe { GetClipboardSequenceNumber() };
        if current_sequence != original_sequence {
            sequence_changed = true;
            capture_source = "user_ctrl_c";
            break;
        }
        if tick >= 8 && !user_keyboard_input_active() {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }

    if !sequence_changed {
        if user_keyboard_input_active() {
            diagnostic("clipboard source=user_keyboard status=skipped_synthetic_copy");
            return Ok(String::new());
        }
        if let Err(error) = press_copy_shortcut_windows_reliable() {
            diagnostic(&format!(
                "clipboard source=synthetic_ctrl_c status=aborted reason={error}"
            ));
            return Ok(String::new());
        }
        synthetic_copy = true;
    }

    for _ in 0..50 {
        if sequence_changed {
            break;
        }
        thread::sleep(Duration::from_millis(20));
        current_sequence = unsafe { GetClipboardSequenceNumber() };
        if current_sequence != original_sequence {
            sequence_changed = true;
            break;
        }
    }

    // Clipboard owners may publish formats in multiple sequence updates.
    // Wait until the sequence stays unchanged briefly before reading text.
    if sequence_changed {
        let mut stable_checks = 0;
        for _ in 0..15 {
            thread::sleep(Duration::from_millis(20));
            let next_sequence = unsafe { GetClipboardSequenceNumber() };
            if next_sequence == current_sequence {
                stable_checks += 1;
                if stable_checks >= 3 {
                    break;
                }
            } else {
                current_sequence = next_sequence;
                stable_checks = 0;
            }
        }
    }

    let mut current_text = String::new();
    for _ in 0..25 {
        if let Ok(value) = clipboard.get_text() {
            let value = value.trim().to_owned();
            if !value.is_empty() {
                current_text = value;
                break;
            }
        }
        thread::sleep(Duration::from_millis(20));
    }
    let content_changed = current_text != original_text.trim();
    diagnostic(&format!(
        "clipboard source={capture_source} sequence_before={original_sequence} sequence_after={current_sequence} sequence_changed={sequence_changed} content_changed={content_changed} chars={}",
        current_text.chars().count()
    ));

    // Only undo our own synthetic Ctrl+C. A real user Ctrl+C remains normal
    // system behavior. The sequence guard also prevents us from overwriting
    // anything the user copied after selection capture completed.
    let clipboard_version = unsafe { GetClipboardSequenceNumber() };
    if should_restore_clipboard(
        synthetic_copy,
        sequence_changed,
        current_sequence,
        clipboard_version,
    ) {
        match original_clipboard.restore(&mut clipboard) {
            Ok(method) => diagnostic(&format!(
                "clipboard restore=completed method={method} captured_version={current_sequence}"
            )),
            Err(error) => diagnostic(&format!(
                "clipboard restore=failed captured_version={current_sequence} reason={error}"
            )),
        }
    } else if synthetic_copy {
        let reason = if sequence_changed {
            "newer_clipboard_content"
        } else {
            "capture_did_not_change_clipboard"
        };
        diagnostic(&format!(
            "clipboard restore=skipped reason={reason} captured_version={current_sequence} current_version={clipboard_version}"
        ));
    }

    if !(sequence_changed || content_changed) || current_text.is_empty() {
        return Ok(String::new());
    }
    Ok(current_text)
}

#[cfg(not(target_os = "windows"))]
fn read_selected_text(
    _start_x: i32,
    _start_y: i32,
    _end_x: i32,
    _end_y: i32,
) -> Result<String, String> {
    let mut clipboard = Clipboard::new().map_err(|error| error.to_string())?;
    let previous = clipboard.get_text().unwrap_or_default();
    let marker = format!("__LUMA_SELECTION_{:?}__", std::time::SystemTime::now());
    clipboard
        .set_text(&marker)
        .map_err(|error| error.to_string())?;
    if let Err(error) = press_copy_shortcut() {
        let _ = clipboard.set_text(previous);
        return Err(error);
    }
    thread::sleep(Duration::from_millis(100));
    let captured_clipboard_text = clipboard.get_text().unwrap_or_default();
    if captured_clipboard_text == marker {
        // 没有可复制的选区时撤销内部 marker，避免污染剪贴板。
        let _ = clipboard.set_text(previous);
        return Ok(String::new());
    }
    let selected = captured_clipboard_text.trim().to_owned();
    // 非 Windows 平台同样仅在剪贴板仍是本次捕获结果时恢复，避免覆盖
    // 用户紧接着主动复制的更新内容。
    if clipboard.get_text().ok().as_deref() == Some(captured_clipboard_text.as_str()) {
        let _ = clipboard.set_text(previous);
    }
    Ok(selected)
}

fn point_in_own_window(app: &tauri::AppHandle, x: i32, y: i32) -> bool {
    #[cfg(target_os = "windows")]
    if native_button::contains(x, y) {
        return true;
    }

    ["button", "panel", "settings"].iter().any(|label| {
        let Some(window) = app.get_webview_window(label) else {
            return false;
        };
        if !window.is_visible().unwrap_or(false) {
            return false;
        }
        let Ok(position) = window.outer_position() else {
            return false;
        };
        let Ok(size) = window.outer_size() else {
            return false;
        };
        x >= position.x
            && x <= position.x + size.width as i32
            && y >= position.y
            && y <= position.y + size.height as i32
    })
}

fn clear_current_selection(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    if let Ok(mut selected) = state.selected_text.lock() {
        selected.clear();
    };
}

fn is_latest_generation(generation: u64) -> bool {
    SELECTION_GENERATION.load(Ordering::Acquire) == generation
}

fn is_meaningful_drag(start_x: i32, start_y: i32, end_x: i32, end_y: i32) -> bool {
    let delta_x = i64::from(end_x) - i64::from(start_x);
    let delta_y = i64::from(end_y) - i64::from(start_y);
    delta_x * delta_x + delta_y * delta_y >= 16
}

#[cfg(not(target_os = "windows"))]
fn physical_cursor_position(fallback: (i32, i32)) -> (i32, i32) {
    fallback
}

fn hide_and_clear_button(app: &tauri::AppHandle) {
    if let Some(button) = app.get_webview_window("button") {
        hide_button(&button);
    }
    clear_current_selection(app);
}

fn handle_mouse_up(
    app: tauri::AppHandle,
    start_x: i32,
    start_y: i32,
    x: i32,
    y: i32,
    generation: u64,
) {
    thread::sleep(Duration::from_millis(120));
    if !is_latest_generation(generation) {
        diagnostic("capture status=discarded_before_lock reason=stale_generation");
        return;
    }

    let Ok(_capture_guard) = CAPTURE_LOCK.lock() else {
        return;
    };
    if !is_latest_generation(generation) {
        diagnostic("capture status=discarded_after_lock reason=stale_generation");
        return;
    }

    let Ok(text) = read_selected_text(start_x, start_y, x, y) else {
        #[cfg(debug_assertions)]
        eprintln!("[selection] no readable text selection");
        if is_latest_generation(generation) {
            hide_and_clear_button(&app);
        }
        return;
    };
    if !is_latest_generation(generation) {
        diagnostic("capture status=discarded_after_read reason=stale_generation");
        return;
    };
    if text.is_empty() || text.chars().count() > 12_000 {
        #[cfg(debug_assertions)]
        eprintln!("[selection] empty or oversized selection");
        hide_and_clear_button(&app);
        diagnostic("capture status=rejected reason=empty_or_oversized");
        return;
    }
    #[cfg(debug_assertions)]
    eprintln!("[selection] accepted {} chars", text.chars().count());
    let state = app.state::<AppState>();
    if let Ok(mut selected) = state.selected_text.lock() {
        *selected = text.clone();
    }
    if let Ok(mut point) = state.selection_point.lock() {
        *point = (x, y);
    }
    #[cfg(target_os = "windows")]
    {
        // Windows uses the native HWND exclusively. Do not gate it on the
        // optional WebView button being created or painted.
        let (px, py) = clamp_window(&app, x + 12, y + 12, 42, 42);
        if let Some(button) = app.get_webview_window("button") {
            let _ = button.emit("selection-changed", &text);
            let _ = button.hide();
        }
        match native_button::show(px, py) {
            Some((left, top, right, bottom, visible)) => diagnostic(&format!(
                "button status=shown_native requested={px},{py} actual={left},{top},{right},{bottom} visible={visible}"
            )),
            None => diagnostic("button status=native_window_missing"),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let Some(button) = app.get_webview_window("button") else {
            diagnostic("button status=missing_window");
            return;
        };
        let button_size = button
            .outer_size()
            .unwrap_or(tauri::PhysicalSize::new(42, 42));
        let (px, py) = clamp_window(
            &app,
            x + 12,
            y + 12,
            button_size.width as i32,
            button_size.height as i32,
        );
        let _ = button.emit("selection-changed", &text);
        match position_and_show_button(&button, px, py) {
            Ok((actual_x, actual_y, visible)) => diagnostic(&format!(
                "button status=shown requested={px},{py} actual={actual_x},{actual_y} visible={visible}"
            )),
            Err(error) => diagnostic(&format!("button status=show_failed error={error}")),
        }
    }

    let timeout_app = app.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(12));
        if is_latest_generation(generation) {
            if let Some(button) = timeout_app.get_webview_window("button") {
                hide_button(&button);
            }
            clear_current_selection(&timeout_app);
        }
    });
}

#[cfg(not(target_os = "windows"))]
pub fn start_selection_watcher(app: tauri::AppHandle) {
    initialize_diagnostics(&app);
    diagnostic("watcher status=started");
    thread::spawn(move || {
        let mut cursor = (0_i32, 0_i32);
        let mut mouse_down = None;
        let callback = move |event: Event| match event.event_type {
            EventType::MouseMove { x, y } => cursor = (x as i32, y as i32),
            EventType::ButtonPress(button) => {
                let (x, y) = physical_cursor_position(cursor);
                if point_in_own_window(&app, x, y) {
                    mouse_down = None;
                    return;
                }

                let generation = SELECTION_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
                hide_and_clear_button(&app);
                if button == Button::Left {
                    if let Some(panel) = app.get_webview_window("panel") {
                        let _ = panel.hide();
                    }
                    mouse_down = Some((x, y, generation));
                } else {
                    mouse_down = None;
                }
            }
            EventType::ButtonRelease(Button::Left) => {
                let (x, y) = physical_cursor_position(cursor);
                if point_in_own_window(&app, x, y) {
                    mouse_down = None;
                    return;
                }

                // A plain click is not a selection. This also prevents editors such as
                // VS Code from treating "copy with an empty selection" as copying a line.
                let Some((start_x, start_y, generation)) = mouse_down.take() else {
                    return;
                };
                if !is_meaningful_drag(start_x, start_y, x, y) {
                    return;
                }

                let handle = app.clone();
                thread::spawn(move || handle_mouse_up(handle, start_x, start_y, x, y, generation));
            }
            _ => {}
        };
        if let Err(error) = listen(callback) {
            eprintln!("global input listener failed: {error:?}");
        }
    });
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug)]
enum NativeMouseEvent {
    LeftDown(i32, i32, bool),
    Move(i32, i32),
    LeftUp(i32, i32, bool),
    OtherDown,
}

#[cfg(target_os = "windows")]
static NATIVE_MOUSE_SENDER: OnceLock<std::sync::mpsc::Sender<NativeMouseEvent>> = OnceLock::new();
#[cfg(target_os = "windows")]
static NATIVE_LEFT_DOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "windows")]
unsafe extern "system" fn low_level_mouse_hook(
    code: i32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, MSLLHOOKSTRUCT, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MOUSEMOVE,
        WM_RBUTTONDOWN, WM_XBUTTONDOWN,
    };

    if code >= 0 {
        let details = &*(lparam.0 as *const MSLLHOOKSTRUCT);
        let injected = details.flags & 1 != 0;
        let event = match wparam.0 as u32 {
            WM_LBUTTONDOWN => {
                NATIVE_LEFT_DOWN.store(true, Ordering::Release);
                Some(NativeMouseEvent::LeftDown(
                    details.pt.x,
                    details.pt.y,
                    injected,
                ))
            }
            WM_MOUSEMOVE if NATIVE_LEFT_DOWN.load(Ordering::Acquire) => {
                Some(NativeMouseEvent::Move(details.pt.x, details.pt.y))
            }
            WM_LBUTTONUP => {
                NATIVE_LEFT_DOWN.store(false, Ordering::Release);
                Some(NativeMouseEvent::LeftUp(
                    details.pt.x,
                    details.pt.y,
                    injected,
                ))
            }
            WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN => Some(NativeMouseEvent::OtherDown),
            _ => None,
        };
        if let (Some(sender), Some(event)) = (NATIVE_MOUSE_SENDER.get(), event) {
            let _ = sender.send(event);
        }
    }

    CallNextHookEx(None, code, wparam, lparam)
}

#[cfg(target_os = "windows")]
fn run_native_mouse_hook() {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetMessageW, SetWindowsHookExW, UnhookWindowsHookEx, MSG, WH_MOUSE_LL,
    };

    unsafe {
        let hook = match SetWindowsHookExW(WH_MOUSE_LL, Some(low_level_mouse_hook), None, 0) {
            Ok(hook) => hook,
            Err(error) => {
                diagnostic(&format!("mouse_hook status=start_failed error={error}"));
                return;
            }
        };
        diagnostic("mouse_hook status=started backend=WH_MOUSE_LL");

        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).0 > 0 {}
        let _ = UnhookWindowsHookEx(hook);
        diagnostic("mouse_hook status=stopped");
    }
}

#[cfg(target_os = "windows")]
struct NativeDrag {
    start_x: i32,
    start_y: i32,
    farthest_x: i32,
    farthest_y: i32,
    max_distance_squared: i64,
    move_count: u32,
    generation: u64,
}

#[cfg(target_os = "windows")]
impl NativeDrag {
    fn new(x: i32, y: i32, generation: u64) -> Self {
        Self {
            start_x: x,
            start_y: y,
            farthest_x: x,
            farthest_y: y,
            max_distance_squared: 0,
            move_count: 0,
            generation,
        }
    }

    fn observe(&mut self, x: i32, y: i32) {
        self.move_count = self.move_count.saturating_add(1);
        let delta_x = i64::from(x) - i64::from(self.start_x);
        let delta_y = i64::from(y) - i64::from(self.start_y);
        let distance_squared = delta_x * delta_x + delta_y * delta_y;
        if distance_squared > self.max_distance_squared {
            self.max_distance_squared = distance_squared;
            self.farthest_x = x;
            self.farthest_y = y;
        }
    }

    fn is_meaningful(&self) -> bool {
        self.max_distance_squared >= 16
    }
}

#[cfg(target_os = "windows")]
pub fn start_selection_watcher(app: tauri::AppHandle) {
    initialize_diagnostics(&app);
    diagnostic("watcher status=starting backend=WH_MOUSE_LL");

    let (sender, receiver) = std::sync::mpsc::channel::<NativeMouseEvent>();
    if NATIVE_MOUSE_SENDER.set(sender).is_err() {
        diagnostic("mouse_hook status=start_skipped reason=already_started");
        return;
    }

    thread::spawn(run_native_mouse_hook);
    thread::spawn(move || {
        let mut mouse_down: Option<NativeDrag> = None;

        while let Ok(event) = receiver.recv() {
            match event {
                NativeMouseEvent::LeftDown(x, y, injected) => {
                    if injected {
                        diagnostic(&format!(
                            "mouse_hook event=left_down injected=true at={x},{y}"
                        ));
                    }
                    if point_in_own_window(&app, x, y) {
                        if injected {
                            diagnostic("mouse_hook event=left_down ignored=own_window");
                        }
                        mouse_down = None;
                        continue;
                    }

                    let generation = SELECTION_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
                    hide_and_clear_button(&app);
                    if let Some(panel) = app.get_webview_window("panel") {
                        let _ = panel.hide();
                    }
                    mouse_down = Some(NativeDrag::new(x, y, generation));
                }
                NativeMouseEvent::Move(x, y) => {
                    if let Some(drag) = mouse_down.as_mut() {
                        drag.observe(x, y);
                    }
                }
                NativeMouseEvent::LeftUp(x, y, injected) => {
                    if injected {
                        diagnostic(&format!(
                            "mouse_hook event=left_up injected=true at={x},{y}"
                        ));
                    }
                    if point_in_own_window(&app, x, y) {
                        if injected {
                            diagnostic("mouse_hook event=left_up ignored=own_window");
                        }
                        mouse_down = None;
                        continue;
                    }

                    let Some(mut drag) = mouse_down.take() else {
                        continue;
                    };
                    drag.observe(x, y);
                    if !drag.is_meaningful() {
                        let target = foreground_window_label();
                        diagnostic(&format!(
                            "drag status=ignored reason=distance start={},{} end={x},{y} moves={} target={target}",
                            drag.start_x, drag.start_y, drag.move_count
                        ));
                        continue;
                    }

                    let (end_x, end_y) = if is_meaningful_drag(drag.start_x, drag.start_y, x, y) {
                        (x, y)
                    } else {
                        (drag.farthest_x, drag.farthest_y)
                    };
                    diagnostic(&format!(
                        "drag status=finished start={},{} end={end_x},{end_y} release={x},{y} moves={} max_distance_squared={}",
                        drag.start_x,
                        drag.start_y,
                        drag.move_count,
                        drag.max_distance_squared
                    ));
                    let handle = app.clone();
                    thread::spawn(move || {
                        handle_mouse_up(
                            handle,
                            drag.start_x,
                            drag.start_y,
                            end_x,
                            end_y,
                            drag.generation,
                        )
                    });
                }
                NativeMouseEvent::OtherDown => {
                    SELECTION_GENERATION.fetch_add(1, Ordering::AcqRel);
                    hide_and_clear_button(&app);
                    mouse_down = None;
                }
            }
        }
        diagnostic("mouse_hook status=event_channel_closed");
    });
}

#[cfg(test)]
mod tests {
    use super::{is_meaningful_drag, should_restore_clipboard};

    #[test]
    fn plain_click_and_pointer_jitter_are_not_selections() {
        assert!(!is_meaningful_drag(100, 100, 100, 100));
        assert!(!is_meaningful_drag(100, 100, 102, 102));
    }

    #[test]
    fn mouse_drag_is_a_selection_candidate() {
        assert!(is_meaningful_drag(100, 100, 104, 100));
        assert!(is_meaningful_drag(100, 100, 90, 125));
    }

    #[test]
    fn restores_only_unchanged_synthetic_clipboard_content() {
        assert!(should_restore_clipboard(true, true, 42, 42));
        assert!(!should_restore_clipboard(false, true, 42, 42));
        assert!(!should_restore_clipboard(true, false, 42, 42));
        assert!(!should_restore_clipboard(true, true, 42, 43));
    }
}
