use futures_util::StreamExt;
use reqwest::multipart;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tokio::io::AsyncWriteExt;

const BASE_URL: &str = "https://www.runninghub.cn";

#[derive(Debug, Error)]
pub enum RunningHubError {
    #[error("网络请求失败：{0}")]
    Request(#[from] reqwest::Error),
    #[error("文件读写失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Api(String),
    #[error("{0}")]
    InvalidResponse(String),
}

pub type Result<T> = std::result::Result<T, RunningHubError>;

#[derive(Clone)]
pub struct RunningHubClient {
    http: reqwest::Client,
    base_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NodeInfo {
    pub node_id: String,
    #[serde(default)]
    pub node_name: Option<String>,
    pub field_name: String,
    #[serde(default)]
    pub field_value: String,
    #[serde(default)]
    pub field_data: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub description_en: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiAppRunData {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueryResultFile {
    pub url: String,
    #[serde(default)]
    pub output_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResponse {
    pub status: String,
    #[serde(default)]
    pub error_code: Option<String>,
    #[serde(default)]
    pub error_message: Option<String>,
    #[serde(default)]
    pub results: Vec<QueryResultFile>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeInspection {
    pub nodes: Vec<NodeInfo>,
    pub recommended: Option<ModelNodeRef>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelNodeRef {
    pub node_id: String,
    pub field_name: String,
}

#[derive(Debug, Deserialize)]
struct UploadEnvelope {
    code: i64,
    #[serde(default, alias = "msg")]
    message: String,
    data: Option<UploadData>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UploadData {
    #[serde(default, alias = "fileName")]
    file_name: String,
}

#[derive(Debug, Deserialize)]
struct ApiEnvelope<T> {
    code: i64,
    #[serde(default)]
    msg: String,
    data: Option<T>,
}

#[derive(Debug, Deserialize)]
struct DemoData {
    curl: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunRequest<'a> {
    webapp_id: u64,
    api_key: &'a str,
    node_info_list: &'a [NodeInfo],
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct QueryRequest<'a> {
    task_id: &'a str,
}

impl RunningHubClient {
    pub fn new() -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent("ClearClip/0.1")
            .build()?;
        Ok(Self {
            http,
            base_url: BASE_URL.into(),
        })
    }

    #[cfg(test)]
    fn with_base_url(base_url: String) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent("ClearClip/0.1")
            .build()?;
        Ok(Self { http, base_url })
    }

    pub async fn upload_video(&self, api_key: &str, file_path: &Path) -> Result<String> {
        let part = multipart::Part::file(file_path).await?;
        let form = multipart::Form::new().part("file", part);

        let response = self
            .http
            .post(format!("{}/openapi/v2/media/upload/binary", self.base_url))
            .bearer_auth(api_key)
            .multipart(form)
            .send()
            .await?;

        let status = response.status();
        let envelope = response.json::<UploadEnvelope>().await?;
        if !status.is_success() || envelope.code != 0 {
            return Err(RunningHubError::Api(map_runninghub_error(
                envelope.code,
                &envelope.message,
            )));
        }

        let file_name = envelope
            .data
            .map(|data| data.file_name)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                RunningHubError::InvalidResponse("上传成功但没有返回 fileName。".into())
            })?;
        Ok(file_name)
    }

    pub async fn fetch_demo_node_info(&self, api_key: &str, app_id: &str) -> Result<Vec<NodeInfo>> {
        let response = self
            .http
            .get(format!("{}/api/webapp/apiCallDemo", self.base_url))
            .bearer_auth(api_key)
            .query(&[("apiKey", api_key), ("webappId", app_id)])
            .send()
            .await?;

        let status = response.status();
        let envelope = response.json::<ApiEnvelope<DemoData>>().await?;
        if !status.is_success() || envelope.code != 0 {
            return Err(RunningHubError::Api(map_runninghub_error(
                envelope.code,
                &envelope.msg,
            )));
        }

        let curl = envelope.data.map(|data| data.curl).ok_or_else(|| {
            RunningHubError::InvalidResponse("没有获取到 AI 应用调用示例。".into())
        })?;
        extract_node_info_from_demo(&curl)
    }

    pub async fn validate_api_key(&self, api_key: &str, app_id: &str) -> Result<NodeInspection> {
        let nodes = self.fetch_demo_node_info(api_key, app_id).await?;
        Ok(inspect_node_info(nodes))
    }

    pub async fn inspect_model_nodes(&self, api_key: &str, app_id: &str) -> Result<NodeInspection> {
        let nodes = self.fetch_demo_node_info(api_key, app_id).await?;
        Ok(inspect_node_info(nodes))
    }

    pub async fn run_ai_app(
        &self,
        api_key: &str,
        app_id: &str,
        node_info: &[NodeInfo],
    ) -> Result<AiAppRunData> {
        let webapp_id = app_id
            .parse::<u64>()
            .map_err(|_| RunningHubError::InvalidResponse("工作流 ID 必须是数字。".into()))?;
        let body = RunRequest {
            webapp_id,
            api_key,
            node_info_list: node_info,
        };

        let response = self
            .http
            .post(format!("{}/task/openapi/ai-app/run", self.base_url))
            .bearer_auth(api_key)
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        let envelope = response.json::<ApiEnvelope<AiAppRunData>>().await?;
        if !status.is_success() || envelope.code != 0 {
            return Err(RunningHubError::Api(map_runninghub_error(
                envelope.code,
                &envelope.msg,
            )));
        }

        envelope
            .data
            .ok_or_else(|| RunningHubError::InvalidResponse("提交成功但没有返回 taskId。".into()))
    }

    pub async fn query_task(&self, api_key: &str, task_id: &str) -> Result<QueryResponse> {
        let response = self
            .http
            .post(format!("{}/openapi/v2/query", self.base_url))
            .bearer_auth(api_key)
            .json(&QueryRequest { task_id })
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(RunningHubError::Api(format!(
                "查询任务失败，HTTP 状态：{}",
                response.status()
            )));
        }

        let value = response.json::<Value>().await?;
        let query = parse_query_response(value)?;
        if query.status.eq_ignore_ascii_case("FAILED") {
            let message = query
                .error_message
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("RunningHub 返回任务失败。");
            return Err(RunningHubError::Api(map_runninghub_status_error(
                query.error_code.as_deref().unwrap_or_default(),
                message,
            )));
        }

        Ok(query)
    }

    pub async fn download_result(&self, url: &str, save_path: &Path) -> Result<()> {
        if let Some(parent) = save_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let response = self.http.get(url).send().await?.error_for_status()?;
        let mut stream = response.bytes_stream();
        let mut file = tokio::fs::File::create(save_path).await?;

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk).await?;
        }

        file.flush().await?;
        Ok(())
    }
}

pub fn parse_query_response(value: Value) -> Result<QueryResponse> {
    if value.get("status").is_some() {
        return serde_json::from_value::<QueryResponse>(value).map_err(|err| {
            RunningHubError::InvalidResponse(format!("任务查询响应解析失败：{err}"))
        });
    }

    if value.get("code").is_some() {
        let envelope =
            serde_json::from_value::<ApiEnvelope<QueryResponse>>(value).map_err(|err| {
                RunningHubError::InvalidResponse(format!("任务查询响应解析失败：{err}"))
            })?;
        if envelope.code != 0 {
            return Err(RunningHubError::Api(map_runninghub_error(
                envelope.code,
                &envelope.msg,
            )));
        }
        return envelope
            .data
            .ok_or_else(|| RunningHubError::InvalidResponse("任务查询响应缺少 data。".into()));
    }

    Err(RunningHubError::InvalidResponse(
        "任务查询响应缺少 status 或 code。".into(),
    ))
}

pub fn extract_node_info_from_demo(curl: &str) -> Result<Vec<NodeInfo>> {
    let value = extract_json_value_from_demo(curl)?;
    let node_info = value.get("nodeInfoList").cloned().ok_or_else(|| {
        RunningHubError::InvalidResponse("AI 应用调用示例缺少 nodeInfoList。".into())
    })?;

    serde_json::from_value::<Vec<NodeInfo>>(node_info)
        .map_err(|err| RunningHubError::InvalidResponse(format!("nodeInfoList 解析失败：{err}")))
}

fn extract_json_value_from_demo(curl: &str) -> Result<Value> {
    let first = curl.find('{').ok_or_else(|| {
        RunningHubError::InvalidResponse("AI 应用调用示例里没有 JSON 请求体。".into())
    })?;
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;

    for (offset, ch) in curl[first..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && in_string {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        if ch == '{' {
            depth += 1;
        } else if ch == '}' {
            depth -= 1;
            if depth == 0 {
                let end = first + offset + ch.len_utf8();
                let raw = &curl[first..end];
                return serde_json::from_str::<Value>(raw).map_err(|err| {
                    RunningHubError::InvalidResponse(format!(
                        "AI 应用调用示例 JSON 解析失败：{err}"
                    ))
                });
            }
        }
    }

    Err(RunningHubError::InvalidResponse(
        "AI 应用调用示例里的 JSON 请求体不完整。".into(),
    ))
}

pub fn choose_video_node(node_info: &[NodeInfo]) -> Option<usize> {
    choose_video_node_with_reason(node_info).0
}

pub fn inspect_node_info(nodes: Vec<NodeInfo>) -> NodeInspection {
    let (index, reason) = choose_video_node_with_reason(&nodes);
    let recommended = index.map(|index| ModelNodeRef {
        node_id: nodes[index].node_id.clone(),
        field_name: nodes[index].field_name.clone(),
    });

    NodeInspection {
        nodes,
        recommended,
        reason,
    }
}

fn choose_video_node_with_reason(node_info: &[NodeInfo]) -> (Option<usize>, String) {
    let strategies: [fn(&NodeInfo) -> bool; 5] = [
        |node| node.field_name.eq_ignore_ascii_case("video"),
        |node| node.field_name.eq_ignore_ascii_case("upload"),
        |node| {
            let field = node.field_name.to_lowercase();
            field.contains("video") || field.contains("file") || field.contains("media")
        },
        |node| {
            let text = format!(
                "{} {} {}",
                node.description.as_deref().unwrap_or_default(),
                node.node_name.as_deref().unwrap_or_default(),
                node.description_en.as_deref().unwrap_or_default()
            )
            .to_lowercase();
            text.contains("视频")
                || text.contains("上传")
                || text.contains("video")
                || text.contains("upload")
        },
        |node| {
            let value = node.field_value.to_lowercase();
            value.ends_with(".mp4")
                || value.ends_with(".mov")
                || value.ends_with(".avi")
                || value.ends_with(".mkv")
                || value.ends_with(".webm")
        },
    ];

    for strategy in strategies {
        let matches = node_info
            .iter()
            .enumerate()
            .filter_map(|(index, node)| strategy(node).then_some(index))
            .collect::<Vec<_>>();
        if matches.len() == 1 {
            return (
                matches.first().copied(),
                "已自动识别唯一的视频输入节点。".into(),
            );
        }
        if matches.len() > 1 {
            return (
                None,
                format!(
                    "检测到 {} 个可能的视频输入节点，请手动选择。",
                    matches.len()
                ),
            );
        }
    }

    (
        None,
        "没有识别到明显的视频输入节点，请手动选择 nodeId 和 fieldName。".into(),
    )
}

pub fn map_runninghub_error(code: i64, message: &str) -> String {
    let fallback = if message.trim().is_empty() {
        "RunningHub 请求失败。"
    } else {
        message.trim()
    };

    match code {
        0 => fallback.to_string(),
        301 => "参数错误，请检查工作流节点配置。".into(),
        380 => "工作流不存在，请检查 AI 工作流 ID。".into(),
        412 => "接口路径错误，请检查 RunningHub API 地址。".into(),
        415 => "独占型 API 机器数不足，请稍后重试。".into(),
        416 => "RunningHub 账户余额不足，请充值后重试。".into(),
        421 => "RunningHub 并发上限已满，请稍后重试。".into(),
        423 => "未找到指定任务，可能任务 ID 错误或已被清理。".into(),
        433 => "工作流校验未通过，请检查节点参数。".into(),
        801 => "免费用户不支持 API Key，请升级 RunningHub 账户。".into(),
        802 => "API Key 未授权或已失效，请重新保存。".into(),
        803 => "nodeInfoList 与工作流不匹配，请检查高级节点配置。".into(),
        804 => "任务正在运行中，请勿重复提交。".into(),
        805 => "任务状态异常，请稍后重试。".into(),
        500 => "RunningHub 服务端异常，请稍后重试。".into(),
        _ => format!("RunningHub 错误 {code}：{fallback}"),
    }
}

fn map_runninghub_status_error(code: &str, message: &str) -> String {
    if let Ok(code) = code.parse::<i64>() {
        return map_runninghub_error(code, message);
    }
    if message.trim().is_empty() {
        "RunningHub 返回任务失败。".into()
    } else {
        message.to_string()
    }
}

pub fn result_path_for(
    results_dir: &Path,
    source_path: &Path,
    model_name: &str,
    task_id: &str,
    result: &QueryResultFile,
    index: usize,
) -> PathBuf {
    let source_stem = source_path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("video");
    let extension = result_extension(result);
    let suffix = if index == 0 {
        String::new()
    } else {
        format!("-{}", index + 1)
    };
    let file_name = format!(
        "{}-{}-{}{}.{}",
        sanitize_file_part(source_stem),
        sanitize_file_part(model_name),
        sanitize_file_part(task_id),
        suffix,
        extension
    );
    results_dir.join(file_name)
}

fn result_extension(result: &QueryResultFile) -> String {
    if let Some(output_type) = result.output_type.as_deref() {
        let clean = output_type.trim().trim_start_matches('.');
        if !clean.is_empty() {
            return clean.to_lowercase();
        }
    }

    result
        .url
        .split('?')
        .next()
        .and_then(|path| path.rsplit('.').next())
        .filter(|ext| ext.len() <= 8 && ext.chars().all(|ch| ch.is_ascii_alphanumeric()))
        .unwrap_or("mp4")
        .to_lowercase()
}

fn sanitize_file_part(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '-',
            ch if ch.is_control() => '-',
            ch => ch,
        })
        .collect::<String>()
        .trim()
        .trim_matches('.')
        .to_string();

    if sanitized.is_empty() {
        "untitled".into()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn extracts_node_info_from_demo_curl() {
        let curl = r#"curl --data-raw '{
          "webappId": "1",
          "apiKey": "{{apikey}}",
          "nodeInfoList": [
            {"nodeId":"7","fieldName":"video","fieldValue":"old.mp4","description":"上传视频"}
          ]
        }'"#;

        let nodes = extract_node_info_from_demo(curl).expect("nodes");
        assert_eq!(nodes[0].node_id, "7");
        assert_eq!(nodes[0].field_name, "video");
    }

    #[test]
    fn chooses_single_video_node_by_field_name() {
        let nodes = vec![
            NodeInfo {
                node_id: "1".into(),
                node_name: None,
                field_name: "prompt".into(),
                field_value: String::new(),
                field_data: None,
                description: None,
                description_en: None,
            },
            NodeInfo {
                node_id: "2".into(),
                node_name: None,
                field_name: "video".into(),
                field_value: String::new(),
                field_data: None,
                description: None,
                description_en: None,
            },
        ];

        assert_eq!(choose_video_node(&nodes), Some(1));
    }

    #[test]
    fn refuses_ambiguous_exact_matches() {
        let nodes = vec![
            NodeInfo {
                node_id: "1".into(),
                node_name: None,
                field_name: "video".into(),
                field_value: String::new(),
                field_data: None,
                description: None,
                description_en: None,
            },
            NodeInfo {
                node_id: "2".into(),
                node_name: None,
                field_name: "video".into(),
                field_value: String::new(),
                field_data: None,
                description: None,
                description_en: None,
            },
        ];

        assert_eq!(choose_video_node(&nodes), None);
    }

    #[test]
    fn inspects_nodes_with_recommendation_and_reason() {
        let nodes = vec![
            NodeInfo {
                node_id: "1".into(),
                node_name: Some("Prompt".into()),
                field_name: "prompt".into(),
                field_value: String::new(),
                field_data: None,
                description: None,
                description_en: None,
            },
            NodeInfo {
                node_id: "9".into(),
                node_name: Some("Upload".into()),
                field_name: "mediaFile".into(),
                field_value: String::new(),
                field_data: None,
                description: Some("上传视频".into()),
                description_en: None,
            },
        ];

        let inspection = inspect_node_info(nodes);
        assert_eq!(
            inspection.recommended,
            Some(ModelNodeRef {
                node_id: "9".into(),
                field_name: "mediaFile".into(),
            })
        );
        assert!(inspection.reason.contains("自动识别"));
    }

    #[test]
    fn inspection_reports_ambiguous_video_nodes() {
        let nodes = vec![
            NodeInfo {
                node_id: "1".into(),
                node_name: None,
                field_name: "video".into(),
                field_value: String::new(),
                field_data: None,
                description: None,
                description_en: None,
            },
            NodeInfo {
                node_id: "2".into(),
                node_name: None,
                field_name: "video".into(),
                field_value: String::new(),
                field_data: None,
                description: None,
                description_en: None,
            },
        ];

        let inspection = inspect_node_info(nodes);
        assert!(inspection.recommended.is_none());
        assert!(inspection.reason.contains("手动选择"));
    }

    #[test]
    fn parses_wrapped_v2_query_response() {
        let response = parse_query_response(serde_json::json!({
            "code": 0,
            "msg": "success",
            "data": {
                "status": "RUNNING",
                "errorCode": "",
                "errorMessage": "",
                "results": []
            }
        }))
        .expect("query response");

        assert_eq!(response.status, "RUNNING");
        assert!(response.results.is_empty());
    }

    #[test]
    fn parses_direct_failed_query_response() {
        let response = parse_query_response(serde_json::json!({
            "status": "FAILED",
            "errorCode": "803",
            "errorMessage": "nodeInfoList mismatch",
            "results": []
        }))
        .expect("query response");

        assert_eq!(response.status, "FAILED");
        assert_eq!(response.error_code.as_deref(), Some("803"));
    }

    #[test]
    fn maps_common_error_codes() {
        assert!(map_runninghub_error(802, "").contains("API Key"));
        assert!(map_runninghub_error(416, "").contains("余额"));
        assert!(map_runninghub_error(803, "").contains("nodeInfoList"));
    }

    #[test]
    fn builds_result_path_with_extension() {
        let result = QueryResultFile {
            url: "https://example.com/output/file.mov?x=1".into(),
            output_type: None,
        };
        let path = result_path_for(
            Path::new("C:/out"),
            Path::new("C:/input/a:b.mp4"),
            "4K 超分增强",
            "task/1",
            &result,
            1,
        );
        let name = path.file_name().unwrap().to_string_lossy();
        assert!(name.ends_with("-2.mov"));
        assert!(!name.contains(':'));
        assert!(!name.contains('/'));
    }

    #[tokio::test]
    async fn runs_complete_client_flow_against_mock_server() {
        let (base_url, server_task) = spawn_mock_server().await;
        let input_path =
            std::env::temp_dir().join(format!("clearclip-input-{}.mp4", uuid::Uuid::new_v4()));
        let output_path =
            std::env::temp_dir().join(format!("clearclip-output-{}.mp4", uuid::Uuid::new_v4()));
        tokio::fs::write(&input_path, b"video")
            .await
            .expect("input");

        let client = RunningHubClient::with_base_url(base_url.clone()).expect("client");
        let uploaded = client
            .upload_video("key", &input_path)
            .await
            .expect("upload");
        assert_eq!(uploaded, "openapi/mock-video.mp4");

        let inspection = client
            .validate_api_key("key", "123")
            .await
            .expect("api key validation");
        assert_eq!(
            inspection.recommended,
            Some(ModelNodeRef {
                node_id: "7".into(),
                field_name: "video".into(),
            })
        );

        let mut nodes = client
            .fetch_demo_node_info("key", "123")
            .await
            .expect("demo nodes");
        let index = choose_video_node(&nodes).expect("video node");
        nodes[index].field_value = uploaded;

        let task = client
            .run_ai_app("key", "123", &nodes)
            .await
            .expect("run app");
        assert_eq!(task.task_id, "remote-task-1");

        let query = client
            .query_task("key", &task.task_id)
            .await
            .expect("query");
        assert_eq!(query.status, "SUCCESS");
        assert_eq!(query.results[0].url, format!("{base_url}/result.mp4"));

        client
            .download_result(&query.results[0].url, &output_path)
            .await
            .expect("download");
        assert_eq!(
            tokio::fs::read(&output_path).await.expect("output"),
            b"done"
        );

        let _ = tokio::fs::remove_file(input_path).await;
        let _ = tokio::fs::remove_file(output_path).await;
        server_task.abort();
    }

    async fn spawn_mock_server() -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let addr = listener.local_addr().expect("addr");
        let base_url = format!("http://{addr}");
        let server_base_url = base_url.clone();

        let task = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let base_url = server_base_url.clone();
                tokio::spawn(async move {
                    let Ok(path) = read_request_path(&mut socket).await else {
                        return;
                    };
                    let route = path.split('?').next().unwrap_or(path.as_str());
                    let (content_type, body) = match route {
                        "/openapi/v2/media/upload/binary" => json_body(serde_json::json!({
                            "code": 0,
                            "message": "success",
                            "data": {
                                "fileName": "openapi/mock-video.mp4"
                            }
                        })),
                        "/api/webapp/apiCallDemo" => json_body(serde_json::json!({
                            "code": 0,
                            "msg": "success",
                            "data": {
                                "curl": "curl --data-raw '{\"webappId\":\"123\",\"apiKey\":\"{{apikey}}\",\"nodeInfoList\":[{\"nodeId\":\"7\",\"fieldName\":\"video\",\"fieldValue\":\"old.mp4\",\"description\":\"上传视频\"}]}'"
                            }
                        })),
                        "/task/openapi/ai-app/run" => json_body(serde_json::json!({
                            "code": 0,
                            "msg": "success",
                            "data": {
                                "taskId": "remote-task-1",
                                "taskStatus": "RUNNING"
                            }
                        })),
                        "/openapi/v2/query" => json_body(serde_json::json!({
                            "taskId": "remote-task-1",
                            "status": "SUCCESS",
                            "errorCode": "",
                            "errorMessage": "",
                            "results": [
                                {
                                    "url": format!("{base_url}/result.mp4"),
                                    "outputType": "mp4"
                                }
                            ],
                            "clientId": "",
                            "promptTips": ""
                        })),
                        "/result.mp4" => ("application/octet-stream", b"done".to_vec()),
                        _ => ("text/plain", b"not found".to_vec()),
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.write_all(&body).await;
                    let _ = socket.shutdown().await;
                });
            }
        });

        (base_url, task)
    }

    fn json_body(value: serde_json::Value) -> (&'static str, Vec<u8>) {
        (
            "application/json",
            serde_json::to_vec(&value).expect("json"),
        )
    }

    async fn read_request_path(socket: &mut tokio::net::TcpStream) -> std::io::Result<String> {
        let mut buffer = Vec::new();
        let mut chunk = [0u8; 1024];
        let headers_end;
        loop {
            let read = socket.read(&mut chunk).await?;
            if read == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "connection closed",
                ));
            }
            buffer.extend_from_slice(&chunk[..read]);
            if let Some(index) = find_headers_end(&buffer) {
                headers_end = index;
                break;
            }
        }

        let headers = String::from_utf8_lossy(&buffer[..headers_end]).to_string();
        let content_length = headers
            .lines()
            .find_map(|line| line.split_once(':'))
            .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
            .and_then(|(_, value)| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        let is_chunked = headers.lines().any(|line| {
            line.split_once(':')
                .map(|(name, value)| {
                    name.eq_ignore_ascii_case("transfer-encoding")
                        && value.to_ascii_lowercase().contains("chunked")
                })
                .unwrap_or(false)
        });
        let body_start = headers_end + 4;
        let already_read = buffer.len().saturating_sub(body_start);
        if is_chunked {
            while !buffer[body_start..]
                .windows(5)
                .any(|window| window == b"0\r\n\r\n")
            {
                let read = socket.read(&mut chunk).await?;
                if read == 0 {
                    break;
                }
                buffer.extend_from_slice(&chunk[..read]);
            }
        } else {
            let mut remaining = content_length.saturating_sub(already_read);
            while remaining > 0 {
                let read = socket.read(&mut chunk).await?;
                if read == 0 {
                    break;
                }
                remaining = remaining.saturating_sub(read);
            }
        }
        loop {
            match tokio::time::timeout(Duration::from_millis(50), socket.read(&mut chunk)).await {
                Ok(Ok(0)) | Err(_) => break,
                Ok(Ok(_)) => {}
                Ok(Err(error)) => return Err(error),
            }
        }

        let request_line = headers.lines().next().unwrap_or_default();
        Ok(request_line
            .split_whitespace()
            .nth(1)
            .unwrap_or("/")
            .to_string())
    }

    fn find_headers_end(buffer: &[u8]) -> Option<usize> {
        buffer.windows(4).position(|window| window == b"\r\n\r\n")
    }
}
