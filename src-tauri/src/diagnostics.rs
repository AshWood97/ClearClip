use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct DiagnosticsLogger {
    path: PathBuf,
    lock: Arc<Mutex<()>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticEntry<'a> {
    timestamp: String,
    event: &'a str,
    task_id: Option<&'a str>,
    message: &'a str,
    details: Value,
}

impl DiagnosticsLogger {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            lock: Arc::new(Mutex::new(())),
        }
    }

    pub async fn log(&self, event: &str, task_id: Option<&str>, message: &str, details: Value) {
        let _guard = self.lock.lock().await;
        if let Some(parent) = self.path.parent() {
            if tokio::fs::create_dir_all(parent).await.is_err() {
                return;
            }
        }

        let entry = DiagnosticEntry {
            timestamp: current_timestamp(),
            event,
            task_id,
            message,
            details,
        };
        let Ok(mut line) = serde_json::to_string(&entry) else {
            return;
        };
        line.push('\n');

        let Ok(mut file) = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await
        else {
            return;
        };

        use tokio::io::AsyncWriteExt;
        let _ = file.write_all(line.as_bytes()).await;
        let _ = file.flush().await;
    }

    pub async fn export(&self, app_data_dir: PathBuf) -> std::io::Result<PathBuf> {
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        if tokio::fs::metadata(&self.path).await.is_err() {
            tokio::fs::write(&self.path, "").await?;
        }

        let export_path =
            app_data_dir.join(format!("diagnostics-export-{}.jsonl", compact_timestamp()));
        tokio::fs::copy(&self.path, &export_path).await?;
        Ok(export_path)
    }
}

pub fn current_timestamp() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    seconds.to_string()
}

fn compact_timestamp() -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    millis.to_string()
}
