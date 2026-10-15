//! Fixed-port conflict diagnostics; process actions require a fresh native ticket.
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortOwner {
    pub pid: u32,
    pub name: String,
    pub ticket: Option<String>,
    pub reason: Option<String>,
}

#[tauri::command]
pub async fn inspect_share_port() -> Result<Vec<PortOwner>, String> {
    tauri::async_runtime::spawn_blocking(inspect)
        .await
        .map_err(|_| "无法读取端口占用，请重试".to_string())?
}

#[tauri::command]
pub async fn release_share_port(ticket: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || release(&ticket))
        .await
        .map_err(|_| "无法结束占用程序，请重试".to_string())?
}

#[cfg(not(windows))]
fn inspect() -> Result<Vec<PortOwner>, String> {
    Ok(Vec::new())
}
#[cfg(not(windows))]
fn release(_: &str) -> Result<(), String> {
    Err("仅支持 Windows".into())
}
#[cfg(windows)]
use native::{inspect, release};

#[cfg(windows)]
mod native {
    use super::*;
    use std::collections::{BTreeSet, HashMap};
    use std::path::Path;
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};
    use windows::core::{BOOL, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
    use windows::Win32::NetworkManagement::IpHelper::*;
    use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6};
    use windows::Win32::Security::{
        EqualSid, GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER,
    };
    use windows::Win32::System::Threading::*;

    struct Owned(HANDLE);
    impl Drop for Owned {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
    struct Identity {
        path: String,
        created: u64,
        allowed: bool,
    }
    struct Ticket {
        pid: u32,
        path: String,
        created: u64,
        issued: Instant,
    }
    fn tickets() -> &'static Mutex<HashMap<String, Ticket>> {
        static TICKETS: OnceLock<Mutex<HashMap<String, Ticket>>> = OnceLock::new();
        TICKETS.get_or_init(|| Mutex::new(HashMap::new()))
    }
    fn table(family: u32) -> Result<Vec<u64>, String> {
        let mut size = 0;
        unsafe {
            GetExtendedTcpTable(
                None,
                &mut size,
                false,
                family,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            );
        }
        for _ in 0..3 {
            if size == 0 || size > 16 * 1024 * 1024 {
                break;
            }
            let mut buffer = vec![0u64; (size as usize + 7) / 8];
            let result = unsafe {
                GetExtendedTcpTable(
                    Some(buffer.as_mut_ptr().cast()),
                    &mut size,
                    false,
                    family,
                    TCP_TABLE_OWNER_PID_LISTENER,
                    0,
                )
            };
            if result == 0 {
                return Ok(buffer);
            }
            if result != 122 {
                break;
            }
        }
        Err("无法读取端口占用，请稍后重试".into())
    }
    fn owners() -> Result<BTreeSet<u32>, String> {
        let mut result = BTreeSet::new();
        for family in [AF_INET.0 as u32, AF_INET6.0 as u32] {
            let buffer = table(family)?;
            let bytes = buffer.len() * 8;
            let ptr = buffer.as_ptr().cast::<u8>();
            let count = unsafe { std::ptr::read_unaligned(ptr.cast::<u32>()) } as usize;
            let stride = if family == AF_INET.0 as u32 {
                std::mem::size_of::<MIB_TCPROW_OWNER_PID>()
            } else {
                std::mem::size_of::<MIB_TCP6ROW_OWNER_PID>()
            };
            if count > bytes.saturating_sub(4) / stride {
                return Err("端口占用信息无效，请重试".into());
            }
            for i in 0..count {
                let row = unsafe { ptr.add(4 + i * stride) };
                let (port, pid) = unsafe {
                    if family == AF_INET.0 as u32 {
                        let row = std::ptr::read_unaligned(row.cast::<MIB_TCPROW_OWNER_PID>());
                        (row.dwLocalPort, row.dwOwningPid)
                    } else {
                        let row = std::ptr::read_unaligned(row.cast::<MIB_TCP6ROW_OWNER_PID>());
                        (row.dwLocalPort, row.dwOwningPid)
                    }
                };
                if u16::from_be(port as u16) == crate::share_policy::QUICK_SHARE_PORT {
                    result.insert(pid);
                }
            }
        }
        Ok(result)
    }
    fn user_token(process: HANDLE) -> Result<Vec<usize>, ()> {
        unsafe {
            let mut token = HANDLE::default();
            OpenProcessToken(process, TOKEN_QUERY, &mut token).map_err(|_| ())?;
            let token = Owned(token);
            let mut size = 0;
            let _ = GetTokenInformation(token.0, TokenUser, None, 0, &mut size);
            if size == 0 || size > 65536 {
                return Err(());
            }
            let mut buffer = vec![
                0usize;
                (size as usize + std::mem::size_of::<usize>() - 1)
                    / std::mem::size_of::<usize>()
            ];
            GetTokenInformation(
                token.0,
                TokenUser,
                Some(buffer.as_mut_ptr().cast()),
                size,
                &mut size,
            )
            .map_err(|_| ())?;
            Ok(buffer)
        }
    }
    fn identity(process: HANDLE) -> Result<Identity, ()> {
        unsafe {
            let mut name = vec![0u16; 32768];
            let mut size = name.len() as u32;
            QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                PWSTR(name.as_mut_ptr()),
                &mut size,
            )
            .map_err(|_| ())?;
            let path = String::from_utf16_lossy(&name[..size as usize]);
            let mut created = FILETIME::default();
            let mut exit = FILETIME::default();
            let mut kernel = FILETIME::default();
            let mut user = FILETIME::default();
            GetProcessTimes(process, &mut created, &mut exit, &mut kernel, &mut user)
                .map_err(|_| ())?;
            let mut critical = BOOL::default();
            IsProcessCritical(process, &mut critical).map_err(|_| ())?;
            let ours = user_token(GetCurrentProcess())?;
            let theirs = user_token(process)?;
            let same_user = EqualSid(
                (*(ours.as_ptr().cast::<TOKEN_USER>())).User.Sid,
                (*(theirs.as_ptr().cast::<TOKEN_USER>())).User.Sid,
            )
            .is_ok();
            Ok(Identity {
                allowed: !critical.as_bool() && same_user && safe_path(&path),
                path,
                created: (created.dwHighDateTime as u64) << 32 | created.dwLowDateTime as u64,
            })
        }
    }
    fn safe_path(path: &str) -> bool {
        let lower = path.to_lowercase();
        let system = std::env::var("WINDIR")
            .unwrap_or_else(|_| "C:\\Windows".into())
            .to_lowercase();
        if lower.starts_with(&(system + "\\")) {
            return false;
        }
        ![
            "valorant",
            "riot",
            "vanguard",
            "vgc.exe",
            "vgtray",
            "aclos",
            "wegame",
            "anti-cheat",
            "anticheat",
            "\\ace\\",
            "aceservice",
            "wonderful-ui.exe",
        ]
        .iter()
        .any(|part| lower.contains(part))
    }
    pub(super) fn inspect() -> Result<Vec<PortOwner>, String> {
        let owners = owners()?;
        let mut registry = tickets().lock().map_err(|_| "无法读取占用程序")?;
        registry.clear();
        let mut result = Vec::new();
        for pid in owners.into_iter().take(16) {
            let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
                .ok()
                .map(Owned);
            let identity = process.as_ref().and_then(|p| identity(p.0).ok());
            let name = identity
                .as_ref()
                .and_then(|id| Path::new(&id.path).file_name())
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| "受保护或无法识别的程序".into());
            let allowed = identity.as_ref().is_some_and(|id| id.allowed)
                && pid != std::process::id()
                && pid > 4;
            let ticket = if allowed {
                let id = identity.as_ref().unwrap();
                let token = uuid::Uuid::new_v4().to_string();
                registry.insert(
                    token.clone(),
                    Ticket {
                        pid,
                        path: id.path.clone(),
                        created: id.created,
                        issued: Instant::now(),
                    },
                );
                Some(token)
            } else {
                None
            };
            result.push(PortOwner {
                pid,
                name,
                ticket,
                reason: if allowed {
                    None
                } else {
                    Some("请在该程序中关闭相关功能，或自行退出程序后重试。".into())
                },
            });
        }
        Ok(result)
    }
    pub(super) fn release(token: &str) -> Result<(), String> {
        let ticket = tickets()
            .lock()
            .map_err(|_| "无法结束占用程序")?
            .remove(token)
            .ok_or("占用信息已变化，请重新检查")?;
        if ticket.issued.elapsed() > Duration::from_secs(120) || !owners()?.contains(&ticket.pid) {
            return Err("占用信息已变化，请重新检查".into());
        }
        let process = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE,
                false,
                ticket.pid,
            )
        }
        .map(Owned)
        .map_err(|_| "无法结束该程序，请自行退出后重试")?;
        let id = identity(process.0).map_err(|_| "无法核对该程序，请重新检查")?;
        if !id.allowed
            || id.created != ticket.created
            || id.path != ticket.path
            || ticket.pid == std::process::id()
            || !owners()?.contains(&ticket.pid)
        {
            return Err("占用信息已变化，请重新检查".into());
        }
        unsafe {
            TerminateProcess(process.0, 1).map_err(|_| "无法结束该程序，请自行退出后重试")?;
            let _ = WaitForSingleObject(process.0, 2000);
        }
        Ok(())
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn process_policy_excludes_system_game_and_own_app() {
            for path in [
                "C:\\Windows\\System32\\svchost.exe",
                "D:\\Games\\VALORANT.exe",
                "D:\\Riot Games\\client.exe",
                "D:\\ACLOS\\client.exe",
                "D:\\Apps\\wonderful-ui.exe",
            ] {
                assert!(!safe_path(path));
            }
            assert!(safe_path("D:\\Apps\\example.exe"));
        }
        #[test]
        fn arbitrary_process_actions_have_no_ticket() {
            assert!(release("unissued-fixture").is_err());
        }
        #[test]
        #[ignore = "child process used only by the port-owner regression"]
        fn port_owner_fixture() {
            let _listener =
                std::net::TcpListener::bind(("0.0.0.0", crate::share_policy::QUICK_SHARE_PORT))
                    .unwrap();
            loop {
                std::thread::sleep(Duration::from_secs(1));
            }
        }
        #[test]
        fn detects_and_releases_only_the_confirmed_port_owner() {
            use std::os::windows::process::CommandExt;
            let _lock = crate::share_server::tests::bind_test_lock().lock().unwrap();
            struct Child(std::process::Child);
            impl Drop for Child {
                fn drop(&mut self) {
                    let _ = self.0.kill();
                    let _ = self.0.wait();
                }
            }
            let mut child = Child(
                std::process::Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--ignored",
                        "--exact",
                        "share_port::native::tests::port_owner_fixture",
                    ])
                    .creation_flags(0x08000000)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .unwrap(),
            );
            let deadline = Instant::now() + Duration::from_secs(5);
            while !owners().unwrap().contains(&child.0.id()) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            let detected = inspect()
                .unwrap()
                .into_iter()
                .find(|owner| owner.pid == child.0.id())
                .expect("fixture listener detected");
            let ticket = detected
                .ticket
                .expect("own noncritical fixture can be ended");
            release(&ticket).unwrap();
            assert!(release(&ticket).is_err(), "tickets are single-use");
            child.0.wait().unwrap();
            let _listener =
                std::net::TcpListener::bind(("0.0.0.0", crate::share_policy::QUICK_SHARE_PORT))
                    .unwrap();
        }
    }
}
