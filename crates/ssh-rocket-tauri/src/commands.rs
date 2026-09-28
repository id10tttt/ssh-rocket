use crate::{
    controller::RuntimeController,
    rules::import_rule_source,
    scanner::scan_desktop_apps,
    types::{DesktopAppDto, RuleImportSummaryDto, RuntimeStatusDto},
};
use ssh_rocket_core::AppConfig;
use tauri::{AppHandle, Manager, State};

pub struct AppState {
    pub controller: RuntimeController,
}

#[tauri::command]
pub async fn get_config() -> Result<AppConfig, String> {
    AppConfig::load().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_config(config: AppConfig, state: State<'_, AppState>) -> Result<(), String> {
    config.save().map_err(|e| e.to_string())?;
    state.controller.sync_rules().await;
    Ok(())
}

#[tauri::command]
pub async fn get_runtime_status(state: State<'_, AppState>) -> Result<RuntimeStatusDto, String> {
    let is_running = state.controller.is_running();
    let status_text = state.controller.get_status_text().await;
    Ok(RuntimeStatusDto {
        is_running,
        status_text,
    })
}

#[tauri::command]
pub async fn start_service(
    app: AppHandle,
    profile_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let config = AppConfig::load().map_err(|e| e.to_string())?;
    let profile = if let Some(id) = profile_id {
        config
            .profiles
            .iter()
            .find(|p| p.id.to_string() == id)
            .cloned()
            .ok_or("指定节点不存在")?
    } else {
        config.active_profile().cloned().ok_or("未找到有效节点")?
    };

    state.controller.start(app, profile, config).await;
    Ok(())
}

#[tauri::command]
pub async fn stop_service(state: State<'_, AppState>) -> Result<(), String> {
    state.controller.stop().await;
    Ok(())
}

#[tauri::command]
pub async fn get_desktop_apps() -> Result<Vec<DesktopAppDto>, String> {
    Ok(scan_desktop_apps())
}

#[tauri::command]
pub async fn import_rules(url: String, state: State<'_, AppState>) -> Result<RuleImportSummaryDto, String> {
    let result = import_rule_source(&url)?;
    let mut config = AppConfig::load().map_err(|e| e.to_string())?;
    config.settings.rule_source_url = url.clone();
    config.settings.rule_source_name = url
        .split(['?', '#'])
        .next()
        .and_then(|u| u.rsplit('/').find(|part| !part.is_empty()))
        .unwrap_or("订阅规则")
        .to_string();
    config.settings.imported_domain_rules = result.domain_rules.clone();
    config.settings.imported_ip_rules = result.ip_rules.clone();
    config.save().map_err(|e| e.to_string())?;
    state.controller.sync_rules().await;
    Ok(RuleImportSummaryDto {
        rule_count: result.rule_count(),
        warnings: result.warnings,
        ignored_count: result.ignored_count,
    })
}

#[tauri::command]
pub async fn toggle_floating_window(app: AppHandle) -> Result<bool, String> {
    if let Some(window) = app.get_webview_window("floating") {
        let is_visible = window.is_visible().map_err(|e| e.to_string())?;
        if is_visible {
            window.hide().map_err(|e| e.to_string())?;
            Ok(false)
        } else {
            window.show().map_err(|e| e.to_string())?;
            window.set_focus().map_err(|e| e.to_string())?;
            Ok(true)
        }
    } else {
        Err("悬浮窗口不存在".into())
    }
}
