//! Read-only historical import, with native-dialog capabilities and atomic storage.
use super::{db, now_ms, sha256_hex};
use crate::parser::{
    model::{Account, MatchRecord, VideoItem},
    parse_snapshot_db, parse_wonderful_db,
};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::Mutex,
};

const MAX_SOURCE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_FILES: usize = 100_000;
const TTL_MS: i64 = 30 * 60 * 1000;

#[derive(Default)]
pub struct ImportState(Mutex<Session>);
#[derive(Default)]
struct Session {
    selections: HashMap<String, (String, PathBuf, i64)>,
    drafts: HashMap<String, Draft>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    pub token: String,
    pub path: String,
}
#[derive(Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub token: String,
    pub device_name: String,
    pub accounts: usize,
    pub new_matches: usize,
    pub duplicate_matches: usize,
    pub conflicting_matches: usize,
    pub found_videos: usize,
    pub missing_videos: usize,
    pub ambiguous_videos: usize,
    pub failed_accounts: usize,
    pub warnings: Vec<String>,
}
struct Draft {
    device_id: String,
    name: String,
    accounts: Vec<Account>,
    matches: Vec<MatchRecord>,
    source_bytes: u64,
    preview: Preview,
    created_at: i64,
}

impl ImportState {
    pub fn select(&self, kind: String, path: PathBuf) -> Result<Selection, String> {
        validate_kind(&kind)?;
        let path = canonical_directory(&path)?;
        if !path.is_dir() {
            return Err("请选择文件夹".into());
        }
        let token = uuid::Uuid::new_v4().to_string();
        let mut s = self.0.lock().map_err(|_| "导入会话不可用")?;
        prune(&mut s);
        if s.selections.len() >= 16 {
            s.selections.clear();
        }
        s.selections
            .insert(token.clone(), (kind, path.clone(), now_ms()));
        Ok(Selection {
            token,
            path: path.to_string_lossy().into_owned(),
        })
    }
    pub fn preview(
        &self,
        database: &str,
        videos: &str,
        name: &str,
        conn: &Connection,
    ) -> Result<Preview, String> {
        let (database, videos) = {
            let mut s = self.0.lock().map_err(|_| "导入会话不可用")?;
            prune(&mut s);
            let resolve = |token: &str, kind: &str| {
                s.selections
                    .get(token)
                    .filter(|(k, _, _)| k == kind)
                    .map(|(_, p, _)| p.clone())
                    .ok_or_else(|| "目录选择已过期，请重新选择".to_string())
            };
            (resolve(database, "database")?, resolve(videos, "videos")?)
        };
        let mut draft = prepare(&database, &videos, name, conn)?;
        draft.preview.token = uuid::Uuid::new_v4().to_string();
        let result = draft.preview.clone();
        let mut s = self.0.lock().map_err(|_| "导入会话不可用")?;
        // Keep only the latest preview, so obsolete confirmations cannot commit.
        s.drafts.clear();
        s.drafts.insert(result.token.clone(), draft);
        Ok(result)
    }
    pub fn commit(&self, token: &str, conn: &Connection) -> Result<Preview, String> {
        let mut s = self.0.lock().map_err(|_| "导入会话不可用")?;
        prune(&mut s);
        let draft = s.drafts.get(token).ok_or("预览已过期，请重新预览")?;
        persist(conn, draft).map_err(|_| "导入失败，资料库未改变，请重试")?;
        let result = draft.preview.clone();
        s.drafts.remove(token);
        Ok(result)
    }
    pub fn cancel(&self) {
        if let Ok(mut s) = self.0.lock() {
            s.drafts.clear();
            s.selections.clear();
        }
    }
}
pub fn validate_kind(kind: &str) -> Result<(), String> {
    if matches!(kind, "database" | "videos") {
        Ok(())
    } else {
        Err("无效的目录类型".into())
    }
}
fn canonical_directory(path: &Path) -> Result<PathBuf, String> {
    let resolved = path.canonicalize().map_err(|_| "所选目录无法读取")?;
    #[cfg(windows)]
    {
        // Keep standard drive/UNC paths for WebView media, Explorer, and sharing.
        // std::fs::canonicalize otherwise exposes the Windows verbatim prefix.
        let value = resolved.to_string_lossy();
        if let Some(rest) = value.strip_prefix("\\\\?\\UNC\\") {
            return Ok(PathBuf::from(format!("\\\\{rest}")));
        }
        if let Some(rest) = value.strip_prefix("\\\\?\\") {
            return Ok(PathBuf::from(rest));
        }
    }
    Ok(resolved)
}
fn prune(s: &mut Session) {
    let earliest = now_ms() - TTL_MS;
    s.selections.retain(|_, (_, _, t)| *t >= earliest);
    s.drafts.retain(|_, d| d.created_at >= earliest);
}
fn ordinary(metadata: &std::fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    true
}
fn inventory(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    let mut pending = vec![(root.to_path_buf(), 0)];
    let mut seen = 0;
    while let Some((dir, depth)) = pending.pop() {
        if depth > 32 {
            return Err("视频目录层级过深，请选择更具体的目录".into());
        }
        for entry in std::fs::read_dir(dir).map_err(|_| "视频目录无法完整读取，请检查权限")?
        {
            let entry = entry.map_err(|_| "视频目录读取失败")?;
            seen += 1;
            if seen > MAX_FILES {
                return Err("视频目录内容过多，请选择更具体的目录".into());
            }
            let path = entry.path();
            let meta = std::fs::symlink_metadata(&path).map_err(|_| "视频文件无法读取")?;
            if !ordinary(&meta) {
                continue;
            }
            if meta.is_dir() {
                pending.push((path, depth + 1));
            } else if meta.is_file()
                && crate::path_has_extension(
                    &path,
                    &[
                        "mp4", "m4v", "mov", "webm", "mkv", "avi", "jpg", "jpeg", "png", "webp",
                    ],
                )
            {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}
fn parts(value: &str) -> Vec<String> {
    value
        .replace('\\', "/")
        .split('/')
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase)
        .collect()
}
// Require a unique name/size candidate; suffixes disambiguate preserved folders.
fn relocate(
    original: &str,
    size: i64,
    candidates: &HashMap<String, Vec<PathBuf>>,
) -> (String, bool) {
    let old = parts(original);
    let Some(name) = old.last() else {
        return (String::new(), false);
    };
    let Some(candidates) = candidates.get(name) else {
        return (String::new(), false);
    };
    let mut scored = Vec::new();
    for path in candidates {
        if size > 0
            && std::fs::metadata(path)
                .map(|m| m.len() != size as u64)
                .unwrap_or(true)
        {
            continue;
        }
        let new = parts(&path.to_string_lossy());
        let score = old
            .iter()
            .rev()
            .zip(new.iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        scored.push((score, path));
    }
    let Some(best) = scored.iter().map(|(s, _)| *s).max() else {
        return (String::new(), false);
    };
    let best: Vec<_> = scored.iter().filter(|(s, _)| *s == best).collect();
    if best.len() != 1 {
        return (String::new(), true);
    }
    (best[0].1.to_string_lossy().into_owned(), false)
}
fn logical_key(m: &MatchRecord) -> String {
    format!("{}\0{}", m.open_id, m.matches_id)
}
fn prepare(database: &Path, videos: &Path, name: &str, conn: &Connection) -> Result<Draft, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 40 {
        return Err("设备名称请填写 1～40 个字符".into());
    }
    let files = inventory(videos)?;
    let mut candidates: HashMap<String, Vec<PathBuf>> = HashMap::new();
    let mut posters: HashMap<String, Vec<PathBuf>> = HashMap::new();
    for path in files {
        let key = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        if crate::path_has_extension(&path, crate::SUPPORTED_VIDEO_EXTENSIONS) {
            candidates.entry(key).or_default().push(path);
        } else {
            posters.entry(key).or_default().push(path);
        }
    }
    let mut sources = Vec::new();
    for entry in std::fs::read_dir(database).map_err(|_| "WonderfulDb 目录无法读取")? {
        let entry = entry.map_err(|_| "WonderfulDb 目录读取失败")?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let meta = std::fs::symlink_metadata(entry.path()).map_err(|_| "数据文件无法读取")?;
        if ordinary(&meta)
            && meta.is_file()
            && !name.is_empty()
            && name.bytes().all(|c| c.is_ascii_digit())
        {
            if sources.len() >= 128 {
                return Err("账户文件过多，请检查所选目录".into());
            }
            sources.push((name, entry.path(), meta.len()));
        }
    }
    sources.sort_by(|a, b| a.0.cmp(&b.0));
    if sources.is_empty() {
        return Err(
            "此目录没有账户数据。请选择旧电脑 WonderfulDb 文件夹；仅有视频无法恢复对局信息。"
                .into(),
        );
    }
    let existing = db::load_library_view(conn, "").map_err(|_| "资料库无法读取")?;
    let existing_keys: HashMap<_, _> = existing
        .matches
        .iter()
        .map(|m| {
            (
                format!(
                    "{}\0{}",
                    m.open_id,
                    m.extras
                        .get("source_match_id")
                        .and_then(|s| s.as_str())
                        .unwrap_or(&m.matches_id)
                ),
                m,
            )
        })
        .collect();
    let mut preview = Preview {
        device_name: name.into(),
        ..Default::default()
    };
    let mut accounts = Vec::new();
    let mut matches = BTreeMap::new();
    let mut source_bytes = 0;
    // Fingerprint source contents rather than paths or labels. Different computers
    // with the same account remain distinguishable; a moved archive is idempotent.
    let mut fingerprint = String::new();
    for (openid, path, bytes) in sources {
        source_bytes += bytes;
        if source_bytes > MAX_SOURCE_BYTES {
            return Err("数据文件超过导入大小限制（128 MB）".into());
        }
        let source = std::fs::read(&path).map_err(|_| "数据文件无法读取")?;
        fingerprint.push_str(&format!(
            "{openid}:{};",
            sha256_hex(&String::from_utf8_lossy(&source))
        ));
        let parsed = match parse_wonderful_db(&path, &openid) {
            Ok(p) => p,
            Err(_) => {
                preview.failed_accounts += 1;
                continue;
            }
        };
        let snapshot_path = database.join(format!("snapshot{openid}"));
        let snapshot = match std::fs::symlink_metadata(&snapshot_path) {
            Ok(meta)
                if ordinary(&meta)
                    && meta.is_file()
                    && source_bytes + meta.len() <= MAX_SOURCE_BYTES =>
            {
                source_bytes += meta.len();
                parse_snapshot_db(&snapshot_path, &openid).unwrap_or_default()
            }
            _ => Default::default(),
        };
        accounts.push(Account {
            openid: openid.clone(),
            path: String::new(),
            match_count: parsed.matches.len(),
            nick: snapshot.nick,
            tag: snapshot.tag,
            custom_name: None,
            achievements: snapshot.achievements,
            match_details: snapshot.match_details,
            error: None,
            error_code: None,
        });
        for mut m in parsed.matches {
            if m.matches_id.trim().is_empty() {
                preview.warnings.push("已跳过缺少对局编号的记录".into());
                continue;
            }
            m.open_id = openid.clone();
            for v in &mut m.videos {
                let (path, ambiguous) = relocate(&v.video_src, v.video_size, &candidates);
                if ambiguous {
                    preview.ambiguous_videos += 1;
                } else if path.is_empty() {
                    preview.missing_videos += 1;
                } else {
                    preview.found_videos += 1;
                }
                v.video_src = path;
                v.video_poster = relocate(&v.video_poster, 0, &posters).0;
            }
            let key = logical_key(&m);
            if let Some(previous) = matches.get_mut(&key) {
                merge_videos(previous, &m);
            } else {
                matches.insert(key, m);
            }
        }
    }
    if accounts.is_empty() {
        return Err("所有账户均解析失败，请确认导入的是完整的旧 WonderfulDb 文件夹".into());
    }
    preview.accounts = accounts.len();
    for (key, m) in &matches {
        if let Some(old) = existing_keys.get(key) {
            preview.duplicate_matches += 1;
            if serde_json::to_value((&old.stats, &old.map, &old.agent)).ok()
                != serde_json::to_value((&m.stats, &m.map, &m.agent)).ok()
            {
                preview.conflicting_matches += 1;
            }
        } else {
            preview.new_matches += 1;
        }
    }
    preview.warnings.sort();
    preview.warnings.dedup();
    let device_id = format!("import:{}", sha256_hex(&fingerprint));
    Ok(Draft {
        device_id,
        name: name.into(),
        accounts,
        matches: matches.into_values().collect(),
        source_bytes,
        preview,
        created_at: now_ms(),
    })
}
fn persist(conn: &Connection, draft: &Draft) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    let now = now_ms();
    tx.execute("INSERT INTO history_devices(id,label,source_bytes,created_at,updated_at) VALUES(?1,?2,?3,?4,?4)
        ON CONFLICT(id) DO UPDATE SET label=excluded.label,source_bytes=excluded.source_bytes,updated_at=excluded.updated_at",
        params![draft.device_id, draft.name, draft.source_bytes, now])?;
    for a in &draft.accounts {
        tx.execute(
            "INSERT INTO history_accounts(device_id,openid,raw_json) VALUES(?1,?2,?3)
            ON CONFLICT(device_id,openid) DO UPDATE SET raw_json=excluded.raw_json",
            params![draft.device_id, a.openid, serde_json::to_string(a).unwrap()],
        )?;
    }
    for m in &draft.matches {
        tx.execute(
            "INSERT INTO history_matches(device_id,openid,match_id,raw_json) VALUES(?1,?2,?3,?4)
            ON CONFLICT(device_id,openid,match_id) DO UPDATE SET raw_json=excluded.raw_json",
            params![
                draft.device_id,
                m.open_id,
                m.matches_id,
                serde_json::to_string(m).unwrap()
            ],
        )?;
        tx.execute(
            "DELETE FROM history_videos WHERE device_id=?1 AND openid=?2 AND match_id=?3",
            params![draft.device_id, m.open_id, m.matches_id],
        )?;
        for v in &m.videos {
            if !v.video_src.is_empty() {
                tx.execute("INSERT OR IGNORE INTO history_videos(device_id,openid,match_id,path) VALUES(?1,?2,?3,?4)", params![draft.device_id,m.open_id,m.matches_id,v.video_src])?;
            }
        }
    }
    tx.execute("INSERT INTO scrape_jobs(id,source_id,trigger,started_at,finished_at,status,matches_seen,videos_seen,errors_seen,message)
        VALUES(?1,?2,'import',?3,?3,'success',?4,?5,?6,?7)",
        params![uuid::Uuid::new_v4().to_string(),draft.device_id,now,draft.matches.len(),draft.preview.found_videos + draft.preview.missing_videos + draft.preview.ambiguous_videos,draft.preview.failed_accounts,"历史资料导入完成"])?;
    tx.commit()
}
fn video_key(v: &VideoItem) -> String {
    if !v.video_id.is_empty() {
        v.video_id.clone()
    } else {
        format!("{}:{}:{}", v.video_name, v.video_time, v.video_size)
    }
}
fn merge_videos(primary: &mut MatchRecord, other: &MatchRecord) {
    for v in &other.videos {
        if let Some(current) = primary
            .videos
            .iter_mut()
            .find(|c| video_key(c) == video_key(v))
        {
            if !Path::new(&current.video_src).is_file() && Path::new(&v.video_src).is_file() {
                current.video_src = v.video_src.clone();
                current.video_poster = v.video_poster.clone();
            }
            if current.rounds.is_empty() {
                current.rounds = v.rounds.clone();
            }
        } else {
            primary.videos.push(v.clone());
        }
    }
}
fn add_source(m: &mut MatchRecord, id: &str, label: &str) {
    let list = m
        .extras
        .entry("library_sources".into())
        .or_insert_with(|| serde_json::json!([]));
    if let Some(a) = list.as_array_mut() {
        if !a.iter().any(|s| s["id"] == id) {
            a.push(serde_json::json!({"id":id,"label":label}));
        }
    }
}
pub fn load_match(
    conn: &Connection,
    openid: &str,
    displayed_id: &str,
) -> Result<MatchRecord, String> {
    use rusqlite::OptionalExtension;
    let mut original = displayed_id.to_string();
    if displayed_id.starts_with("history:") {
        let mut stmt = conn
            .prepare("SELECT DISTINCT match_id FROM history_matches WHERE openid=?1")
            .map_err(|e| e.to_string())?;
        for id in stmt
            .query_map([openid], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
        {
            let id = id.map_err(|e| e.to_string())?;
            if format!("history:{}", sha256_hex(&format!("{openid}\0{id}"))) == displayed_id {
                original = id;
                break;
            }
        }
    }
    let raw: Option<String> = conn
        .query_row(
            "SELECT raw_json FROM matches WHERE openid=?1 AND id=?2",
            [openid, original.as_str()],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let mut result = raw.and_then(|r| serde_json::from_str::<MatchRecord>(&r).ok());
    if let Some(m) = result.as_mut() {
        m.extras.remove("library_sources");
        add_source(m, "local", "本机");
    }
    let mut stmt = conn.prepare("SELECT m.raw_json,d.id,d.label FROM history_matches m JOIN history_devices d ON d.id=m.device_id WHERE m.openid=?1 AND m.match_id=?2 ORDER BY d.created_at,d.id").map_err(|e| e.to_string())?;
    for row in stmt
        .query_map([openid, original.as_str()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?
    {
        let (raw, id, label) = row.map_err(|e| e.to_string())?;
        if let Ok(mut m) = serde_json::from_str::<MatchRecord>(&raw) {
            if let Some(current) = result.as_mut() {
                merge_videos(current, &m);
                add_source(current, &id, &label);
            } else {
                m.extras
                    .insert("source_match_id".into(), serde_json::json!(m.matches_id));
                m.matches_id = format!("history:{}", sha256_hex(&logical_key(&m)));
                add_source(&mut m, &id, &label);
                result = Some(m);
            }
        }
    }
    result
        .filter(|m| m.matches_id == displayed_id)
        .ok_or_else(|| "对局不在资料库中，请刷新列表".into())
}
pub fn merge_view(
    conn: &Connection,
    accounts: &mut Vec<Account>,
    matches: &mut Vec<MatchRecord>,
) -> rusqlite::Result<usize> {
    let mut corrupt = 0;
    let mut keys = HashMap::new();
    for (i, m) in matches.iter_mut().enumerate() {
        m.extras.remove("library_sources");
        add_source(m, "local", "本机");
        keys.insert(logical_key(m), i);
    }
    let mut stmt = conn.prepare("SELECT a.raw_json FROM history_accounts a JOIN history_devices d ON d.id=a.device_id ORDER BY d.created_at,a.openid")?;
    for raw in stmt.query_map([], |r| r.get::<_, String>(0))? {
        if let Ok(a) = serde_json::from_str::<Account>(&raw?) {
            if let Some(current) = accounts.iter_mut().find(|c| c.openid == a.openid) {
                if current.nick.is_none() {
                    current.nick = a.nick;
                }
                if current.tag.is_none() {
                    current.tag = a.tag;
                }
                for ach in a.achievements {
                    if !current
                        .achievements
                        .iter()
                        .any(|c| c.matches_id == ach.matches_id)
                    {
                        current.achievements.push(ach);
                    }
                }
                for detail in a.match_details {
                    if let Some(existing) = current
                        .match_details
                        .iter_mut()
                        .find(|d| d.matches_id == detail.matches_id)
                    {
                        if existing.achievement.is_none() {
                            existing.achievement = detail.achievement;
                        }
                        if existing.is_draw.is_none() {
                            existing.is_draw = detail.is_draw;
                        }
                        if existing.has_won.is_none() {
                            existing.has_won = detail.has_won;
                        }
                    } else {
                        current.match_details.push(detail);
                    }
                }
            } else {
                accounts.push(a);
            }
        } else {
            corrupt += 1;
        }
    }
    let mut stmt = conn.prepare("SELECT m.raw_json,d.id,d.label FROM history_matches m JOIN history_devices d ON d.id=m.device_id ORDER BY d.created_at,d.id,m.openid,m.match_id")?;
    for row in stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })? {
        let (raw, id, label) = row?;
        if let Ok(mut m) = serde_json::from_str::<MatchRecord>(&raw) {
            let key = logical_key(&m);
            if let Some(i) = keys.get(&key) {
                merge_videos(&mut matches[*i], &m);
                add_source(&mut matches[*i], &id, &label);
            } else {
                m.extras
                    .insert("source_match_id".into(), serde_json::json!(m.matches_id));
                m.matches_id = format!("history:{}", sha256_hex(&key));
                add_source(&mut m, &id, &label);
                keys.insert(key, matches.len());
                matches.push(m);
            }
        } else {
            corrupt += 1;
        }
    }
    for a in accounts.iter_mut() {
        a.match_count = matches.iter().filter(|m| m.open_id == a.openid).count();
        for ach in &mut a.achievements {
            if let Some(m) = matches.iter().find(|m| {
                m.open_id == a.openid
                    && m.extras.get("source_match_id").and_then(|v| v.as_str())
                        == Some(&ach.matches_id)
            }) {
                ach.matches_id = m.matches_id.clone();
            }
        }
        for detail in &mut a.match_details {
            if let Some(m) = matches.iter().find(|m| {
                m.open_id == a.openid
                    && m.extras.get("source_match_id").and_then(|v| v.as_str())
                        == Some(&detail.matches_id)
            }) {
                detail.matches_id = m.matches_id.clone();
            }
        }
        if let Ok((custom, _order)) = conn.query_row(
            "SELECT custom_name,sort_order FROM account_preferences WHERE openid=?1",
            [&a.openid],
            |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<i64>>(1)?)),
        ) {
            a.custom_name = custom;
        }
    }
    accounts.retain(|a| {
        a.match_count > 0
            || a.error.is_some()
            || a.nick.as_deref().is_some_and(|n| !n.trim().is_empty())
            || a.custom_name
                .as_deref()
                .is_some_and(|n| !n.trim().is_empty())
    });
    let mut order = HashMap::new();
    let mut stmt = conn.prepare(
        "SELECT openid,sort_order FROM account_preferences WHERE sort_order IS NOT NULL",
    )?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
        let (id, i) = row?;
        order.insert(id, i);
    }
    accounts.sort_by_key(|a| {
        (
            order.get(&a.openid).copied().unwrap_or(i64::MAX),
            a.openid.clone(),
        )
    });
    matches.sort_by(|a, b| {
        b.matches_time
            .cmp(&a.matches_time)
            .then(a.matches_id.cmp(&b.matches_id))
    });
    Ok(corrupt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::model::{RoundItem, SnapshotAchievement};
    use aes::Aes256;
    use cbc::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("wui-history-test-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(root.join("WonderfulDb")).unwrap();
            std::fs::create_dir_all(root.join("media")).unwrap();
            Self(root)
        }
        fn db(&self) -> PathBuf {
            self.0.join("WonderfulDb")
        }
        fn media(&self) -> PathBuf {
            self.0.join("media")
        }
        fn write(&self, openid: &str, value: serde_json::Value, snapshot: bool) -> Vec<u8> {
            let plain = serde_json::to_vec(&value).unwrap();
            let (key, iv) = crate::parser::crypto::derive_key_iv(openid);
            let length = plain.len();
            let mut padded = vec![0; length + 16];
            padded[..length].copy_from_slice(&plain);
            let encrypted = cbc::Encryptor::<Aes256>::new_from_slices(&key, &iv)
                .unwrap()
                .encrypt_padded_mut::<Pkcs7>(&mut padded, length)
                .unwrap();
            let bytes = hex::encode(encrypted).into_bytes();
            std::fs::write(
                self.db().join(if snapshot {
                    format!("snapshot{openid}")
                } else {
                    openid.into()
                }),
                &bytes,
            )
            .unwrap();
            bytes
        }
        fn match_record(&self, id: &str) -> MatchRecord {
            MatchRecord {
                matches_id: id.into(),
                open_id: "123".into(),
                matches_time: 42,
                videos: vec![VideoItem {
                    video_id: "video-a".into(),
                    video_name: "clip".into(),
                    video_src: "X:\\old\\media\\clip.mp4".into(),
                    video_poster: "X:\\old\\media\\clip.jpg".into(),
                    video_size: 3,
                    rounds: vec![RoundItem {
                        round_id: "round-a".into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }
        }
        fn seed(&self, m: &MatchRecord) -> Vec<u8> {
            std::fs::write(self.media().join("clip.mp4"), b"mp4").unwrap();
            std::fs::write(self.media().join("clip.jpg"), b"jpg").unwrap();
            self.write(
                "123",
                serde_json::json!({"key_wonderful_list_123":[m]}),
                false,
            )
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn connection() -> Connection {
        let conn = db::open_memory_for_test().unwrap();
        db::migrate(&conn).unwrap();
        conn
    }
    fn count(conn: &Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn encrypted_import_is_readonly_idempotent_and_survives_local_full_scan() {
        let fixture = Fixture::new();
        let conn = connection();
        let bytes = fixture.seed(&fixture.match_record("match-a"));
        let snapshot = fixture.write("123",serde_json::json!({"key_snapshot_list123":[{"matches_id":"match-a","snapshot":{"ss_nick":"Fixture","ss_nick_id":"001","ss_achieve_type":"mvp"}}]}),true);
        let state = ImportState::default();
        let database = state.select("database".into(), fixture.db()).unwrap();
        let videos = state.select("videos".into(), fixture.media()).unwrap();
        let p = state
            .preview(&database.token, &videos.token, "旧设备", &conn)
            .unwrap();
        assert_eq!((p.new_matches, p.found_videos, p.missing_videos), (1, 1, 0));
        assert_eq!(
            count(&conn, "history_matches"),
            0,
            "preview never writes data"
        );
        state.commit(&p.token, &conn).unwrap();
        assert!(
            state.commit(&p.token, &conn).is_err(),
            "token can only commit once"
        );
        let p = state
            .preview(&database.token, &videos.token, "旧设备", &conn)
            .unwrap();
        assert_eq!((p.new_matches, p.duplicate_matches), (0, 1));
        state.commit(&p.token, &conn).unwrap();
        assert_eq!(count(&conn, "history_matches"), 1);
        assert_eq!(count(&conn, "history_devices"), 1);
        let before = db::load_library_view(&conn, "").unwrap();
        assert!(before.matches[0].videos[0].rounds.is_empty());
        assert_eq!(before.accounts[0].nick.as_deref(), Some("Fixture"));
        let full = db::load_match_rounds(&conn, "123", &before.matches[0].matches_id).unwrap();
        assert_eq!(full.videos[0].rounds[0].round_id, "round-a");
        assert!(db::is_library_video_path(&conn, &full.videos[0].video_src).unwrap());
        assert!(!db::is_library_video_path(&conn, "X:\\old\\media\\clip.mp4").unwrap());
        assert!(db::is_known_account(&conn, "123").unwrap());
        let stats = super::super::stats::compute(&conn).unwrap();
        assert_eq!(
            (
                stats.total_videos,
                stats.total_accounts,
                stats.missing_videos
            ),
            (1, 1, 0)
        );
        let local = fixture.0.join("local");
        std::fs::create_dir_all(&local).unwrap();
        let source = Fixture::new();
        source.write("123", serde_json::json!({}), false);
        std::fs::copy(source.db().join("123"), local.join("123")).unwrap();
        super::super::scraper::scrape_wonderful_dir_with_mode(
            &conn,
            &local,
            "manual",
            super::super::scraper::ScrapeMode::Full,
            None,
        )
        .unwrap();
        let after = db::load_library_view(&conn, "").unwrap();
        assert_eq!(after.matches[0].matches_id, before.matches[0].matches_id);
        assert_eq!(std::fs::read(fixture.db().join("123")).unwrap(), bytes);
        assert_eq!(
            std::fs::read(fixture.db().join("snapshot123")).unwrap(),
            snapshot
        );
        assert_eq!(
            std::fs::read(fixture.media().join("clip.mp4")).unwrap(),
            b"mp4"
        );
        // A new connection reproduces the committed state without source files.
        let persisted = fixture.0.join("library.db");
        conn.execute("VACUUM INTO ?1", [persisted.to_string_lossy().as_ref()])
            .unwrap();
        let reopened = Connection::open(persisted).unwrap();
        db::migrate(&reopened).unwrap();
        assert_eq!(
            db::load_library_view(&reopened, "").unwrap().matches.len(),
            1
        );
    }
    #[test]
    fn duplicate_matches_merge_videos_and_source_labels_with_local_stats_priority() {
        let fixture = Fixture::new();
        let conn = connection();
        let mut local = fixture.match_record("match-a");
        local.stats.kills = 99;
        local.videos[0].video_src = String::new();
        conn.execute(
            "INSERT INTO accounts(openid,source_id,last_seen_at) VALUES('123','aclos',0)",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO matches(id,source_id,source_match_id,openid,matches_time,stats_json,raw_json,last_seen_at) VALUES('match-a','aclos','match-a','123',42,'{}',?1,0)",[serde_json::to_string(&local).unwrap()]).unwrap();
        let mut imported = fixture.match_record("match-a");
        imported.stats.kills = 10;
        imported.videos.push(VideoItem {
            video_id: "extra".into(),
            video_src: "X:\\absent.mp4".into(),
            ..Default::default()
        });
        fixture.seed(&imported);
        let draft = prepare(&fixture.db(), &fixture.media(), "旧设备", &conn).unwrap();
        assert_eq!(
            (draft.preview.new_matches, draft.preview.duplicate_matches),
            (0, 1)
        );
        persist(&conn, &draft).unwrap();
        let view = db::load_library_view(&conn, "").unwrap();
        assert_eq!(view.matches.len(), 1);
        assert_eq!(view.matches[0].stats.kills, 99);
        assert_eq!(view.matches[0].videos.len(), 2);
        assert_eq!(
            view.matches[0].extras["library_sources"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert!(!view.matches[0].videos[0].video_src.is_empty());
        let full = db::load_match_rounds(&conn, "123", "match-a").unwrap();
        assert_eq!(full.videos.len(), 2);
        assert!(!full.videos[0].rounds.is_empty());
        assert_eq!(super::super::stats::compute(&conn).unwrap().total_videos, 2);
        // Another archive containing the same account remains another device.
        imported.matches_id = "match-b".into();
        fixture.seed(&imported);
        persist(
            &conn,
            &prepare(&fixture.db(), &fixture.media(), "另一台设备", &conn).unwrap(),
        )
        .unwrap();
        assert_eq!(count(&conn, "history_devices"), 2);
    }
    #[test]
    fn cancellation_expiry_and_transaction_failure_leave_library_unchanged() {
        let fixture = Fixture::new();
        fixture.seed(&fixture.match_record("m"));
        let conn = connection();
        let state = ImportState::default();
        let d = state.select("database".into(), fixture.db()).unwrap();
        let v = state.select("videos".into(), fixture.media()).unwrap();
        assert!(state.preview(&v.token, &d.token, "旧电脑", &conn).is_err());
        let p = state.preview(&d.token, &v.token, "旧电脑", &conn).unwrap();
        state.cancel();
        assert!(state.commit(&p.token, &conn).is_err());
        assert_eq!(count(&conn, "history_devices"), 0);
        let draft = prepare(&fixture.db(), &fixture.media(), "旧电脑", &conn).unwrap();
        conn.execute_batch("CREATE TRIGGER reject_history BEFORE INSERT ON history_matches BEGIN SELECT RAISE(ABORT,'test failure'); END;").unwrap();
        assert!(persist(&conn, &draft).is_err());
        assert_eq!(count(&conn, "history_devices"), 0);
        assert_eq!(count(&conn, "history_accounts"), 0);
        assert_eq!(count(&conn, "scrape_jobs"), 0);
        let d = state.select("database".into(), fixture.db()).unwrap();
        state
            .0
            .lock()
            .unwrap()
            .selections
            .get_mut(&d.token)
            .unwrap()
            .2 = now_ms() - TTL_MS - 1;
        assert!(state.preview(&d.token, &v.token, "旧电脑", &conn).is_err());
    }
    #[test]
    fn matching_uses_size_and_suffix_but_never_chooses_an_ambiguous_file() {
        let fixture = Fixture::new();
        let a = fixture.media().join("a");
        let b = fixture.media().join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(a.join("clip.mp4"), b"one").unwrap();
        std::fs::write(b.join("clip.mp4"), b"two").unwrap();
        let map = HashMap::from([(
            "clip.mp4".into(),
            vec![a.join("clip.mp4"), b.join("clip.mp4")],
        )]);
        assert_eq!(
            relocate("X:\\unknown\\clip.mp4", 3, &map),
            (String::new(), true)
        );
        assert_eq!(
            relocate("X:\\a\\clip.mp4", 3, &map).0,
            a.join("clip.mp4").to_string_lossy()
        );
        std::fs::write(b.join("clip.mp4"), b"longer").unwrap();
        assert_eq!(
            relocate("X:\\clip.mp4", 3, &map).0,
            a.join("clip.mp4").to_string_lossy()
        );
        assert_eq!(relocate("X:\\clip.mp4", 55, &map), (String::new(), false));
    }
    #[test]
    fn corrupt_account_is_reported_and_preferences_and_achievements_survive() {
        let fixture = Fixture::new();
        fixture.seed(&fixture.match_record("m"));
        let conn = connection();
        std::fs::write(fixture.db().join("456"), b"broken").unwrap();
        let mut draft = prepare(&fixture.db(), &fixture.media(), "旧电脑", &conn).unwrap();
        assert_eq!(draft.preview.failed_accounts, 1);
        draft.accounts[0].achievements.push(SnapshotAchievement {
            matches_id: "m".into(),
            achv_type: "mvp".into(),
            type_str: "MVP".into(),
        });
        draft.accounts[0]
            .match_details
            .push(crate::parser::model::SnapshotMatchDetail {
                matches_id: "m".into(),
                achievement: Some("mvp".into()),
                is_draw: Some(false),
                has_won: Some(true),
            });
        persist(&conn, &draft).unwrap();
        db::set_account_custom_name(&conn, "123", Some("自定义名称")).unwrap();
        db::save_account_order(&conn, &["123".into()]).unwrap();
        let view = db::load_library_view(&conn, "").unwrap();
        assert_eq!(view.accounts[0].custom_name.as_deref(), Some("自定义名称"));
        assert_eq!(
            view.accounts[0].match_details[0].matches_id,
            view.matches[0].matches_id
        );
        assert_eq!(view.accounts[0].match_details[0].has_won, Some(true));
        assert_eq!(
            view.accounts[0].achievements[0].matches_id,
            view.matches[0].matches_id
        );
        let empty = Fixture::new();
        assert!(prepare(&empty.db(), &empty.media(), "旧电脑", &conn)
            .err()
            .unwrap()
            .contains("仅有视频"));
    }
}
