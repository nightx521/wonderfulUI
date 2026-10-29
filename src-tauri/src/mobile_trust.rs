//! App-owned remembered browser grants. Only SHA-256 credential hashes are persisted.
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

pub type SharedTrust = Arc<Mutex<TrustStore>>;
#[derive(Clone, Serialize, Deserialize)]
pub struct TrustedDevice {
    pub id: String,
    pub name: String,
    pub token_hash: String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
struct Data {
    #[serde(default)]
    enabled: bool,
    remember: bool,
    devices: Vec<TrustedDevice>,
}
#[derive(Default)]
pub struct TrustStore {
    data: Data,
    path: Option<PathBuf>,
}
impl TrustStore {
    pub fn load(path: PathBuf) -> Result<Self, String> {
        let data = match std::fs::read(&path) {
            Ok(bytes) if bytes.len() <= 16 * 1024 => serde_json::from_slice::<Data>(&bytes)
                .map_err(|_| "已记住设备的设置无法读取，请检查本地设置".to_string())?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Data::default(),
            _ => return Err("已记住设备的设置无法读取，请检查本地设置".into()),
        };
        if data.devices.len() > 4
            || data.devices.iter().any(|d| {
                d.id.len() != 43
                    || d.name.chars().count() > 40
                    || d.token_hash.len() != 64
                    || !d.token_hash.bytes().all(|b| b.is_ascii_hexdigit())
            })
            || !data.remember && !data.devices.is_empty()
        {
            return Err("已记住设备的设置无法读取，请检查本地设置".into());
        }
        Ok(Self {
            data,
            path: Some(path),
        })
    }
    fn save(&mut self, next: Data) -> Result<(), String> {
        if let Some(path) = &self.path {
            let write = || -> std::io::Result<()> {
                use std::io::Write;
                std::fs::create_dir_all(path.parent().unwrap())?;
                let temporary = path.with_extension("json.tmp");
                let mut file = std::fs::File::create(&temporary)?;
                file.write_all(&serde_json::to_vec(&next)?)?;
                file.sync_all()?;
                drop(file);
                std::fs::rename(temporary, path)
            };
            write().map_err(|_| "设备授权设置保存失败，请稍后重试".to_string())?;
        }
        self.data = next;
        Ok(())
    }
    pub fn remember(&self) -> bool {
        self.data.remember
    }
    pub fn enabled(&self) -> bool {
        self.data.enabled
    }
    pub fn set_enabled(&mut self, enabled: bool) -> Result<(), String> {
        let mut next = self.data.clone();
        next.enabled = enabled;
        self.save(next)
    }
    pub fn devices(&self) -> &[TrustedDevice] {
        &self.data.devices
    }
    pub fn configure(&mut self, remember: bool, devices: Vec<TrustedDevice>) -> Result<(), String> {
        self.save(Data {
            enabled: self.data.enabled,
            remember,
            devices: if remember { devices } else { vec![] },
        })
    }
    pub fn add(&mut self, device: TrustedDevice) -> Result<(), String> {
        if !self.data.remember {
            return Ok(());
        }
        let mut next = self.data.clone();
        next.devices.retain(|d| d.id != device.id);
        if next.devices.len() >= 4 {
            return Err("连接数量已达上限".into());
        }
        next.devices.push(device);
        self.save(next)
    }
    pub fn revoke(&mut self, id: &str) -> Result<(), String> {
        let mut next = self.data.clone();
        next.devices.retain(|d| d.id != id);
        self.save(next)
    }
}
pub fn token_hash(token: &str) -> String {
    crate::library::sha256_hex(token)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persists_only_hashes_and_revocations_survive_reload() {
        let dir = std::env::temp_dir().join(format!("wui-trust-{}", uuid::Uuid::new_v4()));
        let path = dir.join("mobile-access.json");
        let token = crate::mobile_policy::random_token();
        let id = crate::mobile_policy::random_token();
        let mut store = TrustStore::load(path.clone()).unwrap();
        store.configure(true, vec![]).unwrap();
        store.set_enabled(true).unwrap();
        assert!(TrustStore::load(path.clone()).unwrap().enabled());
        store
            .add(TrustedDevice {
                id: id.clone(),
                name: "Fixture".into(),
                token_hash: token_hash(&token),
            })
            .unwrap();
        assert!(!std::fs::read_to_string(&path).unwrap().contains(&token));
        let mut next = TrustStore::load(path.clone()).unwrap();
        assert_eq!(next.devices().len(), 1);
        next.revoke(&id).unwrap();
        assert!(TrustStore::load(path.clone()).unwrap().devices().is_empty());
        next.configure(false, vec![]).unwrap();
        next.set_enabled(false).unwrap();
        assert!(!TrustStore::load(path.clone()).unwrap().enabled());
        assert!(!TrustStore::load(path).unwrap().remember());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
