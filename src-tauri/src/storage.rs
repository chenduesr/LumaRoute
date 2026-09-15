use crate::{config, models::Data};
use std::{fs, io::Write, path::Path};
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
    let data: Data = serde_json::from_slice(&b)
        .map_err(|_| "配置文件损坏；已保留原文件，请先导出诊断或恢复备份。")?;
    if data.version != 1 {
        return Err("不支持的配置版本".into());
    }
    config::validate(&data.settings)?;
    Ok(data)
}
pub fn save(root: &Path, data: &Data) -> Result<(), String> {
    let path = root.join("profile.json");
    if path.exists() {
        let bytes = fs::read(&path).map_err(|e| e.to_string())?;
        if serde_json::from_slice::<Data>(&bytes).is_ok() {
            atomic_write(&root.join("profile.backup.json"), &bytes)?;
        }
    }
    atomic_write(
        &path,
        &serde_json::to_vec_pretty(data).map_err(|e| e.to_string())?,
    )
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
        save(dir.path(), &d).unwrap();
        assert_eq!(load(dir.path()).unwrap().settings.http_port, 9090);
        assert!(dir.path().join("profile.backup.json").exists());
        fs::write(dir.path().join("profile.json"), b"broken").unwrap();
        assert!(load(dir.path()).is_err());
        assert_eq!(
            fs::read(dir.path().join("profile.json")).unwrap(),
            b"broken"
        );
    }
}
