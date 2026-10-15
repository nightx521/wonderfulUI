//! Parse pipeline: read a WonderfulDb file, decrypt it, build the model.
//!
//! Matches the TS parser 1:1 in behavior:
//!   1. Read file as bytes (the file is ASCII hex)
//!   2. UTF-8 decode the bytes -> hex string
//!   3. Hex-decode the string -> ciphertext bytes
//!   4. AES-256-CBC decrypt (PKCS7) -> plaintext bytes
//!   5. UTF-8 decode plaintext -> JSON string
//!   6. Parse JSON, look up `key_wonderful_list_<openid>` (or first array)
//!   7. Deserialize to Vec<MatchRecord>
//!
//! Same scheme decrypts the per-account `snapshot<openid>` file, whose top
//! level key is `key_snapshot_list<openid>` (note: no separator) and whose
//! records carry the player's display name under `snapshot.ss_nick` /
//! `snapshot.ss_nick_id`. See `parse_snapshot_db`.

use crate::parser::crypto::{aes_decrypt, derive_key_iv};
use crate::parser::error::ParseError;
use crate::parser::hex::decode_hex;
use crate::parser::model::{
    MatchRecord, SnapshotAchievement, SnapshotMatchDetail, WonderfulDbFile,
};
use std::path::Path;

/// Parse a WonderfulDb file. `openid` is the file name without path
/// (ACLOS uses the openid as the file name with no extension).
pub fn parse_wonderful_db(path: &Path, openid: &str) -> Result<WonderfulDbFile, ParseError> {
    let bytes = std::fs::read(path)?;
    let hex_text = std::str::from_utf8(&bytes)?;
    let cipher = decode_hex(hex_text)?;

    let (key, iv) = derive_key_iv(openid);
    let plain = aes_decrypt(&cipher, &key, &iv)?;
    let json_text = String::from_utf8(plain)?;
    parse_json(&json_text, openid)
}

fn parse_json(json_text: &str, openid: &str) -> Result<WonderfulDbFile, ParseError> {
    let raw: serde_json::Value = serde_json::from_str(json_text)?;
    let obj = raw.as_object().ok_or(ParseError::NotAnObject)?;

    let expected_key = format!("key_wonderful_list_{}", openid);
    // ACLOS also writes a valid encrypted empty object before highlights exist.
    // Other missing/invalid structures remain errors rather than becoming empty libraries.
    if obj.is_empty() {
        return Ok(WonderfulDbFile {
            key: expected_key,
            matches: Vec::new(),
            raw: Some(raw),
        });
    }
    let list_value = obj
        .get(&expected_key)
        .or_else(|| obj.values().find(|v| v.is_array()))
        .ok_or_else(|| ParseError::NoMatchList(expected_key.clone()))?;

    let mut matches: Vec<MatchRecord> = serde_json::from_value(list_value.clone())?;
    if let Some(records) = list_value.as_array() {
        for (m, record) in matches.iter_mut().zip(records) {
            m.stats.missing_stats = Some(
                ["kills", "deaths", "assists", "score"]
                    .iter()
                    .filter(|key| {
                        record
                            .get("stats")
                            .and_then(|s| s.get(**key))
                            .is_none_or(|v| v.is_null())
                    })
                    .map(|key| (*key).to_string())
                    .collect(),
            );
            m.stats.result_known = Some(
                record
                    .get("stats")
                    .and_then(|s| s.get("has_won"))
                    .is_some_and(|v| !v.is_null()),
            );
            m.stats.score_known = Some(["rounds_won", "rounds_lost"].iter().all(|key| {
                record
                    .get("stats")
                    .and_then(|s| s.get(key))
                    .is_some_and(|v| !v.is_null())
            }));
        }
    }
    Ok(WonderfulDbFile {
        key: expected_key,
        matches,
        raw: Some(raw),
    })
}

/// In-game display name + #tag + per-match achievements extracted from a
/// `snapshot<openid>` file. Any field can be missing when the snapshot file
/// is empty or has no matching records.
#[derive(Debug, Clone, Default)]
pub struct SnapshotData {
    pub nick: Option<String>,
    pub tag: Option<String>,
    pub achievements: Vec<SnapshotAchievement>,
    pub match_details: Vec<SnapshotMatchDetail>,
}

/// Parse the per-account `snapshot<openid>` file. Same AES scheme as
/// `parse_wonderful_db`. The top-level key is `key_snapshot_list<openid>`
/// (no separator — unlike the wonderful-list key). Each record has a
/// `snapshot` sub-object whose `ss_nick` / `ss_nick_id` give the player's
/// own display name and #tag; records with `ss_achieve_type === "mvp"` or
/// `"svp"` are collected as per-match achievements.
///
/// Returns `Ok(SnapshotData::default())` when the file is missing, empty,
/// or has no `ss_nick` / `ss_nick_id` — never an error.
pub fn parse_snapshot_db(path: &Path, openid: &str) -> Result<SnapshotData, ParseError> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(SnapshotData::default()),
        Err(e) => return Err(e.into()),
    };
    let hex_text = std::str::from_utf8(&bytes)
        .map_err(|e| ParseError::Crypto(format!("file is not valid UTF-8: {}", e)))?;
    let cipher = match decode_hex(hex_text) {
        Ok(c) => c,
        // Snapshot files written when ACLOS has nothing to save are exactly
        // 48 bytes of hex → 1 block. Treat anything too short to decrypt as
        // "no nick" rather than blowing up the whole account.
        Err(_) => return Ok(SnapshotData::default()),
    };
    if cipher.is_empty() {
        return Ok(SnapshotData::default());
    }
    let (key, iv) = derive_key_iv(openid);
    let plain = match aes_decrypt(&cipher, &key, &iv) {
        Ok(p) => p,
        Err(_) => return Ok(SnapshotData::default()),
    };
    let json_text = match String::from_utf8(plain) {
        Ok(s) => s,
        Err(_) => return Ok(SnapshotData::default()),
    };
    let raw: serde_json::Value = match serde_json::from_str(&json_text) {
        Ok(v) => v,
        Err(_) => return Ok(SnapshotData::default()),
    };
    let obj = match raw.as_object() {
        Some(o) => o,
        None => return Ok(SnapshotData::default()),
    };
    // Try the known key first, then fall back to "first array value" so we
    // stay robust against ACLOS renaming the key prefix.
    let expected_key = format!("key_snapshot_list{}", openid);
    let list = obj
        .get(&expected_key)
        .or_else(|| obj.values().find(|v| v.is_array()));

    let Some(list) = list.and_then(|v| v.as_array()) else {
        return Ok(SnapshotData::default());
    };

    // nick/tag: prefer the chronologically latest record that carries them
    // (matches_time), so a rename is reflected instead of first-wins stale data.
    let mut nick: Option<String> = None;
    let mut tag: Option<String> = None;
    let mut nick_time: i64 = i64::MIN;
    let mut achievements: Vec<SnapshotAchievement> = Vec::new();
    let mut match_details = std::collections::BTreeMap::<String, (i64, SnapshotMatchDetail)>::new();

    for rec in list {
        let snap = rec.get("snapshot").and_then(|v| v.as_object());
        let Some(snap) = snap else { continue };
        let rec_time = rec
            .get("matches_time")
            .and_then(|v| v.as_i64())
            .or_else(|| snap.get("ss_time").and_then(|v| v.as_i64()))
            .unwrap_or(0);
        let cand_nick = snap
            .get("ss_nick")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let cand_tag = snap
            .get("ss_nick_id")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if (cand_nick.is_some() || cand_tag.is_some()) && rec_time >= nick_time {
            nick_time = rec_time;
            if cand_nick.is_some() {
                nick = cand_nick;
            }
            if cand_tag.is_some() {
                tag = cand_tag;
            }
        }
        let mid = rec.get("matches_id").and_then(|v| v.as_str()).unwrap_or("");
        if !mid.is_empty() {
            let achv = snap.get("ss_achieve_type").and_then(|v| v.as_str());
            let is_match = snap.get("ss_type").and_then(|v| v.as_str()) == Some("match");
            let achievement = match achv {
                Some("mvp" | "svp") => achv.map(str::to_owned),
                Some("") if is_match => Some("normal".into()),
                _ => None,
            };
            let result = snap.get("ss_result_infos").filter(|_| is_match);
            let is_draw = result.and_then(|r| r.get("IsDraw")).and_then(snapshot_bool);
            let has_won = result.and_then(|r| r.get("HasWon")).and_then(snapshot_bool);
            if achievement.is_some() || is_draw.is_some() || has_won.is_some() {
                let entry = match_details.entry(mid.into()).or_default();
                if rec_time >= entry.0 || entry.1.matches_id.is_empty() {
                    let previous = &entry.1;
                    *entry = (
                        rec_time,
                        SnapshotMatchDetail {
                            matches_id: mid.into(),
                            achievement: achievement.or_else(|| previous.achievement.clone()),
                            is_draw: is_draw.or(previous.is_draw),
                            has_won: has_won.or(previous.has_won),
                        },
                    );
                } else {
                    if entry.1.achievement.is_none() {
                        entry.1.achievement = achievement;
                    }
                    if entry.1.is_draw.is_none() {
                        entry.1.is_draw = is_draw;
                    }
                    if entry.1.has_won.is_none() {
                        entry.1.has_won = has_won;
                    }
                }
            }
        }
        // Keep the existing positive-only achievement shape for older consumers.
        let achv = snap
            .get("ss_achieve_type")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if achv != "mvp" && achv != "svp" {
            continue;
        }
        let mid = rec.get("matches_id").and_then(|v| v.as_str()).unwrap_or("");
        if mid.is_empty() {
            continue;
        }
        let type_str = snap
            .get("ss_type_str")
            .and_then(|v| v.as_str())
            .unwrap_or(achv)
            .to_string();
        achievements.push(SnapshotAchievement {
            matches_id: mid.to_string(),
            achv_type: achv.to_string(),
            type_str,
        });
    }

    Ok(SnapshotData {
        nick,
        tag,
        achievements,
        match_details: match_details
            .into_values()
            .map(|(_, detail)| detail)
            .collect(),
    })
}

fn snapshot_bool(value: &serde_json::Value) -> Option<bool> {
    match value {
        serde_json::Value::Bool(b) => Some(*b),
        serde_json::Value::Number(n) if n.as_i64() == Some(0) => Some(false),
        serde_json::Value::Number(n) if n.as_i64() == Some(1) => Some(true),
        serde_json::Value::String(s) if s == "0" || s == "false" => Some(false),
        serde_json::Value::String(s) if s == "1" || s == "true" => Some(true),
        _ => None,
    }
}

// ─── tests ────────────────────────────────────────────────────────────────
//
// Mirror `packages/parser/tests/reader.test.ts` — these hit real ACLOS files
// at the user's %APPDATA% so the Rust output is checked against a known
// working set. Tests are no-ops if the files aren't present (CI, fresh
// machines, etc.).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::model::VideoItem;
    use std::path::PathBuf;

    #[test]
    fn only_valid_empty_objects_and_lists_are_empty_highlight_data() {
        for json in ["{}", r#"{"key_wonderful_list_test":[]}"#] {
            let parsed = parse_json(json, "test").unwrap();
            assert!(parsed.matches.is_empty());
            assert!(parsed.raw.is_some());
        }
        for json in [
            "",
            "{",
            "null",
            "[]",
            r#"{"other":true}"#,
            r#"{"key_wonderful_list_test":null}"#,
        ] {
            assert!(
                parse_json(json, "test").is_err(),
                "invalid input accepted: {json}"
            );
        }
    }

    #[test]
    fn source_result_and_score_presence_survive_default_values() {
        let parsed = parse_json(r#"{"key_wonderful_list_test":[{"stats":{}},{"stats":{"has_won":false,"rounds_won":0,"rounds_lost":0}},{"stats":{"has_won":true,"rounds_won":13}}]}"#, "test").unwrap();
        let flags: Vec<_> = parsed
            .matches
            .iter()
            .map(|m| (m.stats.result_known, m.stats.score_known))
            .collect();
        assert_eq!(
            flags,
            vec![
                (Some(false), Some(false)),
                (Some(true), Some(true)),
                (Some(true), Some(false))
            ]
        );
    }

    #[test]
    fn missing_stats_are_distinct_from_explicit_zero() {
        let parsed = parse_json(r#"{"key_wonderful_list_test":[{"stats":{"kills":0,"deaths":0,"assists":0,"score":0}},{"stats":{"kills":0}}]}"#, "test").unwrap();
        assert_eq!(parsed.matches[0].stats.missing_stats, Some(vec![]));
        assert_eq!(
            parsed.matches[1].stats.missing_stats,
            Some(vec!["deaths".into(), "assists".into(), "score".into()])
        );
    }

    #[test]
    fn snapshot_metadata_uses_latest_known_facts_in_any_order() {
        use cbc::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
        let value = serde_json::json!({"key_snapshot_list123": [
            {"matches_id":"award","matches_time":2,"snapshot":{"ss_type":"match","ss_achieve_type":"svp"}},
            {"matches_id":"award","matches_time":1,"snapshot":{"ss_type":"match","ss_achieve_type":"mvp","ss_result_infos":{"HasWon":"0"}}},
            {"matches_id":"unknown","snapshot":{"ss_type":"match","ss_result_infos":{"IsDraw":2,"HasWon":"unknown"}}}
        ]});
        let text = serde_json::to_vec(&value).unwrap();
        let (key, iv) = derive_key_iv("123");
        let mut buf = vec![0; text.len() + 16];
        buf[..text.len()].copy_from_slice(&text);
        let cipher = cbc::Encryptor::<aes::Aes256>::new_from_slices(&key, &iv)
            .unwrap()
            .encrypt_padded_mut::<Pkcs7>(&mut buf, text.len())
            .unwrap();
        let path =
            std::env::temp_dir().join(format!("wui-snapshot-facts-{}", uuid::Uuid::new_v4()));
        let bytes = hex::encode(cipher).into_bytes();
        std::fs::write(&path, &bytes).unwrap();
        let parsed = parse_snapshot_db(&path, "123").unwrap();
        assert_eq!(parsed.match_details.len(), 1);
        assert_eq!(parsed.match_details[0].achievement.as_deref(), Some("svp"));
        assert_eq!(parsed.match_details[0].has_won, Some(false));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        std::fs::remove_file(&path).unwrap();
    }

    fn fixture_dir() -> PathBuf {
        if let Ok(dir) = std::env::var("WONDERFUL_DB_DIR") {
            PathBuf::from(dir)
        } else {
            let base = std::env::var("USERPROFILE")
                .or_else(|_| std::env::var("HOME"))
                .unwrap_or_default();
            PathBuf::from(base).join(r"AppData\Roaming\ACLOS\WonderfulDb")
        }
    }

    fn fixture_path(suffix: &str) -> String {
        let mut p = fixture_dir();
        p.push(suffix);
        p.to_string_lossy().to_string()
    }

    // min_matches is a soft floor only — local WonderfulDb can be wiped.
    const REAL_DBS: &[(&str, &str, usize, &str)] = &[
        ("4807045517549591240", "4807045517549591240", 0, ""),
        ("14121192131852595386", "14121192131852595386", 0, ""),
        ("13794749312275947089", "13794749312275947089", 0, ""),
    ];

    const REAL_SNAPSHOTS: &[(&str, &str, &str, &str)] = &[
        // path_suffix, openid, expected nick, expected tag
        (
            "snapshot4807045517549591240",
            "4807045517549591240",
            "超雄小猫咪",
            "13949",
        ),
        (
            "snapshot14121192131852595386",
            "14121192131852595386",
            "相对论I",
            "65174",
        ),
        (
            // Local ACLOS may grow this snapshot over time (nick/tag/achievements).
            "snapshot13794749312275947089",
            "13794749312275947089",
            "Smoky",
            "46211",
        ),
        (
            // 1228... is a legacy account whose WonderfulDb has zero matches;
            // the snapshot file is exactly 48 bytes of ciphertext (empty
            // JSON). parse_snapshot_db must return (None, None), not error.
            "snapshot1228584785010313960",
            "1228584785010313960",
            "",
            "",
        ),
    ];

    #[test]
    fn null_round_honors_do_not_reject_the_account() {
        let payload = serde_json::json!({
            "key_wonderful_list_test": [{
                "matches_id": "match-with-null-honors",
                "videos": [{
                    "video_id": "video-1",
                    "rounds": [
                        { "round_id": 1, "round_honors": null,
                          "round_clips": [{ "clip_id": "clip-1" }] },
                        { "round_id": 2, "round_honors": [] },
                        { "round_id": 3 },
                        { "round_id": 4, "round_honors": [{
                            "honor_id": "honor-1", "honor_name": "Triple kill"
                        }] }
                    ]
                }]
            }, {
                "matches_id": "another-match",
                "videos": [{ "video_id": "video-2" }]
            }]
        });
        let result = parse_json(&payload.to_string(), "test").unwrap();
        assert_eq!(result.matches.len(), 2);
        let rounds = &result.matches[0].videos[0].rounds;
        assert_eq!(rounds.len(), 4);
        assert!(rounds[..3]
            .iter()
            .all(|round| round.round_honors.is_empty()));
        assert_eq!(rounds[0].round_clips[0].clip_id, "clip-1");
        assert_eq!(rounds[3].round_honors[0].honor_id, "honor-1");
        assert_eq!(result.matches[1].videos[0].video_id, "video-2");
        assert!(
            result.raw.unwrap()["key_wonderful_list_test"][0]["videos"][0]["rounds"][0]
                ["round_honors"]
                .is_null()
        );
    }

    #[test]
    fn invalid_round_honors_still_report_an_error() {
        for honors in [serde_json::json!("invalid"), serde_json::json!({})] {
            let payload = serde_json::json!({
                "key_wonderful_list_test": [{
                    "videos": [{ "rounds": [{ "round_honors": honors }] }]
                }]
            });
            assert!(parse_json(&payload.to_string(), "test").is_err());
        }
    }

    #[test]
    fn hex_decode_roundtrip_and_errors() {
        assert_eq!(decode_hex("").unwrap(), Vec::<u8>::new());
        assert_eq!(decode_hex("48656c6c6f").unwrap(), b"Hello");
        assert_eq!(decode_hex("00ff").unwrap(), vec![0x00, 0xff]);
        assert_eq!(decode_hex("00FF").unwrap(), vec![0x00, 0xff]);
        assert!(decode_hex("abc").is_err());
        assert!(decode_hex("zz").is_err());
    }

    #[test]
    fn key_iv_is_32_and_16_bytes() {
        let (k, iv) = derive_key_iv("4807045517549591240");
        assert_eq!(k.len(), 32);
        assert_eq!(iv.len(), 16);
    }

    #[test]
    fn video_processing_field_matches_ts_model_name() {
        let video: VideoItem = serde_json::from_value(serde_json::json!({
            "video_isProcessing": true
        }))
        .expect("video JSON should deserialize");
        assert_eq!(video.video_is_processing, Some(true));

        let out = serde_json::to_value(&video).expect("video should serialize");
        assert_eq!(
            out.get("video_isProcessing").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert!(out.get("video_is_processing").is_none());
    }

    #[test]
    fn real_wonderful_db_files() {
        for (path_suffix, openid, min_matches, expected_agent) in REAL_DBS {
            let path = PathBuf::from(fixture_path(path_suffix));
            if !path.exists() {
                eprintln!("skip (file not present): {}", path.display());
                continue;
            }
            let result = parse_wonderful_db(&path, openid).expect("parse should succeed");
            eprintln!("\n--- {} ---", openid);
            eprintln!("key: {}", result.key);
            eprintln!("matches: {}", result.matches.len());
            if let Some(m) = result.matches.first() {
                eprintln!(
                    "  [0] {}  {} {}  {}/{}/{}  win={}  videos={}",
                    m.matches_id,
                    m.agent.agent_name,
                    m.map.map_id,
                    m.stats.kills,
                    m.stats.deaths,
                    m.stats.assists,
                    m.stats.has_won,
                    m.videos.len()
                );
            }
            assert!(
                result.matches.len() >= *min_matches,
                "{}: expected at least {} matches, got {}",
                openid,
                min_matches,
                result.matches.len()
            );
            if !expected_agent.is_empty() {
                assert_eq!(
                    result.matches[0].agent.agent_name, *expected_agent,
                    "{}: first match agent mismatch",
                    openid
                );
            }
        }
    }

    #[test]
    fn real_snapshot_db_files() {
        for (path_suffix, openid, expected_nick, expected_tag) in REAL_SNAPSHOTS {
            let path = PathBuf::from(fixture_path(path_suffix));
            if !path.exists() {
                eprintln!("skip (file not present): {}", path.display());
                continue;
            }
            let data = parse_snapshot_db(&path, openid)
                .unwrap_or_else(|e| panic!("parse_snapshot_db({}) failed: {}", openid, e));
            eprintln!(
                "\n--- {} ---\n  nick: {:?}\n  tag:  {:?}\n  achievements: {}",
                openid,
                data.nick,
                data.tag,
                data.achievements.len()
            );
            let exp_nick: Option<&str> = if expected_nick.is_empty() {
                None
            } else {
                Some(expected_nick)
            };
            let exp_tag: Option<&str> = if expected_tag.is_empty() {
                None
            } else {
                Some(expected_tag)
            };
            assert_eq!(data.nick.as_deref(), exp_nick, "{}: nick mismatch", openid);
            assert_eq!(data.tag.as_deref(), exp_tag, "{}: tag mismatch", openid);
            // Prefer non-empty nick when expected; achievements are optional for
            // newer snapshots that ACLOS may populate later.
            if openid == &"4807045517549591240" || openid == &"14121192131852595386" {
                assert!(
                    !data.achievements.is_empty(),
                    "{}: should have achievements",
                    openid
                );
            }
        }
    }
}
