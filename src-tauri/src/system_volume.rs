//! Windows 系统主音量控制（默认播放设备）。

use anyhow::{Context, Result};
use windows::Win32::{
    Foundation::RPC_E_CHANGED_MODE,
    Media::Audio::{
        Endpoints::IAudioEndpointVolume, IMMDeviceEnumerator, MMDeviceEnumerator, eConsole, eRender,
    },
    System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    },
};

struct ComGuard(bool);

impl ComGuard {
    fn new() -> Result<Self> {
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if hr.is_ok() {
            Ok(Self(true))
        } else if hr == RPC_E_CHANGED_MODE {
            // 当前线程已经用其他 apartment 初始化过 COM，可以直接使用。
            Ok(Self(false))
        } else {
            hr.ok().context("初始化系统音量 COM 失败")?;
            Ok(Self(false))
        }
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

fn endpoint() -> Result<IAudioEndpointVolume> {
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
            .context("创建系统音频设备枚举器失败")?;
    let device = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }
        .context("获取默认播放设备失败")?;
    unsafe { device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) }
        .context("打开系统音量接口失败")
}

pub fn get() -> Result<(u32, bool)> {
    let _com = ComGuard::new()?;
    let endpoint = endpoint()?;
    let scalar = unsafe { endpoint.GetMasterVolumeLevelScalar() }
        .context("读取系统音量失败")?;
    let muted = unsafe { endpoint.GetMute() }
        .context("读取系统静音状态失败")?
        .as_bool();
    Ok(((scalar * 100.0).round().clamp(0.0, 100.0) as u32, muted))
}

pub fn set_level(level: u32) -> Result<()> {
    let _com = ComGuard::new()?;
    let endpoint = endpoint()?;
    let scalar = level.min(100) as f32 / 100.0;
    unsafe { endpoint.SetMasterVolumeLevelScalar(scalar, std::ptr::null()) }
        .context("设置系统音量失败")
}

pub fn set_muted(muted: bool) -> Result<()> {
    let _com = ComGuard::new()?;
    let endpoint = endpoint()?;
    unsafe { endpoint.SetMute(muted, std::ptr::null()) }.context("设置系统静音失败")
}


#[cfg(test)]
mod tests {
    #[test]
    fn reads_and_restores_default_endpoint() {
        let (level, muted) = super::get().expect("读取默认播放设备音量");
        assert!(level <= 100);
        super::set_level(level).expect("写回默认播放设备音量");
        if muted {
            super::set_muted(true).expect("恢复默认播放设备静音状态");
        }
    }
}
