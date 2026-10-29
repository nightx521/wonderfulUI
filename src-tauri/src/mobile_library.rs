//! A read-only, explicitly projected mobile view. Never expose source paths,
//! raw event trees, diagnostics, database identifiers, or arbitrary JSON extras.
use crate::library::{db, sha256_hex};
use crate::parser::model::LoadResult;
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Copy)]
pub enum MediaKind {
    Video,
    Poster,
}
#[derive(Clone)]
pub struct MediaFile {
    pub path: PathBuf,
    pub kind: MediaKind,
}
pub struct Snapshot {
    pub json: Vec<u8>,
    pub media: HashMap<String, MediaFile>,
}

fn key(salt: &str, kind: &str, id: &str) -> String {
    sha256_hex(&format!("{salt}|{kind}|{id}"))
}
fn selected(value: Option<&Value>, keys: &[&str]) -> Value {
    let mut out = serde_json::Map::new();
    if let Some(object) = value.and_then(Value::as_object) {
        for k in keys {
            if let Some(v) = object.get(*k) {
                if v.is_string() || v.is_number() || v.is_boolean() || v.is_null() {
                    out.insert((*k).into(), v.clone());
                }
            }
        }
    }
    Value::Object(out)
}
fn register(
    media: &mut HashMap<String, MediaFile>,
    salt: &str,
    id: &str,
    path: &str,
    kind: MediaKind,
) -> String {
    if path.is_empty() {
        return String::new();
    }
    let category = match kind {
        MediaKind::Video => "video",
        MediaKind::Poster => "poster",
    };
    let opaque = key(salt, category, &format!("{id}|{path}"));
    media.insert(
        opaque.clone(),
        MediaFile {
            path: PathBuf::from(path),
            kind,
        },
    );
    format!("/media/{opaque}")
}
pub fn project(view: LoadResult, salt: &str) -> Result<Snapshot, String> {
    let mut media = HashMap::new();
    let accounts:Vec<_>=view.accounts.into_iter().map(|a|json!({
        "openid":key(salt,"account",&a.openid),"nick":a.nick,"tag":a.tag,"customName":a.custom_name,"matchCount":a.match_count,
        "achievements":a.achievements.into_iter().map(|item|json!({"matchesId":key(salt,"match",&item.matches_id),"achvType":item.achv_type})).collect::<Vec<_>>(),
        "matchDetails":a.match_details.into_iter().map(|mut item|{item.matches_id=key(salt,"match",&item.matches_id);json!(item)}).collect::<Vec<_>>()
    })).collect();
    let matches:Vec<_>=view.matches.into_iter().filter(|m|!m.matches_id.trim().is_empty()&&!m.open_id.trim().is_empty()).map(|m|{
        let videos:Vec<_>=m.videos.into_iter().map(|v|json!({
            "video_id":key(salt,"video",&v.video_id),"video_src":register(&mut media,salt,&v.video_id,&v.video_src,MediaKind::Video),
            "video_poster":register(&mut media,salt,&v.video_id,&v.video_poster,MediaKind::Poster),"video_type":v.video_type,
            "video_name":v.video_type,"video_duration":v.video_duration,"video_size":v.video_size,"video_fps":v.video_fps,
            "video_resolution":v.video_resolution,"video_level":v.video_level,"video_isProcessing":v.video_is_processing.unwrap_or(false),"rounds":[]
        })).collect();
        let sources:Vec<_>=m.extras.get("library_sources").and_then(Value::as_array).map(|a|a.iter().filter_map(|s|{
            let id=s.get("id")?.as_str()?;let label=s.get("label")?.as_str()?;
            Some(json!({"id":if id=="local"{"local".into()}else{key(salt,"source",id)},"label":label}))
        }).collect()).unwrap_or_else(||vec![json!({"id":"local","label":"本机"})]);
        json!({"matches_id":key(salt,"match",&m.matches_id),"openID":key(salt,"account",&m.open_id),"matches_time":m.matches_time,
            "gameStartTime":m.game_start_time,"gameEndTime":m.game_end_time,"minRoundId":m.min_round_id,"mode":m.mode,
            "map":{"map_id":m.map.map_id,"map_name":m.map.map_name},"agent":m.agent,"stats":m.stats,
            "career":selected(m.extras.get("career"),&["hero_name","map_name","game_mode"]),
            "tier":selected(m.extras.get("tier"),&["tier_before","tier_after"]),"library_sources":sources,"videos":videos})
    }).collect();
    let json = serde_json::to_vec(
        &json!({"accounts":accounts,"matches":matches,"totalErrors":view.total_errors}),
    )
    .map_err(|_| "手机资料准备失败")?;
    if json.len() > 32 * 1024 * 1024 {
        return Err("资料库较大，暂时无法在手机中加载".into());
    }
    Ok(Snapshot { json, media })
}
pub fn load(salt: &str) -> Result<Snapshot, String> {
    let path = db::library_dir()?.join("library.db");
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| "资料库暂时无法读取，请稍后重试")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| "资料库暂时无法读取")?;
    let view = db::load_library_view(&conn, String::new())
        .map_err(|_| "资料库暂时无法读取，请稍后重试")?;
    project(view, salt)
}
pub fn allowed_media(file: &MediaFile) -> bool {
    let extension = file
        .path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let allowed = match file.kind {
        MediaKind::Video => matches!(extension.as_str(), "mp4" | "m4v" | "mov" | "webm" | "mkv"),
        MediaKind::Poster => matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "webp"),
    };
    allowed
        && file.path.is_absolute()
        && std::fs::symlink_metadata(&file.path).is_ok_and(|m| m.file_type().is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn projection_keeps_display_facts_and_omits_private_paths_and_raw_fields() {
        let view:LoadResult=serde_json::from_value(json!({"dir":"C:\\private","accounts":[{"openid":"123","path":"C:\\private\\123","nick":"Fixture","tag":"001","matchCount":1,"error":"private diagnostic","achievements":[{"matchesId":"match","achvType":"mvp","typeStr":"MVP"}]}],"matches":[{"matches_id":"match","openID":"123","career":{"hero_name":"捷风","private":"secret"},"secret":"private","videos":[{"video_id":"video","video_src":"C:\\private\\clip.mp4","video_poster":"C:\\private\\cover.png","video_name":"private clip","video_type":"击杀集锦","rounds":[{}]}]}],"totalErrors":1})).unwrap();
        let snap = project(view, "salt").unwrap();
        let text = String::from_utf8(snap.json).unwrap();
        assert!(!text.contains("private"));
        assert!(!text.contains("secret"));
        assert!(!text.contains("C:"));
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["accounts"][0]["nick"], "Fixture");
        assert_eq!(
            v["accounts"][0]["achievements"][0]["matchesId"],
            v["matches"][0]["matches_id"]
        );
        assert_eq!(snap.media.len(), 2);
        assert!(v["matches"][0]["videos"][0]["video_src"]
            .as_str()
            .unwrap()
            .starts_with("/media/"));
    }
}
