use serde::{Deserialize, Serialize};
use std::{os::windows::io::AsRawHandle, path::Path, process::Child};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::JobObjects::*,
};
use winreg::{enums::*, RegKey, RegValue};
const INTERNET: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings";
#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
pub struct RegistryValue {
    pub kind: u32,
    pub bytes: Vec<u8>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Backup {
    pub values: Vec<(String, Option<RegistryValue>)>,
    pub applied: String,
}
fn capture(key: &RegKey, name: &str) -> Option<RegistryValue> {
    key.get_raw_value(name).ok().map(|v| RegistryValue {
        kind: v.vtype as u32,
        bytes: v.bytes,
    })
}
fn restore_values(key: &RegKey, backup: &Backup) -> Result<(), String> {
    for (name, value) in &backup.values {
        match value {
            Some(v) => {
                let vtype = match v.kind {
                    1 => REG_SZ,
                    2 => REG_EXPAND_SZ,
                    3 => REG_BINARY,
                    4 => REG_DWORD,
                    _ => return Err("不支持的代理注册表类型".into()),
                };
                key.set_raw_value(
                    name,
                    &RegValue {
                        bytes: v.bytes.clone(),
                        vtype,
                    },
                )
                .map_err(|e| e.to_string())?;
            }
            None => match key.delete_value(name) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.to_string()),
            },
        }
    }
    Ok(())
}
pub fn should_restore(current: &str, applied: &str) -> bool {
    current.eq_ignore_ascii_case(applied)
}
fn notify() {
    unsafe {
        windows_sys::Win32::Networking::WinInet::InternetSetOptionW(
            std::ptr::null_mut(),
            39,
            std::ptr::null(),
            0,
        );
        windows_sys::Win32::Networking::WinInet::InternetSetOptionW(
            std::ptr::null_mut(),
            37,
            std::ptr::null(),
            0,
        );
    }
}
pub fn enable(root: &Path, http: u16, socks: u16) -> Result<(), String> {
    let path = root.join("system-proxy-backup.json");
    if path.exists() {
        return Err("仍有待恢复的系统代理备份，请先断开/恢复".into());
    }
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(INTERNET, KEY_READ | KEY_WRITE)
        .map_err(|e| e.to_string())?;
    let applied = format!("http=127.0.0.1:{http};https=127.0.0.1:{http};socks=127.0.0.1:{socks}");
    let backup = Backup {
        values: [
            "ProxyEnable",
            "ProxyServer",
            "ProxyOverride",
            "AutoConfigURL",
        ]
        .iter()
        .map(|name| (name.to_string(), capture(&key, name)))
        .collect(),
        applied: applied.clone(),
    };
    crate::storage::atomic_write(
        &path,
        &serde_json::to_vec(&backup).map_err(|e| e.to_string())?,
    )?;
    let write = (|| -> std::io::Result<()> {
        key.set_value("ProxyEnable", &1u32)?;
        key.set_value("ProxyServer", &applied)?;
        key.set_value("ProxyOverride",&"localhost;127.*;10.*;192.168.*;172.16.*;172.17.*;172.18.*;172.19.*;172.20.*;172.21.*;172.22.*;172.23.*;172.24.*;172.25.*;172.26.*;172.27.*;172.28.*;172.29.*;172.30.*;172.31.*;<local>")?;
        if key.get_raw_value("AutoConfigURL").is_ok() {
            key.delete_value("AutoConfigURL")?;
        }
        Ok(())
    })();
    if let Err(e) = write {
        restore_values(&key, &backup)?;
        let _ = std::fs::remove_file(path);
        notify();
        return Err(e.to_string());
    }
    notify();
    Ok(())
}

pub fn proxy_matches(http: u16, socks: u16) -> Result<bool, String> {
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(INTERNET, KEY_READ)
        .map_err(|e| e.to_string())?;
    let enabled: u32 = key.get_value("ProxyEnable").unwrap_or(0);
    let current: String = key.get_value("ProxyServer").unwrap_or_default();
    let expected = format!("http=127.0.0.1:{http};https=127.0.0.1:{http};socks=127.0.0.1:{socks}");
    Ok(enabled == 1 && current.eq_ignore_ascii_case(&expected))
}
pub fn restore(root: &Path) -> Result<bool, String> {
    let path = root.join("system-proxy-backup.json");
    if !path.exists() {
        return Ok(false);
    }
    let backup: Backup = serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?)
        .map_err(|_| "系统代理备份损坏，保留文件以便恢复。")?;
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(INTERNET, KEY_READ | KEY_WRITE)
        .map_err(|e| e.to_string())?;
    let current: String = key.get_value("ProxyServer").unwrap_or_default();
    let owned = should_restore(&current, &backup.applied);
    if owned {
        restore_values(&key, &backup)?;
        notify();
    }
    std::fs::remove_file(path).map_err(|e| e.to_string())?;
    Ok(owned)
}
pub fn autostart(enabled: bool) -> Result<(), String> {
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
        .map_err(|e| e.to_string())?;
    if enabled {
        let path = std::env::current_exe().map_err(|e| e.to_string())?;
        key.set_value("LumaRoute", &format!("\"{}\"", path.display()))
            .map_err(|e| e.to_string())?;
    } else if key.get_raw_value("LumaRoute").is_ok() {
        key.delete_value("LumaRoute").map_err(|e| e.to_string())?;
    }
    Ok(())
}

// Closing this handle kills only core processes assigned to this application.
pub struct ChildJob(HANDLE);
unsafe impl Send for ChildJob {}
unsafe impl Sync for ChildJob {}
impl ChildJob {
    pub fn new() -> Result<Self, String> {
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of_val(&info) as u32,
            ) == 0
            {
                CloseHandle(job);
                return Err(std::io::Error::last_os_error().to_string());
            }
            Ok(Self(job))
        }
    }
    pub fn assign(&self, child: &Child) -> Result<(), String> {
        unsafe {
            if AssignProcessToJobObject(self.0, child.as_raw_handle() as HANDLE) == 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
        }
        Ok(())
    }
}
impl Drop for ChildJob {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn never_overwrite_another_apps_proxy() {
        assert!(should_restore("HTTP=127.0.0.1:7890", "http=127.0.0.1:7890"));
        assert!(!should_restore("127.0.0.1:8888", "127.0.0.1:7890"));
    }
}
