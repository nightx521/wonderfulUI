//! Confirm warnings in our own index; ACLOS records and media stay unchanged.
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use std::collections::HashSet;

fn records(error: &str) -> Option<Vec<Value>> {
    let value: Value = serde_json::from_str(error).ok()?;
    let records = value.get("records")?.as_array()?;
    if records.is_empty() || records.len() != value.get("skipped_records")?.as_u64()? as usize {
        return None;
    }
    if records.iter().any(|r| {
        r.get("fingerprint")
            .and_then(Value::as_str)
            .is_none_or(|f| f.len() != 64 || !f.bytes().all(|b| b.is_ascii_hexdigit()))
    }) {
        return None;
    }
    Some(records.clone())
}
pub fn visible_error(
    conn: &Connection,
    openid: &str,
    error: &str,
) -> rusqlite::Result<Option<String>> {
    let Some(records) = records(error) else {
        return Ok(Some(error.into()));
    };
    let mut statement =
        conn.prepare("SELECT fingerprint FROM ignored_record_warnings WHERE openid=?1")?;
    let known = statement
        .query_map([openid], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<HashSet<_>>>()?;
    let remaining: Vec<_> = records
        .into_iter()
        .filter(|r| !known.contains(r["fingerprint"].as_str().unwrap()))
        .collect();
    if remaining.is_empty() {
        return Ok(None);
    }
    let mut next: Value = serde_json::from_str(error).unwrap();
    next["skipped_records"] = remaining.len().into();
    for reason in [
        "missing_match_ids",
        "duplicate_match_ids",
        "conflicting_records",
    ] {
        next[reason] = remaining
            .iter()
            .filter(|r| r["reason"] == reason)
            .count()
            .into();
    }
    next["records"] = remaining.into();
    Ok(Some(next.to_string()))
}
pub fn confirm(conn: &Connection, openid: &str, expected: &str) -> Result<(), String> {
    let transaction = conn
        .unchecked_transaction()
        .map_err(|_| "暂时无法保存忽略设置")?;
    let error:Option<String>=transaction.query_row("SELECT parse_error FROM accounts WHERE openid=?1 AND parse_error_code='partial_records'",[openid],|r|r.get(0))
        .optional().map_err(|_|"暂时无法读取异常记录")?.flatten();
    let error = error.ok_or("异常记录已更新，请刷新后重试")?;
    let visible =
        visible_error(&transaction, openid, &error).map_err(|_| "暂时无法读取异常记录")?;
    let value: Value =
        serde_json::from_str(expected).map_err(|_| "异常记录已更新，请刷新后重试")?;
    let current: Value =
        serde_json::from_str(visible.as_deref().ok_or("异常记录已更新，请刷新后重试")?)
            .map_err(|_| "暂时无法读取异常记录")?;
    if current != value {
        return Err("异常记录已更新，请刷新后重试".into());
    }
    let records = records(&error).ok_or("请先执行全量扫描，再确认忽略异常记录")?;
    for record in records {
        transaction.execute("INSERT OR IGNORE INTO ignored_record_warnings(openid,fingerprint,confirmed_at) VALUES(?1,?2,?3)",params![openid,record["fingerprint"].as_str().unwrap(),super::now_ms()])
            .map_err(|_|"暂时无法保存忽略设置")?;
    }
    transaction
        .commit()
        .map_err(|_| "暂时无法保存忽略设置".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_confirmed_records_are_silenced_and_new_errors_are_still_visible() {
        let conn = Connection::open_in_memory().unwrap();
        super::super::db::migrate(&conn).unwrap();
        let error=serde_json::json!({"skipped_records":1,"records":[{"fingerprint":"a".repeat(64),"reason":"missing_match_ids"}]}).to_string();
        conn.execute("INSERT INTO accounts(openid,source_id,last_seen_at,parse_error,parse_error_code)VALUES('fixture','aclos',0,?1,'partial_records')",[&error]).unwrap();
        let visible = visible_error(&conn, "fixture", &error).unwrap().unwrap();
        assert!(confirm(&conn, "fixture", "{}").is_err());
        confirm(&conn, "fixture", &visible).unwrap();
        assert!(visible_error(&conn, "fixture", &error).unwrap().is_none());
        let mut changed: Value = serde_json::from_str(&error).unwrap();
        changed["records"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"fingerprint":"b".repeat(64),"reason":"conflicting_records"}));
        changed["skipped_records"] = 2.into();
        let visible = visible_error(&conn, "fixture", &changed.to_string())
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&visible).unwrap()["skipped_records"],
            1
        );
        let stored: String = conn
            .query_row(
                "SELECT parse_error FROM accounts WHERE openid='fixture'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, error);
    }
}
