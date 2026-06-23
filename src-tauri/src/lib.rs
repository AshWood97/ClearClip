mod diagnostics;
mod runninghub;
mod settings;
mod task_manager;

use diagnostics::{current_timestamp, DiagnosticsLogger};
use runninghub::{NodeInspection, RunningHubClient};
use settings::{AppSettings, ModelOverride, SettingsSnapshot, SettingsStore};
use std::path::PathBuf;
use std::sync::Arc;
use task_manager::{TaskEvent, TaskManager, TaskRecord};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;
use tokio::sync::mpsc;

const DEFAULT_VALIDATE_APP_ID: &str = "2059943323456065537";

pub struct AppState {
    task_manager: TaskManager,
    settings: Arc<SettingsStore>,
    client: RunningHubClient,
    app_data_dir: PathBuf,
}

#[tauri::command]
async fn submit_video_task(
    state: State<'_, AppState>,
    app_id: String,
    app_name: String,
    file_path: String,
    params: serde_json::Value,
) -> Result<String, String> {
    state
        .task_manager
        .submit_and_poll(app_id, app_name, PathBuf::from(file_path), params)
        .await
}

#[tauri::command]
async fn save_api_key(
    state: State<'_, AppState>,
    api_key: String,
) -> Result<SettingsSnapshot, String> {
    if api_key.trim().is_empty() {
        return Err("API Key 不能为空。".into());
    }
    if let Err(error) = state
        .client
        .validate_api_key(api_key.trim(), DEFAULT_VALIDATE_APP_ID)
        .await
    {
        let message = error.to_string();
        let _ = state.settings.mark_api_key_error(message.clone()).await;
        return Err(format!("API Key 验证失败：{message}"));
    }
    settings::save_api_key(&api_key).map_err(|error| error.to_string())?;
    state
        .settings
        .mark_api_key_verified(current_timestamp())
        .await
        .map_err(|error| error.to_string())?;
    state
        .settings
        .snapshot()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn clear_api_key(state: State<'_, AppState>) -> Result<SettingsSnapshot, String> {
    settings::clear_api_key().map_err(|error| error.to_string())?;
    state
        .settings
        .clear_api_key_state()
        .await
        .map_err(|error| error.to_string())?;
    state
        .settings
        .snapshot()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn has_api_key() -> Result<bool, String> {
    settings::has_api_key().map_err(|error| error.to_string())
}

#[tauri::command]
async fn get_app_settings(state: State<'_, AppState>) -> Result<SettingsSnapshot, String> {
    state
        .settings
        .snapshot()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn save_model_override(
    state: State<'_, AppState>,
    app_id: String,
    override_config: Option<ModelOverride>,
) -> Result<AppSettings, String> {
    state
        .settings
        .save_model_override(app_id, override_config)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn validate_api_key(
    state: State<'_, AppState>,
    api_key: String,
) -> Result<NodeInspection, String> {
    if api_key.trim().is_empty() {
        return Err("API Key 不能为空。".into());
    }
    state
        .client
        .validate_api_key(api_key.trim(), DEFAULT_VALIDATE_APP_ID)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn inspect_model_nodes(
    state: State<'_, AppState>,
    app_id: String,
) -> Result<NodeInspection, String> {
    let api_key = settings::get_api_key().map_err(|error| error.to_string())?;
    state
        .client
        .inspect_model_nodes(&api_key, &app_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn list_tasks(state: State<'_, AppState>) -> Result<Vec<TaskRecord>, String> {
    Ok(state.task_manager.list_tasks().await)
}

#[tauri::command]
async fn cancel_task(state: State<'_, AppState>, task_id: String) -> Result<TaskRecord, String> {
    state.task_manager.cancel_task(&task_id).await
}

#[tauri::command]
async fn clear_task_history(state: State<'_, AppState>) -> Result<Vec<TaskRecord>, String> {
    state.task_manager.clear_task_history().await
}

#[tauri::command]
async fn open_results_dir(app: AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    let results_dir = state.task_manager.results_dir();
    tokio::fs::create_dir_all(&results_dir)
        .await
        .map_err(|error| error.to_string())?;
    let path = results_dir.to_string_lossy().to_string();
    app.opener()
        .open_path(path.clone(), None::<String>)
        .map_err(|error| error.to_string())?;
    Ok(path)
}

#[tauri::command]
async fn export_diagnostics(state: State<'_, AppState>) -> Result<String, String> {
    state
        .task_manager
        .diagnostics()
        .export(state.app_data_dir.clone())
        .await
        .map(|path| path.to_string_lossy().to_string())
        .map_err(|error| error.to_string())
}

fn start_event_loop(app: AppHandle, mut rx: mpsc::Receiver<TaskEvent>) {
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                TaskEvent::Updated(record) => {
                    let _ = app.emit("task-updated", record);
                }
            }
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            let settings_path = app_data_dir.join("settings.json");
            let tasks_path = app_data_dir.join("tasks.json");
            let results_dir = app_data_dir.join("results");
            let diagnostics = DiagnosticsLogger::new(app_data_dir.join("diagnostics.log"));
            let settings = Arc::new(tauri::async_runtime::block_on(SettingsStore::new(
                settings_path,
            ))?);
            let client = RunningHubClient::new()?;
            let (event_tx, event_rx) = mpsc::channel(128);
            let task_manager = tauri::async_runtime::block_on(TaskManager::new(
                client.clone(),
                settings.clone(),
                event_tx,
                results_dir,
                tasks_path,
                diagnostics,
            ))?;

            app.manage(AppState {
                task_manager: task_manager.clone(),
                settings,
                client,
                app_data_dir,
            });
            start_event_loop(app.handle().clone(), event_rx);
            task_manager.start_recovery();
            Ok(())
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            submit_video_task,
            save_api_key,
            clear_api_key,
            has_api_key,
            get_app_settings,
            save_model_override,
            validate_api_key,
            inspect_model_nodes,
            list_tasks,
            cancel_task,
            clear_task_history,
            open_results_dir,
            export_diagnostics
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
