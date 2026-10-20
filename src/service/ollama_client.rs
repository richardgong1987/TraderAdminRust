//! HTTP client for Ollama's chat API (https://github.com/ollama/ollama/blob/main/docs/api.md).
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// On a CPU, one answer with several example images can take minutes.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20 * 60);

#[derive(Serialize, Debug)]
pub struct ChatMessage<'a> {
    pub role: &'static str,
    pub content: String,
    /// Raw base64 images.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<&'a str>,
}

#[derive(Serialize, Debug)]
pub struct ChatRequest<'a> {
    pub model: &'a str,
    pub messages: Vec<ChatMessage<'a>>,
    /// JSON schema the answer must follow.
    pub format: serde_json::Value,
    pub stream: bool,
    pub options: ChatOptions,
}

#[derive(Serialize, Debug)]
pub struct ChatOptions {
    pub temperature: f32,
    pub num_ctx: u32,
}

#[derive(Deserialize, Debug)]
pub struct ChatResponse {
    pub message: ResponseMessage,
    pub prompt_eval_count: Option<i32>,
}

#[derive(Deserialize, Debug)]
pub struct ResponseMessage {
    pub content: String,
}

#[derive(Deserialize)]
struct ErrorBody {
    error: String,
}

pub struct OllamaClient {
    http: reqwest::Client,
    base_url: String,
}

impl OllamaClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .build()
                .unwrap_or_default(),
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    pub async fn chat(&self, request: &ChatRequest<'_>) -> Result<ChatResponse> {
        let response = self
            .http
            .post(format!("{}/api/chat", self.base_url))
            .json(request)
            .send()
            .await
            .map_err(|e| Error::from(format!("Ollama unreachable at {}: {}", self.base_url, e)))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            let message = serde_json::from_str::<ErrorBody>(&body)
                .map(|b| b.error)
                .unwrap_or(body);
            return Err(Error::from(format!("Ollama error {}: {}", status, message)));
        }
        response
            .json::<ChatResponse>()
            .await
            .map_err(|e| Error::from(format!("Ollama returned an unreadable answer: {}", e)))
    }
}
