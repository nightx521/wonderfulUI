//! ACLOS account identity (nick / #tag) resolution.
//!
//! Structured Local Storage fallback for accounts without a named snapshot.
//! The live database is only copied. LevelDB opens the temporary copy.

use rusty_leveldb::{LdbIterator, Options, DB};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Display name + Riot-style #tag for one openid.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IdentityHint {
    pub nick: Option<String>,
    pub tag: Option<String>,
}

#[derive(Debug, Clone)]
struct ScoredIdentity {
    nick: Option<String>,
    tag: Option<String>,
    /// Higher = preferred. LevelDB roles/highlight keys use high scores.
    score: u32,
}

#[derive(Debug, Default)]
pub struct AclosIdentityIndex {
    exact: HashMap<String, ScoredIdentity>,
}

const SCORE_LEVELDB_ROLES: u32 = 100;
const SCORE_LEVELDB_HIGHLIGHT_USER: u32 = 90;

impl AclosIdentityIndex {
    pub fn load_default() -> Self {
        let Some(root) = aclos_roaming_dir() else {
            return Self::default();
        };
        Self::load_from_aclos_root(&root)
    }

    pub fn load_from_aclos_root(aclos_root: &Path) -> Self {
        let mut idx = Self::default();

        // 1) Formal Local Storage LevelDB (primary for nick/tag).
        let ls = aclos_root.join("Local Storage").join("leveldb");
        if ls.is_dir() {
            load_from_local_storage_leveldb(&ls, &mut idx);
        }

        idx
    }

    pub fn lookup(&self, openid: &str) -> IdentityHint {
        self.exact.get(openid).map(|identity| IdentityHint {
            nick: identity.nick.clone(),
            tag: identity.tag.clone(),
        }).unwrap_or_default()
    }

    pub fn merge_with_snapshot(
        &self,
        openid: &str,
        snap_nick: Option<String>,
        snap_tag: Option<String>,
    ) -> (Option<String>, Option<String>) {
        if snap_nick.is_some() {
            return (snap_nick, snap_tag);
        }
        let cached = self.lookup(openid);
        (cached.nick, cached.tag)
    }
}

// ─── LevelDB Local Storage ───────────────────────────────────────────────────

fn load_from_local_storage_leveldb(dir: &Path, idx: &mut AclosIdentityIndex) {
    // rusty-leveldb has no read-only open and may create/update LOCK or log
    // files even with create_if_missing=false. Always operate on a copy so
    // WonderfulUI never mutates ACLOS-owned storage.
    let Ok(tmp) = copy_leveldb_to_temp(dir) else {
        return;
    };
    let _ = try_open_local_storage(&tmp, idx);
    let _ = std::fs::remove_dir_all(&tmp);
}

fn try_open_local_storage(dir: &Path, idx: &mut AclosIdentityIndex) -> bool {
    let mut opts = Options::default();
    opts.create_if_missing = false;
    // This path is always app-owned temporary storage; ignore open errors and
    // let the secondary read-only text harvest provide a best-effort fallback.
    let mut db = match DB::open(dir, opts) {
        Ok(db) => db,
        Err(_) => return false,
    };
    let mut iter = match db.new_iter() {
        Ok(it) => it,
        Err(_) => return false,
    };
    // Prefer advance/current for efficiency per crate docs.
    iter.reset();
    while iter.advance() {
        let Some((key, value)) = iter.current() else {
            continue;
        };
        let key_name = decode_chromium_ls_key_name(key.as_ref());
        if key_name.is_empty() {
            continue;
        }
        if key_name == "ACLOS_USER_ROLES_INFO" {
            if let Some(text) = decode_chromium_ls_value(value.as_ref()) {
                ingest_roles_json(&text, idx, SCORE_LEVELDB_ROLES);
            }
            continue;
        }
        if let Some(openid) = key_name.strip_prefix("acloshighlight_user_") {
            if is_full_openid(openid) {
                if let Some(text) = decode_chromium_ls_value(value.as_ref()) {
                    ingest_single_role_json(&text, openid, idx, SCORE_LEVELDB_HIGHLIGHT_USER);
                }
            }
        }
    }
    true
}

fn copy_leveldb_to_temp(src: &Path) -> Result<PathBuf, std::io::Error> {
    let tmp = std::env::temp_dir().join(format!("wui-aclos-ls-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&tmp)?;

    let copy_result = (|| {
        for ent in std::fs::read_dir(src)? {
            let ent = ent?;
            let name = ent.file_name();
            let name_s = name.to_string_lossy();
            // Skip live LOCK so we can open the copy.
            if name_s == "LOCK" {
                continue;
            }
            // Do not follow links out of the ACLOS-owned database directory.
            if !ent.file_type()?.is_file() {
                continue;
            }
            std::fs::copy(ent.path(), tmp.join(&name))?;
        }
        Ok::<(), std::io::Error>(())
    })();

    if let Err(error) = copy_result {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(error);
    }
    Ok(tmp)
}

/// Chromium Local Storage key: `_` + origin + `\0` + optional `\x01` + name.
fn decode_chromium_ls_key_name(key: &[u8]) -> String {
    if let Some(nul) = key.iter().position(|&b| b == 0) {
        let mut i = nul + 1;
        if i < key.len() && key[i] == 1 {
            i += 1;
        }
        return String::from_utf8_lossy(&key[i..]).into_owned();
    }
    String::from_utf8_lossy(key).into_owned()
}

/// Chromium Local Storage value: type byte + payload.
///
/// Observed ACLOS encodings (both real on one machine, generic decode):
/// - `0x01` + UTF-8 JSON — common for `app://aclos.val.qq.com` keys
/// - `0x00` + UTF-16LE JSON — common for `https://val.qq.com` keys such as
///   `acloshighlight_user_<openid>` (most multi-account nick rows)
/// - bare UTF-16LE / UTF-8 JSON without type byte (rare)
fn decode_chromium_ls_value(value: &[u8]) -> Option<String> {
    if value.is_empty() {
        return None;
    }

    // Type 1 = string (Chromium). Prefer UTF-8 JSON, then UTF-16LE.
    if value[0] == 1 && value.len() > 1 {
        let rest = &value[1..];
        if let Some(s) = jsonish_utf8(rest) {
            return Some(s);
        }
        if let Some(s) = jsonish_utf16le(rest) {
            return Some(s);
        }
        return Some(String::from_utf8_lossy(rest).into_owned());
    }

    // Type 0 on disk for some origins still carries a string payload as UTF-16LE
    // (not a JSON null). Prefer UTF-16LE when it looks like JSON.
    if value[0] == 0 && value.len() > 1 {
        let rest = &value[1..];
        if let Some(s) = jsonish_utf16le(rest) {
            return Some(s);
        }
        if let Some(s) = jsonish_utf8(rest) {
            return Some(s);
        }
    }

    // Bare payloads
    if let Some(s) = jsonish_utf16le(value) {
        return Some(s);
    }
    if let Some(s) = jsonish_utf8(value) {
        return Some(s);
    }
    None
}

fn looks_like_json_object_or_array(s: &str) -> bool {
    let t = s.trim_start();
    t.starts_with('{') || t.starts_with('[') || t.contains("openid") || t.contains("nick")
}

fn jsonish_utf8(bytes: &[u8]) -> Option<String> {
    let s = String::from_utf8_lossy(bytes);
    if looks_like_json_object_or_array(&s) {
        Some(s.into_owned())
    } else {
        None
    }
}

fn jsonish_utf16le(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 2 {
        return None;
    }
    // Odd trailing byte: ignore last (common with type-prefix framing).
    let even = &bytes[..bytes.len() - (bytes.len() % 2)];
    if even.is_empty() {
        return None;
    }
    let u16s: Vec<u16> = even
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let s = String::from_utf16_lossy(&u16s);
    if looks_like_json_object_or_array(&s) {
        Some(s)
    } else {
        None
    }
}

fn ingest_roles_json(text: &str, idx: &mut AclosIdentityIndex, score: u32) {
    let text = text.trim_start_matches(['\u{0}', '\u{1}']).trim();
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        // Substring array if framing junk remains
        if let (Some(i), Some(j)) = (text.find('['), text.rfind(']')) {
            if j > i {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text[i..=j]) {
                    ingest_roles_value(&v, idx, score);
                }
            }
        }
        return;
    };
    ingest_roles_value(&v, idx, score);
}

fn ingest_roles_value(v: &serde_json::Value, idx: &mut AclosIdentityIndex, score: u32) {
    let list = if let Some(arr) = v.as_array() {
        arr.as_slice()
    } else if let Some(arr) = v.get("data").and_then(|x| x.as_array()) {
        arr.as_slice()
    } else if let Some(arr) = v.get("role_list").and_then(|x| x.as_array()) {
        arr.as_slice()
    } else if v.is_object() {
        ingest_one_role_obj(v, None, idx, score);
        return;
    } else {
        return;
    };
    for item in list {
        ingest_one_role_obj(item, None, idx, score);
    }
}

fn ingest_single_role_json(
    text: &str,
    openid_from_key: &str,
    idx: &mut AclosIdentityIndex,
    score: u32,
) {
    let text = text.trim_start_matches(['\u{0}', '\u{1}']).trim();
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };
    ingest_one_role_obj(&v, Some(openid_from_key), idx, score);
}

fn ingest_one_role_obj(
    v: &serde_json::Value,
    openid_hint: Option<&str>,
    idx: &mut AclosIdentityIndex,
    score: u32,
) {
    let openid = v
        .get("openid")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .or_else(|| openid_hint.map(|s| s.to_string()));
    let Some(openid) = openid else {
        return;
    };
    if !is_full_openid(&openid) || openid_hint.is_some_and(|hint| hint != openid) {
        return;
    }
    let nick = v
        .get("nick")
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| is_plausible_nick(s));
    let tag = v
        .get("nick_id")
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| is_plausible_tag(s));
    if nick.is_none() {
        return;
    }
    record_exact(idx, &openid, nick, tag, score);
}

fn aclos_roaming_dir() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    let p = PathBuf::from(home)
        .join("AppData")
        .join("Roaming")
        .join("ACLOS");
    if p.is_dir() {
        Some(p)
    } else {
        None
    }
}

fn is_full_openid(s: &str) -> bool {
    let b = s.as_bytes();
    (15..=25).contains(&b.len()) && b.iter().all(|c| c.is_ascii_digit())
}

fn is_plausible_nick(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() || t.len() > 40 {
        return false;
    }
    if t == "我" || t.eq_ignore_ascii_case("null") {
        return false;
    }
    true
}

fn is_plausible_tag(s: &str) -> bool {
    let b = s.as_bytes();
    (3..=8).contains(&b.len()) && b.iter().all(|c| c.is_ascii_digit())
}

fn record_exact(
    idx: &mut AclosIdentityIndex,
    openid: &str,
    nick: Option<String>,
    tag: Option<String>,
    score: u32,
) {
    let replace = idx.exact.get(openid).is_none_or(|previous| {
        score > previous.score
            || (score == previous.score && previous.tag.is_none() && tag.is_some())
    });
    if replace {
        idx.exact.insert(openid.to_owned(), ScoredIdentity { nick, tag, score });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OID_A: &str = "1000000000000000001";
    const OID_B: &str = "2000000000000000002";

    #[test]
    fn decode_chromium_ls_value_utf8_json() {
        // type=1 + UTF-8 JSON (app://aclos.val.qq.com style)
        let mut raw = vec![1u8];
        raw.extend_from_slice(
            br#"[{"openid":"1000000000000000001","nick":"TestNick","nick_id":"12345"}]"#,
        );
        let s = decode_chromium_ls_value(&raw).expect("decode");
        let mut idx = AclosIdentityIndex::default();
        ingest_roles_json(&s, &mut idx, SCORE_LEVELDB_ROLES);
        let h = idx.lookup(OID_A);
        assert_eq!(h.nick.as_deref(), Some("TestNick"));
        assert_eq!(h.tag.as_deref(), Some("12345"));
    }

    #[test]
    fn decode_chromium_ls_value_type0_utf16le_json() {
        // type=0 + UTF-16LE JSON (https://val.qq.com acloshighlight_user_* style)
        let json = r#"{"openid":"2000000000000000002","nick":"Beta","nick_id":"67890"}"#;
        let mut raw = vec![0u8];
        for u in json.encode_utf16() {
            raw.extend_from_slice(&u.to_le_bytes());
        }
        let s = decode_chromium_ls_value(&raw).expect("decode type0 utf16");
        let mut idx = AclosIdentityIndex::default();
        ingest_single_role_json(&s, OID_B, &mut idx, SCORE_LEVELDB_HIGHLIGHT_USER);
        let h = idx.lookup(OID_B);
        assert_eq!(h.nick.as_deref(), Some("Beta"));
        assert_eq!(h.tag.as_deref(), Some("67890"));
    }

    #[test]
    fn decode_chromium_ls_key_name_with_origin_prefix() {
        let mut key = b"_app://aclos.val.qq.com".to_vec();
        key.push(0);
        key.push(1);
        key.extend_from_slice(b"acloshighlight_user_2000000000000000002");
        assert_eq!(
            decode_chromium_ls_key_name(&key),
            "acloshighlight_user_2000000000000000002"
        );
    }

    #[test]
    fn ingest_highlight_user_object() {
        let json = r#"{"openid":"2000000000000000002","nick":"Beta","nick_id":"67890"}"#;
        let mut idx = AclosIdentityIndex::default();
        ingest_single_role_json(json, OID_B, &mut idx, SCORE_LEVELDB_HIGHLIGHT_USER);
        let h = idx.lookup(OID_B);
        assert_eq!(h.nick.as_deref(), Some("Beta"));
        assert_eq!(h.tag.as_deref(), Some("67890"));
    }

    #[test]
    fn snapshot_identity_preferred_as_a_pair() {
        let mut idx = AclosIdentityIndex::default();
        record_exact(
            &mut idx,
            OID_A,
            Some("FromLevelDb".into()),
            Some("11111".into()),
            SCORE_LEVELDB_HIGHLIGHT_USER,
        );
        let (nick, tag) =
            idx.merge_with_snapshot(OID_A, Some("FromSnapshot".into()), Some("22222".into()));
        assert_eq!(nick.as_deref(), Some("FromSnapshot"));
        assert_eq!(tag.as_deref(), Some("22222"));
        let (_, tag) = idx.merge_with_snapshot(OID_A, Some("NewName".into()), None);
        assert_eq!(tag, None);
    }

    #[test]
    fn leveldb_identity_load_never_mutates_source_directory() {
        let root =
            std::env::temp_dir().join(format!("wui-aclos-readonly-test-{}", uuid::Uuid::new_v4()));
        let leveldb = root.join("Local Storage").join("leveldb");
        std::fs::create_dir_all(&leveldb).expect("fixture directory");

        let mut options = Options::default();
        options.create_if_missing = true;
        {
            let mut db = DB::open(&leveldb, options).expect("fixture leveldb opens");
            let mut key = b"_app://aclos.val.qq.com".to_vec();
            key.push(0);
            key.push(1);
            key.extend_from_slice(b"acloshighlight_user_1000000000000000001");
            let mut value = vec![1u8];
            value.extend_from_slice(
                br#"{"openid":"1000000000000000001","nick":"ReadOnly","nick_id":"54321"}"#,
            );
            db.put(&key, &value).expect("fixture row inserted");
            db.flush().expect("fixture leveldb flushed");
        }
        let before = directory_snapshot(&leveldb);

        let index = AclosIdentityIndex::load_from_aclos_root(&root);

        assert_eq!(index.lookup(OID_A).nick.as_deref(), Some("ReadOnly"));
        assert_eq!(directory_snapshot(&leveldb), before);
        std::fs::remove_dir_all(&root).expect("fixture removed");
    }

    fn directory_snapshot(path: &Path) -> Vec<(String, Vec<u8>)> {
        let mut files = std::fs::read_dir(path)
            .expect("snapshot directory")
            .map(|entry| entry.expect("snapshot entry"))
            .filter(|entry| entry.path().is_file())
            .map(|entry| {
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    std::fs::read(entry.path()).expect("snapshot file"),
                )
            })
            .collect::<Vec<_>>();
        files.sort_by(|left, right| left.0.cmp(&right.0));
        files
    }

    #[test]
    fn snapshot_used_when_leveldb_missing() {
        let idx = AclosIdentityIndex::default();
        let (nick, tag) =
            idx.merge_with_snapshot(OID_A, Some("SnapOnly".into()), Some("33333".into()));
        assert_eq!(nick.as_deref(), Some("SnapOnly"));
        assert_eq!(tag.as_deref(), Some("33333"));
    }

    /// Optional: when this machine has ACLOS Local Storage, LevelDB open must
    /// not panic. Does not assert specific openids (not a one-PC fixture).
    #[test]
    fn load_default_does_not_panic() {
        let idx = AclosIdentityIndex::load_default();
        let _ = idx.lookup("0");
    }
}
