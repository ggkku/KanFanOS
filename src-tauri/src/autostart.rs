//! Windows 开机自启动：写入 HKCU 的 Run 项，不依赖额外插件。
//!
//! 发布版在启动时用 `sync` 把 Run 项修复到当前 exe；调试版不碰注册表，
//! 避免把 `target\debug\kanfanos.exe` 写进开机启动。

use std::slice;

use anyhow::{bail, Context, Result};
use windows::{
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
        System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RegCloseKey,
            RegDeleteValueW, RegOpenKeyExW, RegSetValueExW,
        },
    },
    core::w,
};

/// 按配置写入或删除开机启动项；发布版启动时调用可修正 exe 路径。
pub fn sync(enabled: bool) -> Result<()> {
    if cfg!(debug_assertions) {
        return Ok(());
    }

    set_enabled(enabled)
}

pub fn set_enabled(enabled: bool) -> Result<()> {
    let mut key = HKEY::default();
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Run"),
            None,
            KEY_SET_VALUE,
            &mut key,
        )
    };
    if status != ERROR_SUCCESS {
        bail!("打开开机启动注册表失败（错误码 {}）", status.0);
    }

    let result = if enabled {
        write_command(key)
    } else {
        delete_command(key)
    };

    unsafe {
        let _ = RegCloseKey(key);
    }

    result
}

fn write_command(key: HKEY) -> Result<()> {
    let exe = std::env::current_exe().context("获取程序路径失败")?;
    let command = format!("\"{}\"", exe.display());
    let wide: Vec<u16> = command.encode_utf16().chain(Some(0)).collect();
    let bytes = unsafe {
        slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2)
    };

    let status = unsafe {
        RegSetValueExW(key, w!("KanFanOS"), None, REG_SZ, Some(bytes))
    };
    if status != ERROR_SUCCESS {
        bail!("写入开机启动失败（错误码 {}）", status.0);
    }
    Ok(())
}

fn delete_command(key: HKEY) -> Result<()> {
    let status = unsafe { RegDeleteValueW(key, w!("KanFanOS")) };
    if status != ERROR_SUCCESS && status != ERROR_FILE_NOT_FOUND {
        bail!("删除开机启动失败（错误码 {}）", status.0);
    }
    Ok(())
}
