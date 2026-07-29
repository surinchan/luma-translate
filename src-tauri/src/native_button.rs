#![cfg(target_os = "windows")]

use std::{
    ffi::c_void,
    io::Cursor,
    sync::{
        atomic::{AtomicIsize, Ordering},
        OnceLock,
    },
};
use tauri::{AppHandle, Emitter};
use windows::{
    core::w,
    Win32::{
        Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM},
        Graphics::Gdi::{
            BeginPaint, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, EndPaint,
            SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
            BLENDFUNCTION, DIB_RGB_COLORS, PAINTSTRUCT,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::HiDpi::{SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2},
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DispatchMessageW, GetWindowRect, IsWindowVisible,
            LoadCursorW, PeekMessageW, RegisterClassW, SetWindowPos, ShowWindow, TranslateMessage,
            UpdateLayeredWindow, CS_HREDRAW, CS_VREDRAW, HTCLIENT, HTTRANSPARENT, HWND_TOPMOST,
            IDC_ARROW, MSG, PM_REMOVE, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, SW_SHOWNOACTIVATE,
            ULW_ALPHA, WINDOW_EX_STYLE, WM_DESTROY, WM_LBUTTONUP, WM_NCHITTEST, WM_PAINT,
            WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
        },
    },
};

const WIDTH: i32 = 42;
const HEIGHT: i32 = 42;
const HIT_RADIUS: i32 = 20;
static HWND_HANDLE: AtomicIsize = AtomicIsize::new(0);
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

fn decode_icon_bgra() -> Result<Vec<u8>, String> {
    let decoder = png::Decoder::new(Cursor::new(include_bytes!("../icons/translate-button.png")));
    let mut reader = decoder.read_info().map_err(|error| error.to_string())?;
    let mut rgba = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut rgba)
        .map_err(|error| error.to_string())?;
    if info.width != WIDTH as u32
        || info.height != HEIGHT as u32
        || info.color_type != png::ColorType::Rgba
        || info.bit_depth != png::BitDepth::Eight
    {
        return Err("translate button asset must be a 42x42 RGBA8 PNG".into());
    }

    // UpdateLayeredWindow expects premultiplied BGRA pixels.
    let mut bgra = Vec::with_capacity(info.buffer_size());
    for pixel in rgba[..info.buffer_size()].chunks_exact(4) {
        let alpha = u16::from(pixel[3]);
        let premultiply = |channel: u8| ((u16::from(channel) * alpha + 127) / 255) as u8;
        bgra.extend_from_slice(&[
            premultiply(pixel[2]),
            premultiply(pixel[1]),
            premultiply(pixel[0]),
            pixel[3],
        ]);
    }
    Ok(bgra)
}

unsafe fn apply_layered_icon(hwnd: HWND) -> Result<(), String> {
    let pixels = decode_icon_bgra()?;
    let bitmap_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: WIDTH,
            biHeight: -HEIGHT,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            biSizeImage: pixels.len() as u32,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut c_void = std::ptr::null_mut();
    let bitmap = CreateDIBSection(None, &bitmap_info, DIB_RGB_COLORS, &mut bits, None, 0)
        .map_err(|error| error.to_string())?;
    if bits.is_null() {
        let _ = DeleteObject(bitmap.into());
        return Err("CreateDIBSection returned an empty pixel buffer".into());
    }
    std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits.cast::<u8>(), pixels.len());

    let memory_dc = CreateCompatibleDC(None);
    if memory_dc.0.is_null() {
        let _ = DeleteObject(bitmap.into());
        return Err("CreateCompatibleDC failed".into());
    }
    let previous = SelectObject(memory_dc, bitmap.into());
    let destination = POINT { x: 0, y: 0 };
    let source = POINT { x: 0, y: 0 };
    let size = SIZE {
        cx: WIDTH,
        cy: HEIGHT,
    };
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    let result = UpdateLayeredWindow(
        hwnd,
        None,
        Some(&destination),
        Some(&size),
        Some(memory_dc),
        Some(&source),
        COLORREF(0),
        Some(&blend),
        ULW_ALPHA,
    )
    .map_err(|error| error.to_string());
    let _ = SelectObject(memory_dc, previous);
    let _ = DeleteObject(bitmap.into());
    let _ = DeleteDC(memory_dc);
    result
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCHITTEST => {
            let screen_x = (lparam.0 as u32 & 0xffff) as u16 as i16 as i32;
            let screen_y = ((lparam.0 as u32 >> 16) & 0xffff) as u16 as i16 as i32;
            let mut rect = RECT::default();
            let _ = GetWindowRect(hwnd, &mut rect);
            let local_x = screen_x - rect.left - WIDTH / 2;
            let local_y = screen_y - rect.top - HEIGHT / 2;
            if local_x * local_x + local_y * local_y <= HIT_RADIUS * HIT_RADIUS {
                LRESULT(HTCLIENT as isize)
            } else {
                LRESULT(HTTRANSPARENT as isize)
            }
        }
        WM_LBUTTONUP => {
            if let Some(app) = APP_HANDLE.get() {
                let _ = app.emit("native-translate-click", ());
            }
            LRESULT(0)
        }
        WM_PAINT => {
            // Layered window pixels are supplied by UpdateLayeredWindow.
            let mut paint = PAINTSTRUCT::default();
            let _ = BeginPaint(hwnd, &mut paint);
            let _ = EndPaint(hwnd, &paint);
            LRESULT(0)
        }
        WM_DESTROY => {
            HWND_HANDLE.store(0, Ordering::Release);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

pub fn start(app: AppHandle) {
    let _ = APP_HANDLE.set(app);
    std::thread::spawn(|| unsafe {
        let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let Ok(instance) = GetModuleHandleW(None) else {
            return;
        };
        let class_name = w!("LumaTranslateNativeButton");
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hInstance: instance.into(),
            lpszClassName: class_name,
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return;
        }
        let Ok(hwnd) = CreateWindowExW(
            WINDOW_EX_STYLE(
                WS_EX_LAYERED.0 | WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0 | WS_EX_TOPMOST.0,
            ),
            class_name,
            w!(""),
            WS_POPUP,
            0,
            0,
            WIDTH,
            HEIGHT,
            None,
            None,
            Some(instance.into()),
            None,
        ) else {
            return;
        };
        if let Err(error) = apply_layered_icon(hwnd) {
            crate::selection::diagnostic(&format!(
                "button status=layered_icon_failed error={error}"
            ));
            return;
        }
        HWND_HANDLE.store(hwnd.0 as isize, Ordering::Release);

        let mut message = MSG::default();
        loop {
            if PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                if message.message == WM_DESTROY {
                    break;
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            } else {
                std::thread::sleep(std::time::Duration::from_millis(4));
            }
        }
    });
}

pub fn show(x: i32, y: i32) -> Option<(i32, i32, i32, i32, bool)> {
    let hwnd = HWND(HWND_HANDLE.load(Ordering::Acquire) as *mut _);
    if hwnd.0.is_null() {
        return None;
    }
    unsafe {
        let previous = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            x,
            y,
            WIDTH,
            HEIGHT,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        let _ = SetThreadDpiAwarenessContext(previous);
        let mut rect = RECT::default();
        let _ = GetWindowRect(hwnd, &mut rect);
        Some((
            rect.left,
            rect.top,
            rect.right,
            rect.bottom,
            IsWindowVisible(hwnd).as_bool(),
        ))
    }
}

pub fn hide() {
    let hwnd = HWND(HWND_HANDLE.load(Ordering::Acquire) as *mut _);
    if hwnd.0.is_null() {
        return;
    }
    unsafe {
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
}

pub fn contains(x: i32, y: i32) -> bool {
    let hwnd = HWND(HWND_HANDLE.load(Ordering::Acquire) as *mut _);
    if hwnd.0.is_null() {
        return false;
    }
    unsafe {
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return false;
        }
        let local_x = x - rect.left - WIDTH / 2;
        let local_y = y - rect.top - HEIGHT / 2;
        local_x * local_x + local_y * local_y <= HIT_RADIUS * HIT_RADIUS
    }
}
