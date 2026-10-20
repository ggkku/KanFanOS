//! 嵌入视频画面用的 Win32 子窗口。
//!
//! MPV 通过 `--wid` 渲染到这个窗口里；窗口是主窗口的子窗口，
//! 因此始终位于 WebView 之上，但只在视频区域内显示。

use std::{
    ffi::c_void,
    sync::OnceLock,
};

use anyhow::{bail, Result};
use windows::{
    core::w,
    Win32::{
        Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM, HINSTANCE},
        Graphics::Gdi::CreateSolidBrush,
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, MoveWindow, RegisterClassW, SetWindowPos,
            ShowWindow, CS_HREDRAW, CS_VREDRAW, HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE,
            SWP_NOSIZE, SW_HIDE, SW_SHOW, WINDOW_EX_STYLE, WNDCLASSW, WS_CHILD,
            WS_CLIPSIBLINGS,
        },
    },
};

const CLASS_NAME: windows::core::PCWSTR = w!("KanFanOSVideoSurface");

static REGISTERED: OnceLock<u16> = OnceLock::new();

unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// 在主窗口下创建一个隐藏的子窗口，返回 HWND 的整数值。
pub fn create(parent: HWND) -> Result<isize> {
    let module = unsafe { GetModuleHandleW(None)? };
    let instance = HINSTANCE(module.0);

    let atom = *REGISTERED.get_or_init(|| unsafe {
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: instance,
            hbrBackground: CreateSolidBrush(COLORREF(0)),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        RegisterClassW(&class)
    });

    if atom == 0 {
        bail!("注册视频窗口类失败: {}", std::io::Error::last_os_error());
    }

    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            CLASS_NAME,
            w!(""),
            WS_CHILD | WS_CLIPSIBLINGS,
            0,
            0,
            1,
            1,
            Some(parent),
            None,
            Some(instance),
            None,
        )?
    };

    Ok(hwnd.0 as isize)
}

/// 显示并定位视频窗口（坐标是主窗口客户区的物理像素）。
pub fn show(hwnd: isize, x: i32, y: i32, width: i32, height: i32) -> Result<()> {
    let hwnd = HWND(hwnd as *mut c_void);

    unsafe {
        MoveWindow(hwnd, x, y, width.max(1), height.max(1), true)?;
        let _ = ShowWindow(hwnd, SW_SHOW);
        SetWindowPos(
            hwnd,
            Some(HWND_TOP),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )?;
    }

    Ok(())
}

pub fn resize(hwnd: isize, x: i32, y: i32, width: i32, height: i32) -> Result<()> {
    let hwnd = HWND(hwnd as *mut c_void);

    unsafe {
        MoveWindow(hwnd, x, y, width.max(1), height.max(1), true)?;
    }

    Ok(())
}

pub fn hide(hwnd: isize) {
    let hwnd = HWND(hwnd as *mut c_void);
    unsafe {
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
}
