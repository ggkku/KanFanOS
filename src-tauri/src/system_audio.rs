//! Windows 播放设备枚举与默认设备切换。
//!
//! 播放器里的「音频输出」直接切换系统的默认播放设备；主音量接口
//! （`system_volume`）控制的也是默认设备，所以两者始终指向同一个设备。

// COM 接口方法名沿用系统定义（PascalCase）。
#![allow(non_snake_case)]

use anyhow::{Context, Result};
use serde::Serialize;
use windows::{
    core::{interface, BOOL, GUID, HRESULT, PCWSTR},
    Win32::{
        Devices::FunctionDiscovery::PKEY_Device_FriendlyName,
        Foundation::{PROPERTYKEY, RPC_E_CHANGED_MODE},
        Media::Audio::{
            eCommunications, eConsole, eMultimedia, eRender, ERole, IMMDevice,
            IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE, WAVEFORMATEX,
        },
        System::Com::{
            CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, StructuredStorage,
            CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ,
        },
        UI::Shell::PropertiesSystem::IPropertyStore,
    },
};

struct ComGuard(bool);

impl ComGuard {
    fn new() -> Result<Self> {
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if hr.is_ok() {
            Ok(Self(true))
        } else if hr == RPC_E_CHANGED_MODE {
            Ok(Self(false))
        } else {
            hr.ok().context("初始化系统音频 COM 失败")?;
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

/// 未公开但广泛使用的默认播放设备切换接口（PolicyConfig）。
const CLSID_POLICY_CONFIG: GUID =
    GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

#[interface("f8679f50-850a-41cf-9c72-430f290290c8")]
unsafe trait IPolicyConfig: windows::core::IUnknown {
    fn GetMixFormat(
        &self,
        device_id: PCWSTR,
        format: *mut *mut WAVEFORMATEX,
    ) -> HRESULT;
    fn GetDeviceFormat(
        &self,
        device_id: PCWSTR,
        default: BOOL,
        format: *mut *mut WAVEFORMATEX,
    ) -> HRESULT;
    fn ResetDeviceFormat(&self, device_id: PCWSTR) -> HRESULT;
    fn SetDeviceFormat(
        &self,
        device_id: PCWSTR,
        endpoint_format: *const WAVEFORMATEX,
        mix_format: *const WAVEFORMATEX,
    ) -> HRESULT;
    fn GetProcessingPeriod(
        &self,
        device_id: PCWSTR,
        default: BOOL,
        period: *mut i64,
        min_period: *mut i64,
    ) -> HRESULT;
    fn SetProcessingPeriod(&self, device_id: PCWSTR, period: *const i64) -> HRESULT;
    fn GetShareMode(&self, device_id: PCWSTR, mode: *mut i32) -> HRESULT;
    fn SetShareMode(&self, device_id: PCWSTR, mode: *const i32) -> HRESULT;
    fn GetPropertyValue(
        &self,
        device_id: PCWSTR,
        key: *const PROPERTYKEY,
        value: *mut StructuredStorage::PROPVARIANT,
    ) -> HRESULT;
    fn SetPropertyValue(
        &self,
        device_id: PCWSTR,
        key: *const PROPERTYKEY,
        value: *const StructuredStorage::PROPVARIANT,
    ) -> HRESULT;
    fn SetDefaultEndpoint(&self, device_id: PCWSTR, role: ERole) -> HRESULT;
    fn SetEndpointVisibility(&self, device_id: PCWSTR, visible: BOOL) -> HRESULT;
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

fn device_friendly_name(store: &IPropertyStore) -> Option<String> {
    unsafe {
        let value = store.GetValue(&PKEY_Device_FriendlyName).ok()?;
        let allocated = StructuredStorage::PropVariantToStringAlloc(&value).ok();
        let mut value = value;
        let _ = StructuredStorage::PropVariantClear(&mut value);
        let allocated = allocated?;
        let name = allocated.to_string().ok();
        CoTaskMemFree(Some(allocated.0 as *const core::ffi::c_void));
        name
    }
}

fn default_device_id(enumerator: &IMMDeviceEnumerator) -> Option<String> {
    unsafe {
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        let id = device.GetId().ok()?;
        let text = id.to_string().ok();
        CoTaskMemFree(Some(id.0 as *const core::ffi::c_void));
        text
    }
}

fn endpoint_id(device: &IMMDevice) -> Option<String> {
    unsafe {
        let id = device.GetId().ok()?;
        let text = id.to_string().ok();
        CoTaskMemFree(Some(id.0 as *const core::ffi::c_void));
        text
    }
}

/// 列出所有已启用的播放设备，并标记当前的系统默认设备。
pub fn list_devices() -> Result<Vec<OutputDevice>> {
    let _com = ComGuard::new()?;

    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
            .context("创建系统音频设备枚举器失败")?;
    let default_id = default_device_id(&enumerator);

    let collection = unsafe { enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) }
        .context("枚举系统播放设备失败")?;
    let count = unsafe { collection.GetCount() }.context("读取播放设备数量失败")?;

    let mut devices = Vec::with_capacity(count as usize);
    for index in 0..count {
        let Ok(device) = (unsafe { collection.Item(index) }) else {
            continue;
        };
        let Some(id) = endpoint_id(&device) else {
            continue;
        };
        if id.is_empty() {
            continue;
        }
        let name = unsafe { device.OpenPropertyStore(STGM_READ) }
            .ok()
            .and_then(|store| device_friendly_name(&store))
            .unwrap_or_else(|| id.clone());
        devices.push(OutputDevice {
            is_default: default_id.as_deref() == Some(id.as_str()),
            id,
            name,
        });
    }

    Ok(devices)
}

/// 把指定设备设为系统默认播放设备（声音/多媒体/通信三个角色）。
pub fn set_default_device(device_id: &str) -> Result<()> {
    let _com = ComGuard::new()?;

    let policy: IPolicyConfig =
        unsafe { CoCreateInstance(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL) }
            .context("创建系统音频策略接口失败")?;
    let wide: Vec<u16> = device_id.encode_utf16().chain(std::iter::once(0)).collect();
    let id = PCWSTR(wide.as_ptr());

    for role in [eConsole, eMultimedia, eCommunications] {
        unsafe { policy.SetDefaultEndpoint(id, role) }
            .ok()
            .context("设置系统默认播放设备失败")?;
    }

    Ok(())
}

/// 把 Windows 端点 ID 转换成 mpv 的 WASAPI 设备名，让正在播放的 mpv
/// 立即切到同一个设备。端点 ID 形如 `{0.0.0.00000000}.{GUID}`，
/// mpv 的设备名是 `wasapi/{GUID}`。
pub fn mpv_device_name(device_id: &str) -> String {
    match device_id.rsplit_once('.') {
        Some((_, guid)) if guid.starts_with('{') && guid.ends_with('}') => {
            format!("wasapi/{}", guid.to_lowercase())
        }
        _ => "auto".to_string(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn lists_active_render_devices() {
        let devices = super::list_devices().expect("枚举系统播放设备");
        for device in &devices {
            eprintln!(
                "device default={} name={} id={}",
                device.is_default, device.name, device.id
            );
        }
        assert!(!devices.is_empty());
        assert!(devices.iter().any(|device| device.is_default));
    }

    #[test]
    fn sets_current_default_device() {
        let devices = super::list_devices().expect("枚举系统播放设备");
        let default = devices
            .iter()
            .find(|device| device.is_default)
            .expect("存在默认播放设备");
        // 把当前默认设备重新设为默认：验证 IPolicyConfig 可用，且不改变输出。
        super::set_default_device(&default.id).expect("设置系统默认播放设备");
    }

    #[test]
    fn converts_endpoint_id_to_mpv_name() {
        assert_eq!(
            super::mpv_device_name("{0.0.0.00000000}.{63440022-F874-4BA0-9A9A-315AF549AE93}"),
            "wasapi/{63440022-f874-4ba0-9a9a-315af549ae93}"
        );
        assert_eq!(super::mpv_device_name("unexpected"), "auto");
    }
}
