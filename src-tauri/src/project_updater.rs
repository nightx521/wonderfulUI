use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::{Manager, ResourceId, Webview};
use tauri_plugin_updater::UpdaterExt;

const PROJECT: &str = "https://github.com/nightx521/wonderfulUI";
const PROXY: &str = "https://github.murainy.com/";

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    Proxy,
    Github,
}

fn endpoint(channel: UpdateChannel) -> String {
    let official = format!("{PROJECT}/releases/latest/download/latest.json");
    match channel {
        UpdateChannel::Proxy => format!("{PROXY}{official}"),
        UpdateChannel::Github => official,
    }
}

fn installer_url(
    channel: UpdateChannel,
    advertised: &str,
    version: &str,
) -> Result<tauri::Url, String> {
    let official =
        format!("{PROJECT}/releases/download/v{version}/WonderfulUI_{version}_x64-setup.exe");
    if advertised != official && advertised != format!("{PROXY}{official}") {
        return Err("更新安装包地址与项目版本不符".into());
    }
    let routed = match channel {
        UpdateChannel::Proxy => format!("{PROXY}{official}"),
        UpdateChannel::Github => official,
    };
    tauri::Url::parse(&routed).map_err(|_| "无效更新地址".into())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMetadata {
    rid: ResourceId,
    current_version: String,
    version: String,
    date: Option<String>,
    body: Option<String>,
    raw_json: serde_json::Value,
}

#[tauri::command]
pub async fn check_project_update(
    webview: Webview,
    channel: UpdateChannel,
) -> Result<Option<UpdateMetadata>, String> {
    let endpoint = tauri::Url::parse(&endpoint(channel)).map_err(|e| e.to_string())?;
    let updater = webview
        .updater_builder()
        .endpoints(vec![endpoint])
        .map_err(|e| e.to_string())?
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let Some(mut update) = updater.check().await.map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    update.download_url = installer_url(channel, update.download_url.as_str(), &update.version)?;
    update.timeout = Some(Duration::from_secs(300));
    let metadata = UpdateMetadata {
        rid: 0,
        current_version: update.current_version.clone(),
        version: update.version.clone(),
        date: update
            .raw_json
            .get("pub_date")
            .and_then(|v| v.as_str())
            .map(str::to_owned),
        body: update.body.clone(),
        raw_json: update.raw_json.clone(),
    };
    // The official Update resource retains signature verification during download/install.
    Ok(Some(UpdateMetadata {
        rid: webview.resources_table().add(update),
        ..metadata
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn both_routes_use_the_owned_project_and_same_release() {
        let github = endpoint(UpdateChannel::Github);
        assert_eq!(endpoint(UpdateChannel::Proxy), format!("{PROXY}{github}"));
        let direct = format!("{PROJECT}/releases/download/v1.2.3/WonderfulUI_1.2.3_x64-setup.exe");
        let proxied = format!("{PROXY}{direct}");
        for advertised in [&direct, &proxied] {
            assert_eq!(
                installer_url(UpdateChannel::Proxy, advertised, "1.2.3")
                    .unwrap()
                    .as_str(),
                proxied
            );
            assert_eq!(
                installer_url(UpdateChannel::Github, advertised, "1.2.3")
                    .unwrap()
                    .as_str(),
                direct
            );
        }
    }
    #[test]
    fn rejects_unrelated_insecure_credentialed_and_wrong_version_urls() {
        let valid = format!("{PROJECT}/releases/download/v1.2.3/WonderfulUI_1.2.3_x64-setup.exe");
        for invalid in [
            valid.replace("https:", "http:"),
            valid.replace("nightx521", "other"),
            valid.replace("github.com", "github.com.example.org"),
            valid.replace("https://", "https://user:pass@"),
            format!("{valid}?token=fixture"),
            format!("{valid}#fragment"),
            valid.replace("v1.2.3/", "v1.2.2/"),
        ] {
            assert!(installer_url(UpdateChannel::Proxy, &invalid, "1.2.3").is_err());
        }
        assert!(serde_json::from_str::<UpdateChannel>("\"https://example.org\"").is_err());
    }
}
