use crate::{config, models::Data};
use std::{fs, io::Write, path::Path};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    },
};
const MAGIC: &[u8] = b"LUMAROUTE-DPAPI-1\0";

fn protect(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len().try_into().map_err(|_| "配置数据过大")?,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let ok = unsafe {
        CryptProtectData(
            &input,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 {
        return Err(format!(
            "Windows DPAPI 加密失败：{}",
            std::io::Error::last_os_error()
        ));
    }
    let encrypted = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) };
    let mut result = Vec::with_capacity(MAGIC.len() + encrypted.len());
    result.extend_from_slice(MAGIC);
    result.extend_from_slice(encrypted);
    unsafe { LocalFree(output.pbData as *mut core::ffi::c_void) };
    Ok(result)
}

fn unprotect(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let encrypted = bytes.strip_prefix(MAGIC).ok_or("配置不是 DPAPI 加密格式")?;
    let input = CRYPT_INTEGER_BLOB {
        cbData: encrypted.len().try_into().map_err(|_| "配置数据过大")?,
        pbData: encrypted.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let ok = unsafe {
        CryptUnprotectData(
            &input,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 {
        return Err(format!(
            "Windows DPAPI 解密失败：{}",
            std::io::Error::last_os_error()
        ));
    }
    let result =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe { LocalFree(output.pbData as *mut core::ffi::c_void) };
    Ok(result)
}

pub fn decode(bytes: &[u8]) -> Result<Data, String> {
    let plain = if bytes.starts_with(MAGIC) {
        unprotect(bytes)?
    } else {
        bytes.to_vec()
    };
    serde_json::from_slice(&plain)
        .map_err(|_| "配置文件损坏；已保留原文件，请先导出诊断或恢复备份。".into())
}
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("存储目录无效")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    tmp.write_all(bytes)
        .and_then(|_| tmp.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    tmp.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}
pub fn load(root: &Path) -> Result<Data, String> {
    let path = root.join("profile.json");
    if !path.exists() {
        return Ok(Data::default());
    }
    let b = fs::read(path).map_err(|e| e.to_string())?;
    if b.len() > 32 * 1024 * 1024 {
        return Err("配置文件过大".into());
    }
    let mut data = decode(&b)?;
    if data.version == 1 {
        data.settings.capture_mode = if data.settings.legacy_system_proxy.unwrap_or(true) {
            "systemProxy"
        } else {
            "none"
        }
        .into();
        data.settings.legacy_system_proxy = None;
        data.version = 2;
    }
    if data.version != 2 {
        return Err("不支持的配置版本".into());
    }
    config::validate(&data.settings)?;
    Ok(data)
}

pub fn save(root: &Path, data: &Data) -> Result<(), String> {
    let path = root.join("profile.json");
    if path.exists() {
        let bytes = fs::read(&path).map_err(|e| e.to_string())?;
        if let Ok(previous) = decode(&bytes) {
            let plain = serde_json::to_vec_pretty(&previous).map_err(|e| e.to_string())?;
            atomic_write(&root.join("profile.backup.json"), &protect(&plain)?)?;
        }
    }
    let plain = serde_json::to_vec_pretty(data).map_err(|e| e.to_string())?;
    atomic_write(&path, &protect(&plain)?)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_backup_and_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let mut d = Data::default();
        save(dir.path(), &d).unwrap();
        d.settings.http_port = 9090;
        d.subscriptions.push(crate::models::Subscription {
            name: "secret".into(),
            url: "https://example.test/private-token".into(),
            ..Default::default()
        });
        save(dir.path(), &d).unwrap();
        assert_eq!(load(dir.path()).unwrap().settings.http_port, 9090);
        assert!(dir.path().join("profile.backup.json").exists());
        let encrypted = fs::read(dir.path().join("profile.json")).unwrap();
        assert!(encrypted.starts_with(MAGIC));
        assert!(!String::from_utf8_lossy(&encrypted).contains("private-token"));
        fs::write(dir.path().join("profile.json"), b"broken").unwrap();
        assert!(load(dir.path()).is_err());
        assert_eq!(
            fs::read(dir.path().join("profile.json")).unwrap(),
            b"broken"
        );
    }
    #[test]
    fn migrates_legacy_system_proxy_to_capture_mode() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("profile.json"),
            br#"{"version":1,"settings":{"systemProxy":false}}"#,
        )
        .unwrap();
        let data = load(dir.path()).unwrap();
        assert_eq!(data.version, 2);
        assert_eq!(data.settings.capture_mode, "none");
        assert!(data.settings.legacy_system_proxy.is_none());
    }
}
