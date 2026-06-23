use crate::diagnostics::{current_timestamp, DiagnosticsLogger};
use crate::runninghub::{
    choose_video_node, result_path_for, QueryResultFile, RunningHubClient, RunningHubError,
};
use crate::settings::{self, SettingsStore};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock, Semaphore};
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

const MAX_CONCURRENT_TASKS: usize = 2;
const POLL_INTERVAL: Duration = Duration::from_secs(5);
const MAX_POLL_ATTEMPTS: usize = 1440;

#[derive(Debug, Clone)]
pub enum TaskEvent {
    Updated(TaskRecord),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskStatus {
    Uploading,
    Configuring,
    Pending,
    Running,
    Downloading,
    Success,
    Failed,
    Canceled,
}

impl TaskStatus {
    pub fn is_active(self) -> bool {
        matches!(
            self,
            Self::Uploading | Self::Configuring | Self::Pending | Self::Running | Self::Downloading
        )
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Success | Self::Failed | Self::Canceled)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRecord {
    pub task_id: String,
    #[serde(default)]
    pub remote_task_id: Option<String>,
    pub app_id: String,
    pub app_name: String,
    pub file_name: String,
    pub file_path: String,
    #[serde(default)]
    pub params: Value,
    pub status: TaskStatus,
    pub progress: f64,
    #[serde(default)]
    pub save_path: Option<String>,
    #[serde(default)]
    pub save_paths: Vec<String>,
    #[serde(default)]
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone)]
pub struct TaskManager {
    client: RunningHubClient,
    settings: Arc<SettingsStore>,
    event_tx: mpsc::Sender<TaskEvent>,
    results_dir: PathBuf,
    tasks_path: PathBuf,
    diagnostics: DiagnosticsLogger,
    semaphore: Arc<Semaphore>,
    records: Arc<RwLock<HashMap<String, TaskRecord>>>,
    active_tasks: Arc<RwLock<HashMap<String, String>>>,
    cancel_tokens: Arc<RwLock<HashMap<String, CancellationToken>>>,
}

impl TaskManager {
    pub async fn new(
        client: RunningHubClient,
        settings: Arc<SettingsStore>,
        event_tx: mpsc::Sender<TaskEvent>,
        results_dir: PathBuf,
        tasks_path: PathBuf,
        diagnostics: DiagnosticsLogger,
    ) -> Result<Self, String> {
        let records = load_records(&tasks_path).await?;
        Ok(Self {
            client,
            settings,
            event_tx,
            results_dir,
            tasks_path,
            diagnostics,
            semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_TASKS)),
            records: Arc::new(RwLock::new(records)),
            active_tasks: Arc::new(RwLock::new(HashMap::new())),
            cancel_tokens: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    pub async fn submit_and_poll(
        &self,
        app_id: String,
        app_name: String,
        file_path: PathBuf,
        params: Value,
    ) -> Result<String, String> {
        validate_video_file(&file_path)?;
        if !settings::has_api_key().map_err(|error| error.to_string())? {
            return Err("请先在设置中保存 RunningHub API Key。".into());
        }

        let cache_key = format!("{}:{}:{}", app_id, file_path.to_string_lossy(), params);
        {
            let active = self.active_tasks.read().await;
            if let Some(task_id) = active.get(&cache_key) {
                return Ok(task_id.clone());
            }
        }

        let task_id = format!("task-{}", uuid::Uuid::new_v4());
        let now = current_timestamp();
        let record = TaskRecord {
            task_id: task_id.clone(),
            remote_task_id: None,
            app_id,
            app_name,
            file_name: file_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("video")
                .to_string(),
            file_path: file_path.to_string_lossy().to_string(),
            params,
            status: TaskStatus::Pending,
            progress: 0.0,
            save_path: None,
            save_paths: Vec::new(),
            error: None,
            created_at: now.clone(),
            updated_at: now,
        };

        self.insert_record(record.clone()).await?;
        {
            let mut active = self.active_tasks.write().await;
            active.insert(cache_key.clone(), task_id.clone());
        }

        let token = CancellationToken::new();
        self.cancel_tokens
            .write()
            .await
            .insert(task_id.clone(), token.clone());

        TaskWorker {
            manager: self.clone(),
            task_id: task_id.clone(),
            cache_key: Some(cache_key),
            token,
            resume_remote_task_id: None,
        }
        .spawn();

        Ok(task_id)
    }

    pub async fn list_tasks(&self) -> Vec<TaskRecord> {
        sorted_records(self.records.read().await.values().cloned().collect())
    }

    pub async fn cancel_task(&self, task_id: &str) -> Result<TaskRecord, String> {
        if let Some(token) = self.cancel_tokens.read().await.get(task_id).cloned() {
            token.cancel();
        }

        self.update_record(task_id, |record| {
            if !record.status.is_terminal() {
                record.status = TaskStatus::Canceled;
                record.progress = 100.0;
                record.error =
                    Some("已取消本机跟踪。已提交到 RunningHub 的任务不会被远程取消。".into());
            }
        })
        .await?
        .ok_or_else(|| "没有找到任务。".to_string())
    }

    pub async fn clear_task_history(&self) -> Result<Vec<TaskRecord>, String> {
        let snapshot = {
            let mut records = self.records.write().await;
            records.retain(|_, record| !record.status.is_terminal());
            sorted_records(records.values().cloned().collect())
        };
        self.persist().await?;
        self.diagnostics
            .log(
                "task_history_cleared",
                None,
                "Cleared terminal task history",
                serde_json::json!({ "remaining": snapshot.len() }),
            )
            .await;
        Ok(snapshot)
    }

    pub fn results_dir(&self) -> PathBuf {
        self.results_dir.clone()
    }

    pub fn diagnostics(&self) -> DiagnosticsLogger {
        self.diagnostics.clone()
    }

    pub fn start_recovery(&self) {
        let manager = self.clone();
        tokio::spawn(async move {
            manager.recover_unfinished().await;
        });
    }

    async fn recover_unfinished(&self) {
        let records = self.list_tasks().await;
        for record in records {
            if !record.status.is_active() {
                continue;
            }

            if let Some(remote_task_id) = record.remote_task_id.clone() {
                let token = CancellationToken::new();
                self.cancel_tokens
                    .write()
                    .await
                    .insert(record.task_id.clone(), token.clone());
                TaskWorker {
                    manager: self.clone(),
                    task_id: record.task_id.clone(),
                    cache_key: None,
                    token,
                    resume_remote_task_id: Some(remote_task_id),
                }
                .spawn();
            } else {
                let _ = self
                    .update_record(&record.task_id, |record| {
                        record.status = TaskStatus::Failed;
                        record.progress = 100.0;
                        record.error = Some(
                            "应用重启前任务尚未提交到 RunningHub，已停止以避免重复扣费。".into(),
                        );
                    })
                    .await;
            }
        }
    }

    async fn insert_record(&self, record: TaskRecord) -> Result<(), String> {
        {
            let mut records = self.records.write().await;
            records.insert(record.task_id.clone(), record.clone());
        }
        self.persist().await?;
        self.emit(record).await;
        Ok(())
    }

    async fn update_record<F>(&self, task_id: &str, mutate: F) -> Result<Option<TaskRecord>, String>
    where
        F: FnOnce(&mut TaskRecord),
    {
        let updated = {
            let mut records = self.records.write().await;
            let Some(record) = records.get_mut(task_id) else {
                return Ok(None);
            };
            mutate(record);
            record.updated_at = current_timestamp();
            record.clone()
        };
        self.persist().await?;
        self.emit(updated.clone()).await;
        Ok(Some(updated))
    }

    async fn set_remote_task_id(
        &self,
        task_id: &str,
        remote_task_id: String,
    ) -> Result<(), String> {
        self.update_record(task_id, |record| {
            record.remote_task_id = Some(remote_task_id);
        })
        .await?;
        Ok(())
    }

    async fn set_progress(
        &self,
        task_id: &str,
        status: TaskStatus,
        progress: f64,
    ) -> Result<(), String> {
        self.update_record(task_id, |record| {
            record.status = status;
            record.progress = clamp_progress(progress);
            if status != TaskStatus::Failed {
                record.error = None;
            }
        })
        .await?;
        Ok(())
    }

    async fn fail_task(&self, task_id: &str, error: String) {
        let _ = self
            .update_record(task_id, |record| {
                record.status = TaskStatus::Failed;
                record.progress = 100.0;
                record.error = Some(error);
            })
            .await;
    }

    async fn complete_task(&self, task_id: &str, save_paths: Vec<PathBuf>) -> Result<(), String> {
        let save_paths_string = save_paths
            .iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        let first = save_paths_string.first().cloned();
        self.update_record(task_id, |record| {
            record.status = TaskStatus::Success;
            record.progress = 100.0;
            record.save_path = first.clone();
            record.save_paths = save_paths_string;
            record.error = None;
        })
        .await?;
        Ok(())
    }

    async fn record_snapshot(&self, task_id: &str) -> Option<TaskRecord> {
        self.records.read().await.get(task_id).cloned()
    }

    async fn emit(&self, record: TaskRecord) {
        self.diagnostics
            .log(
                "task_status",
                Some(&record.task_id),
                "Task state changed",
                serde_json::json!({
                    "status": record.status,
                    "progress": record.progress,
                    "remoteTaskId": record.remote_task_id,
                    "error": record.error,
                }),
            )
            .await;
        let _ = self.event_tx.send(TaskEvent::Updated(record)).await;
    }

    async fn persist(&self) -> Result<(), String> {
        let records = self.list_tasks().await;
        if let Some(parent) = self.tasks_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|error| error.to_string())?;
        }
        let content = serde_json::to_string_pretty(&records).map_err(|error| error.to_string())?;
        tokio::fs::write(&self.tasks_path, content)
            .await
            .map_err(|error| error.to_string())
    }
}

struct TaskWorker {
    manager: TaskManager,
    task_id: String,
    cache_key: Option<String>,
    token: CancellationToken,
    resume_remote_task_id: Option<String>,
}

impl TaskWorker {
    fn spawn(self) {
        tokio::spawn(async move {
            self.run().await;
        });
    }

    async fn run(self) {
        let result = if let Some(remote_task_id) = self.resume_remote_task_id.clone() {
            self.run_resume(remote_task_id).await
        } else {
            self.run_new().await
        };

        if let Err(error) = result {
            if error == "CANCELED" {
                let _ = self.manager.cancel_task(&self.task_id).await;
            } else {
                self.manager.fail_task(&self.task_id, error).await;
            }
        }

        if let Some(cache_key) = &self.cache_key {
            self.manager.active_tasks.write().await.remove(cache_key);
        }
        self.manager
            .cancel_tokens
            .write()
            .await
            .remove(&self.task_id);
    }

    async fn run_new(&self) -> Result<(), String> {
        self.manager
            .set_progress(&self.task_id, TaskStatus::Pending, 0.0)
            .await?;
        let permit = self.acquire_permit().await?;
        let _permit = permit;
        let api_key = settings::get_api_key().map_err(|error| error.to_string())?;
        let record = self
            .manager
            .record_snapshot(&self.task_id)
            .await
            .ok_or_else(|| "任务记录不存在。".to_string())?;

        self.manager
            .set_progress(&self.task_id, TaskStatus::Uploading, 10.0)
            .await?;
        let uploaded_file_name = self
            .cancelable(
                self.manager
                    .client
                    .upload_video(&api_key, Path::new(&record.file_path)),
            )
            .await
            .map_err(format_runninghub_error)?;

        self.manager
            .set_progress(&self.task_id, TaskStatus::Configuring, 24.0)
            .await?;
        let node_info = self
            .build_node_info(&api_key, &record.app_id, &uploaded_file_name)
            .await?;

        self.manager
            .set_progress(&self.task_id, TaskStatus::Pending, 34.0)
            .await?;
        let remote_task = self
            .cancelable(
                self.manager
                    .client
                    .run_ai_app(&api_key, &record.app_id, &node_info),
            )
            .await
            .map_err(format_runninghub_error)?;
        self.manager
            .set_remote_task_id(&self.task_id, remote_task.task_id.clone())
            .await?;

        self.poll_and_download(&api_key, &remote_task.task_id).await
    }

    async fn run_resume(&self, remote_task_id: String) -> Result<(), String> {
        let permit = self.acquire_permit().await?;
        let _permit = permit;
        let api_key = settings::get_api_key().map_err(|error| error.to_string())?;
        self.manager
            .set_progress(&self.task_id, TaskStatus::Running, 40.0)
            .await?;
        self.poll_and_download(&api_key, &remote_task_id).await
    }

    async fn build_node_info(
        &self,
        api_key: &str,
        app_id: &str,
        uploaded_file_name: &str,
    ) -> Result<Vec<crate::runninghub::NodeInfo>, String> {
        let mut nodes = self
            .cancelable(self.manager.client.fetch_demo_node_info(api_key, app_id))
            .await
            .map_err(format_runninghub_error)?;
        let override_config = self.manager.settings.model_override(app_id).await;

        let target_index = if let Some(config) = override_config {
            nodes
                .iter()
                .position(|node| {
                    node.node_id == config.node_id
                        && node.field_name.eq_ignore_ascii_case(&config.field_name)
                })
                .ok_or_else(|| {
                    format!(
                        "高级节点配置未在 RunningHub 示例中找到：nodeId={}，fieldName={}。",
                        config.node_id, config.field_name
                    )
                })?
        } else {
            choose_video_node(&nodes).ok_or_else(|| {
                "无法自动识别视频输入节点，请在设置中检测该工作流节点。".to_string()
            })?
        };

        self.manager
            .diagnostics
            .log(
                "node_selected",
                Some(&self.task_id),
                "Selected video input node",
                serde_json::json!({
                    "appId": app_id,
                    "nodeId": nodes[target_index].node_id,
                    "fieldName": nodes[target_index].field_name,
                }),
            )
            .await;

        nodes[target_index].field_value = uploaded_file_name.to_string();
        Ok(nodes)
    }

    async fn poll_and_download(&self, api_key: &str, remote_task_id: &str) -> Result<(), String> {
        self.manager
            .set_progress(&self.task_id, TaskStatus::Running, 40.0)
            .await?;
        let results = self.poll_until_success(api_key, remote_task_id).await?;
        if results.is_empty() {
            return Err("RunningHub 返回成功，但没有结果文件。".into());
        }

        self.manager
            .set_progress(&self.task_id, TaskStatus::Downloading, 96.0)
            .await?;
        let save_paths = self.download_results(&results).await?;
        self.manager.complete_task(&self.task_id, save_paths).await
    }

    async fn poll_until_success(
        &self,
        api_key: &str,
        remote_task_id: &str,
    ) -> Result<Vec<QueryResultFile>, String> {
        for attempt in 0..MAX_POLL_ATTEMPTS {
            let query = self
                .cancelable(self.manager.client.query_task(api_key, remote_task_id))
                .await
                .map_err(format_runninghub_error)?;

            if query.status.eq_ignore_ascii_case("SUCCESS") {
                self.manager
                    .set_progress(&self.task_id, TaskStatus::Running, 95.0)
                    .await?;
                return Ok(query.results);
            }

            let progress = (42.0 + attempt as f64 * 0.4).min(94.0);
            self.manager
                .set_progress(&self.task_id, TaskStatus::Running, progress)
                .await?;
            tokio::select! {
                _ = self.token.cancelled() => return Err("CANCELED".into()),
                _ = sleep(POLL_INTERVAL) => {}
            }
        }

        Err("任务轮询超时，请稍后在 RunningHub 后台确认结果。".into())
    }

    async fn download_results(&self, results: &[QueryResultFile]) -> Result<Vec<PathBuf>, String> {
        let record = self
            .manager
            .record_snapshot(&self.task_id)
            .await
            .ok_or_else(|| "任务记录不存在。".to_string())?;
        let mut save_paths = Vec::with_capacity(results.len());
        for (index, result) in results.iter().enumerate() {
            let save_path = result_path_for(
                &self.manager.results_dir,
                Path::new(&record.file_path),
                &record.app_name,
                &self.task_id,
                result,
                index,
            );
            self.cancelable(self.manager.client.download_result(&result.url, &save_path))
                .await
                .map_err(format_runninghub_error)?;
            save_paths.push(save_path);
        }
        Ok(save_paths)
    }

    async fn acquire_permit(&self) -> Result<tokio::sync::OwnedSemaphorePermit, String> {
        tokio::select! {
            _ = self.token.cancelled() => Err("CANCELED".into()),
            permit = self.manager.semaphore.clone().acquire_owned() => {
                permit.map_err(|error| error.to_string())
            }
        }
    }

    async fn cancelable<T, Fut>(&self, future: Fut) -> Result<T, RunningHubError>
    where
        Fut: Future<Output = Result<T, RunningHubError>>,
    {
        tokio::select! {
            _ = self.token.cancelled() => Err(RunningHubError::Api("CANCELED".into())),
            result = future => result,
        }
    }
}

pub fn validate_video_file(file_path: &Path) -> Result<(), String> {
    if !file_path.exists() {
        return Err("视频文件不存在，请重新选择。".into());
    }
    if !file_path.is_file() {
        return Err("请选择一个视频文件，而不是文件夹。".into());
    }
    validate_video_extension(file_path)
}

pub fn validate_video_extension(file_path: &Path) -> Result<(), String> {
    let extension = file_path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_lowercase();
    let supported = matches!(
        extension.as_str(),
        "mp4" | "mov" | "avi" | "mkv" | "webm" | "m4v"
    );
    if !supported {
        return Err("仅支持 mp4、mov、avi、mkv、webm、m4v 视频文件。".into());
    }
    Ok(())
}

fn format_runninghub_error(error: RunningHubError) -> String {
    let message = error.to_string();
    if message == "CANCELED" {
        "CANCELED".into()
    } else {
        message
    }
}

fn clamp_progress(progress: f64) -> f64 {
    if !progress.is_finite() {
        return 0.0;
    }
    progress.clamp(0.0, 100.0)
}

async fn load_records(path: &Path) -> Result<HashMap<String, TaskRecord>, String> {
    let content = match tokio::fs::read_to_string(path).await {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(error) => return Err(error.to_string()),
    };
    let records =
        serde_json::from_str::<Vec<TaskRecord>>(&content).map_err(|error| error.to_string())?;
    Ok(records
        .into_iter()
        .map(|record| (record.task_id.clone(), record))
        .collect())
}

fn sorted_records(mut records: Vec<TaskRecord>) -> Vec<TaskRecord> {
    records.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    records
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::SettingsStore;
    use std::sync::Arc;
    use tokio::sync::mpsc;

    #[test]
    fn rejects_unsupported_extension() {
        let path = Path::new("C:/clips/input.txt");
        let error = validate_video_extension(path).unwrap_err();
        assert!(error.contains("仅支持"));
    }

    #[test]
    fn accepts_supported_extension_case_insensitive() {
        validate_video_extension(Path::new("C:/clips/input.MP4")).expect("extension");
    }

    #[tokio::test]
    async fn persists_and_sorts_records() {
        let path =
            std::env::temp_dir().join(format!("clearclip-tasks-{}.json", uuid::Uuid::new_v4()));
        let records = vec![sample_record("task-a", "1"), sample_record("task-b", "2")];
        tokio::fs::write(&path, serde_json::to_string(&records).unwrap())
            .await
            .unwrap();

        let loaded = load_records(&path).await.unwrap();
        assert_eq!(loaded.len(), 2);
        let sorted = sorted_records(loaded.values().cloned().collect());
        assert_eq!(sorted[0].task_id, "task-b");

        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn cancel_marks_record_and_persists() {
        let root = std::env::temp_dir().join(format!("clearclip-state-{}", uuid::Uuid::new_v4()));
        let tasks_path = root.join("tasks.json");
        let settings = Arc::new(
            SettingsStore::new(root.join("settings.json"))
                .await
                .expect("settings"),
        );
        let (event_tx, _event_rx) = mpsc::channel(8);
        let manager = TaskManager::new(
            RunningHubClient::new().expect("client"),
            settings,
            event_tx,
            root.join("results"),
            tasks_path.clone(),
            DiagnosticsLogger::new(root.join("diagnostics.log")),
        )
        .await
        .expect("manager");

        manager
            .insert_record(sample_record("task-cancel", "1"))
            .await
            .expect("insert");

        let canceled = manager.cancel_task("task-cancel").await.expect("cancel");
        assert_eq!(canceled.status, TaskStatus::Canceled);
        assert!(canceled.error.unwrap_or_default().contains("本机跟踪"));

        let loaded = load_records(&tasks_path).await.expect("persisted records");
        assert_eq!(
            loaded.get("task-cancel").map(|record| record.status),
            Some(TaskStatus::Canceled)
        );

        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn clear_history_keeps_active_records() {
        let root = std::env::temp_dir().join(format!("clearclip-state-{}", uuid::Uuid::new_v4()));
        let tasks_path = root.join("tasks.json");
        let settings = Arc::new(
            SettingsStore::new(root.join("settings.json"))
                .await
                .expect("settings"),
        );
        let (event_tx, _event_rx) = mpsc::channel(8);
        let manager = TaskManager::new(
            RunningHubClient::new().expect("client"),
            settings,
            event_tx,
            root.join("results"),
            tasks_path.clone(),
            DiagnosticsLogger::new(root.join("diagnostics.log")),
        )
        .await
        .expect("manager");
        let mut failed = sample_record("task-failed", "1");
        failed.status = TaskStatus::Failed;
        let active = sample_record("task-active", "2");

        manager.insert_record(failed).await.expect("failed insert");
        manager.insert_record(active).await.expect("active insert");

        let remaining = manager.clear_task_history().await.expect("clear history");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].task_id, "task-active");

        let loaded = load_records(&tasks_path).await.expect("persisted records");
        assert!(loaded.contains_key("task-active"));
        assert!(!loaded.contains_key("task-failed"));

        let _ = tokio::fs::remove_dir_all(root).await;
    }

    fn sample_record(task_id: &str, created_at: &str) -> TaskRecord {
        TaskRecord {
            task_id: task_id.into(),
            remote_task_id: None,
            app_id: "app".into(),
            app_name: "model".into(),
            file_name: "a.mp4".into(),
            file_path: "C:/a.mp4".into(),
            params: Value::Null,
            status: TaskStatus::Pending,
            progress: 0.0,
            save_path: None,
            save_paths: Vec::new(),
            error: None,
            created_at: created_at.into(),
            updated_at: created_at.into(),
        }
    }
}
