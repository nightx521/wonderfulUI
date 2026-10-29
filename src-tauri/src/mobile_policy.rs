//! In-memory pairing and revocation for the opt-in LAN library.
use crate::mobile_trust::{token_hash, SharedTrust, TrustStore, TrustedDevice};
use base64::Engine;
use rand::RngCore;
use serde::Serialize;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

pub const MOBILE_PORT: u16 = 22358;
pub const MOBILE_RULE: &str = "WonderfulUI Mobile Access";
pub const MOBILE_HELPER: &str = "--wui-mobile-firewall-helper";
const PAIR_TTL: Duration = Duration::from_secs(60);
const SESSION_TTL: Duration = Duration::from_secs(12 * 60 * 60);
pub const COOKIE: &str = "wui_mobile";
const MAX_DEVICES: usize = 4;

pub fn random_token() -> String {
    let mut bytes = [0; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub ip: String,
    pub connected: bool,
    pub remembered: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remaining_seconds: Option<u64>,
}
#[derive(Clone)]
pub struct Grant {
    pub active: Arc<AtomicBool>,
    pub until: Instant,
}

struct Device {
    info: DeviceInfo,
    token: String,
    until: Instant,
    active: Arc<AtomicBool>,
}
struct Pending {
    info: DeviceInfo,
    secret: String,
    until: Instant,
    outcome: Option<bool>,
}
pub struct Pairing {
    pub invite: String,
    invite_until: Instant,
    pending: HashMap<String, Pending>,
    devices: HashMap<String, Device>,
    attempts: HashMap<IpAddr, (Instant, usize)>,
    pub enabled: bool,
    pub trust: SharedTrust,
}

impl Default for Pairing {
    fn default() -> Self {
        Self {
            invite: random_token(),
            invite_until: Instant::now() + Duration::from_secs(600),
            pending: HashMap::new(),
            devices: HashMap::new(),
            attempts: HashMap::new(),
            enabled: true,
            trust: Arc::new(Mutex::new(TrustStore::default())),
        }
    }
}
impl Pairing {
    fn prune(&mut self) {
        let now = Instant::now();
        self.pending.retain(|_, p| p.until > now);
        self.devices.retain(|_, d| {
            if d.until <= now {
                d.active.store(false, Ordering::Release);
                false
            } else {
                true
            }
        });
        self.attempts
            .retain(|_, (at, _)| now.duration_since(*at) < Duration::from_secs(60));
    }
    pub fn rotate(&mut self) {
        self.invite = random_token();
        self.invite_until = Instant::now() + Duration::from_secs(600);
    }
    pub fn infos(&mut self) -> (Vec<DeviceInfo>, Vec<DeviceInfo>) {
        self.prune();
        let trust = self.trust.lock().unwrap_or_else(|e| e.into_inner());
        let mut pending: Vec<_> = self
            .pending
            .values()
            .filter(|p| p.outcome.is_none())
            .collect();
        pending.sort_by_key(|p| p.until);
        let pending = pending
            .into_iter()
            .map(|p| {
                let mut info = p.info.clone();
                info.remaining_seconds = Some(
                    p.until
                        .saturating_duration_since(Instant::now())
                        .as_secs()
                        .saturating_add(1)
                        .min(60),
                );
                info
            })
            .collect();
        let mut devices: Vec<_> = self
            .devices
            .values()
            .map(|d| {
                let mut info = d.info.clone();
                info.remembered = trust.devices().iter().any(|t| t.id == info.id);
                info
            })
            .collect();
        for d in trust.devices() {
            if !devices.iter().any(|a| a.id == d.id) {
                devices.push(DeviceInfo {
                    id: d.id.clone(),
                    name: d.name.clone(),
                    ip: String::new(),
                    connected: false,
                    remembered: true,
                    remaining_seconds: None,
                });
            }
        }
        devices.sort_by(|a, b| a.id.cmp(&b.id));
        (pending, devices)
    }
    pub fn begin(
        &mut self,
        invite: &str,
        name: &str,
        ip: IpAddr,
    ) -> Result<(String, String), &'static str> {
        self.prune();
        if !self.enabled {
            return Err("手机访问已关闭");
        }
        if self.attempts.len() >= 128 && !self.attempts.contains_key(&ip) {
            return Err("连接请求过多，请稍后重试");
        }
        let attempt = self.attempts.entry(ip).or_insert((Instant::now(), 0));
        attempt.1 += 1;
        if attempt.1 > 10 {
            return Err("尝试过于频繁，请稍后重试");
        }
        if self.invite_until <= Instant::now() || invite.len() != 43 || invite != self.invite {
            return Err("二维码已失效，请在电脑上刷新二维码");
        }
        if self.pending.len() >= 8 || self.infos().1.len() >= MAX_DEVICES {
            return Err("连接数量已达上限，请在电脑上断开闲置设备");
        }
        let id = random_token();
        let secret = random_token();
        let name: String = name.chars().filter(|c| !c.is_control()).take(40).collect();
        let info = DeviceInfo {
            id: id.clone(),
            name: if name.trim().is_empty() {
                "手机浏览器".into()
            } else {
                name
            },
            ip: ip.to_string(),
            connected: false,
            remembered: false,
            remaining_seconds: None,
        };
        self.pending.insert(
            id.clone(),
            Pending {
                info,
                secret: secret.clone(),
                until: Instant::now() + PAIR_TTL,
                outcome: None,
            },
        );
        // One scan claims this invitation; existing device sessions are unaffected.
        self.rotate();
        Ok((id, secret))
    }
    pub fn decide(&mut self, id: &str, allow: bool) -> Result<(), String> {
        self.prune();
        if !self.enabled {
            return Err("手机访问已关闭".into());
        }
        if allow && self.infos().1.len() >= MAX_DEVICES {
            return Err("连接数量已达上限".into());
        }
        let pending = self.pending.get_mut(id).ok_or("连接请求已过期")?;
        if pending.outcome.is_some() {
            return Err("连接请求已处理".into());
        }
        if allow {
            let token = random_token();
            self.trust
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .add(TrustedDevice {
                    id: pending.info.id.clone(),
                    name: pending.info.name.clone(),
                    token_hash: token_hash(&token),
                })?;
            let mut info = pending.info.clone();
            info.connected = true;
            self.devices.insert(
                pending.info.id.clone(),
                Device {
                    info,
                    token,
                    until: Instant::now() + SESSION_TTL,
                    active: Arc::new(AtomicBool::new(true)),
                },
            );
        }
        pending.outcome = Some(allow);
        Ok(())
    }
    pub fn poll(
        &mut self,
        id: &str,
        secret: &str,
        ip: IpAddr,
    ) -> Result<Option<String>, &'static str> {
        self.prune();
        if !self.enabled {
            return Err("手机访问已关闭");
        }
        let p = self.pending.get(id).ok_or("连接请求已过期，请重新扫码")?;
        if p.secret != secret || p.info.ip != ip.to_string() {
            return Err("连接凭证无效");
        }
        match p.outcome {
            None => Ok(None),
            Some(false) => Err("电脑已拒绝连接"),
            Some(true) => {
                let device = self
                    .devices
                    .get(id)
                    .filter(|d| d.active.load(Ordering::Acquire))
                    .ok_or("连接已断开，请重新扫码")?;
                Ok(Some(device.token.clone()))
            }
        }
    }
    pub fn authorize(&mut self, token: &str, ip: IpAddr) -> Option<Grant> {
        self.prune();
        if !self.enabled || token.len() != 43 {
            return None;
        }
        if let Some(grant) = self
            .devices
            .values()
            .find(|d| {
                d.token == token && d.info.ip == ip.to_string() && d.active.load(Ordering::Acquire)
            })
            .map(|d| Grant {
                active: d.active.clone(),
                until: d.until,
            })
        {
            return Some(grant);
        }
        // A remembered browser proves possession of its credential, not its IP/name.
        let trusted = self
            .trust
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .devices()
            .iter()
            .find(|d| d.token_hash == token_hash(token))
            .cloned()?;
        if let Some(previous) = self.devices.remove(&trusted.id) {
            previous.active.store(false, Ordering::Release);
        }
        let active = Arc::new(AtomicBool::new(true));
        let until = Instant::now() + SESSION_TTL;
        self.devices.insert(
            trusted.id.clone(),
            Device {
                info: DeviceInfo {
                    id: trusted.id,
                    name: trusted.name,
                    ip: ip.to_string(),
                    connected: true,
                    remembered: true,
                    remaining_seconds: None,
                },
                token: token.into(),
                until,
                active: active.clone(),
            },
        );
        Some(Grant { active, until })
    }
    pub fn revoke(&mut self, id: &str) -> Result<(), String> {
        self.trust
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .revoke(id)?;
        if let Some(d) = self.devices.remove(id) {
            d.active.store(false, Ordering::Release);
        }
        self.pending.remove(id);
        Ok(())
    }
    pub fn configure_remember(&mut self, remember: bool) -> Result<(), String> {
        let mut trust = self.trust.lock().unwrap_or_else(|e| e.into_inner());
        let mut devices = trust.devices().to_vec();
        for d in self.devices.values() {
            if !devices.iter().any(|t| t.id == d.info.id) {
                devices.push(TrustedDevice {
                    id: d.info.id.clone(),
                    name: d.info.name.clone(),
                    token_hash: token_hash(&d.token),
                });
            }
        }
        trust.configure(remember, devices)
    }
    pub fn close(&mut self) {
        self.enabled = false;
        for d in self.devices.values() {
            d.active.store(false, Ordering::Release)
        }
        self.devices.clear();
        self.pending.clear();
        self.invite.clear();
    }
}

/// Single byte ranges, including suffix/open-ended ranges. Never underflow or
/// allocate the requested range; unsupported/malformed requests return 416.
pub fn byte_range(header: Option<&str>, length: u64) -> Result<(u64, u64, bool), ()> {
    let Some(raw) = header else {
        return Ok((0, length, false));
    };
    if raw.len() > 128 || length == 0 {
        return Err(());
    }
    let pair = raw.strip_prefix("bytes=").ok_or(())?;
    let (start, end) = pair.split_once('-').ok_or(())?;
    let number = |s: &str| {
        if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) {
            s.parse::<u64>().map_err(|_| ())
        } else {
            Err(())
        }
    };
    if start.is_empty() {
        let n = number(end)?;
        if n == 0 {
            return Err(());
        }
        let n = n.min(length);
        return Ok((length - n, n, true));
    }
    let start = number(start)?;
    if start >= length {
        return Err(());
    }
    let end = if end.is_empty() {
        length - 1
    } else {
        number(end)?.min(length - 1)
    };
    if end < start {
        return Err(());
    }
    Ok((start, end - start + 1, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pairing_requires_desktop_approval_and_binds_device_identity() {
        let ip = "192.168.1.2".parse().unwrap();
        let other = "192.168.1.3".parse().unwrap();
        let mut p = Pairing::default();
        let invite = p.invite.clone();
        let (id, secret) = p.begin(&invite, "Phone", ip).unwrap();
        assert_ne!(p.invite, invite);
        assert!(p.begin(&invite, "Phone", ip).is_err());
        assert_eq!(p.poll(&id, &secret, ip).unwrap(), None);
        assert!(p.poll(&id, "wrong", ip).is_err());
        assert!(p.poll(&id, &secret, other).is_err());
        p.decide(&id, true).unwrap();
        let token = p.poll(&id, &secret, ip).unwrap().unwrap();
        assert!(p.authorize(&token, other).is_none());
        let active = p.authorize(&token, ip).unwrap();
        p.revoke(&id).unwrap();
        assert!(!active.active.load(Ordering::Acquire));
        assert!(p.authorize(&token, ip).is_none());
        assert!(p.poll(&id, &secret, ip).is_err());
    }
    #[test]
    fn close_denial_expiry_and_rate_limits_reject_access() {
        let ip = "192.168.1.2".parse().unwrap();
        let mut p = Pairing::default();
        let (id, s) = p.begin(&p.invite.clone(), "Phone", ip).unwrap();
        p.decide(&id, false).unwrap();
        assert!(p.poll(&id, &s, ip).is_err());
        let (id, s) = p.begin(&p.invite.clone(), "Phone", ip).unwrap();
        p.pending.get_mut(&id).unwrap().until = Instant::now();
        assert!(p.poll(&id, &s, ip).is_err());
        for _ in 0..12 {
            let _ = p.begin("wrong", "Phone", ip);
        }
        assert!(p.begin(&p.invite.clone(), "Phone", ip).is_err());
        p.close();
        assert!(!p.enabled);
        assert!(p.authorize(&random_token(), ip).is_none());
    }

    #[test]
    fn an_expired_device_invalidates_existing_stream_grants() {
        let ip = "192.168.1.2".parse().unwrap();
        let mut p = Pairing::default();
        let (id, secret) = p.begin(&p.invite.clone(), "Phone", ip).unwrap();
        p.decide(&id, true).unwrap();
        let token = p.poll(&id, &secret, ip).unwrap().unwrap();
        let grant = p.authorize(&token, ip).unwrap();
        p.devices.get_mut(&id).unwrap().until = Instant::now();
        assert!(p.authorize(&token, ip).is_none());
        assert!(!grant.active.load(Ordering::Acquire));
    }
    #[test]
    fn remembered_browser_reconnects_after_restart_and_revoke_is_durable() {
        let ip = "192.168.1.2".parse().unwrap();
        let new_ip = "192.168.1.3".parse().unwrap();
        let mut first = Pairing::default();
        first.configure_remember(true).unwrap();
        let trust = first.trust.clone();
        let (id, secret) = first.begin(&first.invite.clone(), "Phone", ip).unwrap();
        first.decide(&id, true).unwrap();
        let token = first.poll(&id, &secret, ip).unwrap().unwrap();
        let stream = first.authorize(&token, ip).unwrap();
        first.close();
        assert!(!stream.active.load(Ordering::Acquire));
        let mut next = Pairing {
            trust: trust.clone(),
            ..Pairing::default()
        };
        assert_eq!(next.infos().1[0].connected, false);
        assert!(next.authorize(&random_token(), ip).is_none());
        assert!(next.authorize(&token, new_ip).is_some());
        next.revoke(&id).unwrap();
        next.close();
        let mut restarted = Pairing {
            trust,
            ..Pairing::default()
        };
        assert!(restarted.authorize(&token, ip).is_none());
    }
    #[test]
    fn sixty_second_deadline_cannot_be_extended_by_polling_or_desktop_approval() {
        let ip = "192.168.1.2".parse().unwrap();
        let mut p = Pairing::default();
        let (id, secret) = p.begin(&p.invite.clone(), "Phone", ip).unwrap();
        assert_eq!(p.infos().0[0].remaining_seconds, Some(60));
        let deadline = p.pending[&id].until;
        p.poll(&id, &secret, ip).unwrap();
        assert_eq!(p.pending[&id].until, deadline);
        p.pending.get_mut(&id).unwrap().until = Instant::now();
        assert!(p.decide(&id, true).is_err());
        assert!(p.poll(&id, &secret, ip).is_err());
        assert!(p.infos().0.is_empty());
        assert!(p.infos().1.is_empty());
    }
    #[test]
    fn range_supports_seek_suffix_empty_and_boundary_cases() {
        assert_eq!(byte_range(None, 0), Ok((0, 0, false)));
        assert_eq!(byte_range(Some("bytes=2-4"), 10), Ok((2, 3, true)));
        assert_eq!(byte_range(Some("bytes=6-"), 10), Ok((6, 4, true)));
        assert_eq!(byte_range(Some("bytes=-3"), 10), Ok((7, 3, true)));
        assert_eq!(byte_range(Some("bytes=0-99"), 10), Ok((0, 10, true)));
        for s in [
            "bytes=10-",
            "bytes=9-8",
            "bytes=-0",
            "bytes=0-1,3-4",
            "bytes=+2-3",
            "items=0-1",
            "bytes=18446744073709551616-",
        ] {
            assert!(byte_range(Some(s), 10).is_err(), "{s}")
        }
        assert!(byte_range(Some("bytes=0-"), 0).is_err());
    }
}
