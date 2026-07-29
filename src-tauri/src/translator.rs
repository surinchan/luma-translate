use crate::settings::load_settings;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TranslationError {
    #[error("{0}")]
    Settings(#[from] crate::settings::SettingsError),
    #[error("请先在设置中填写 API Key")]
    MissingApiKey,
    #[error("网络请求失败：{0}")]
    Request(#[from] reqwest::Error),
    #[error("API 请求失败（HTTP {status}）：{message}")]
    Api { status: u16, message: String },
    #[error("LLM 返回了空的翻译结果")]
    EmptyResponse,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    temperature: f32,
    messages: Vec<Message<'a>>,
}

#[derive(Serialize)]
struct Message<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Deserialize)]
struct ResponseMessage {
    content: String,
}

#[derive(Deserialize)]
struct ErrorEnvelope {
    error: Option<ApiError>,
}

#[derive(Deserialize)]
struct ApiError {
    message: String,
}

fn build_prompt(source_language: &str, target_language: &str) -> String {
    let source = if source_language == "自动检测" {
        "自动识别源语言。".to_owned()
    } else {
        format!("源语言是{source_language}。")
    };
    format!(
        "你是一名专业、克制的翻译助手。\n将用户提供的内容翻译为{target_language}。\n{source}\n\
         保留段落、列表、数字、专有名词和语气。只输出译文，不要解释，不要添加任何前缀。"
    )
}

pub async fn translate(app: &AppHandle, text: &str) -> Result<String, TranslationError> {
    translate_to(app, text, None).await
}

pub async fn translate_to(
    app: &AppHandle,
    text: &str,
    target_language: Option<&str>,
) -> Result<String, TranslationError> {
    let settings = load_settings(app)?;
    if settings.api_key.is_empty() {
        return Err(TranslationError::MissingApiKey);
    }
    let target_language = target_language
        .map(str::trim)
        .filter(|language| !language.is_empty())
        .unwrap_or(&settings.target_language);

    let prompt = build_prompt(&settings.source_language, target_language);
    let body = ChatRequest {
        model: &settings.model,
        temperature: 0.2,
        messages: vec![
            Message {
                role: "system",
                content: &prompt,
            },
            Message {
                role: "user",
                content: text,
            },
        ],
    };

    let response = Client::builder()
        .timeout(std::time::Duration::from_secs(45))
        .build()?
        .post(format!("{}/chat/completions", settings.endpoint))
        .bearer_auth(&settings.api_key)
        .json(&body)
        .send()
        .await?;
    let status = response.status();
    if !status.is_success() {
        let message = response
            .json::<ErrorEnvelope>()
            .await
            .ok()
            .and_then(|body| body.error)
            .map(|error| error.message)
            .unwrap_or_else(|| "服务未返回错误详情".into());
        return Err(TranslationError::Api {
            status: status.as_u16(),
            message,
        });
    }
    let payload = response.json::<ChatResponse>().await?;
    payload
        .choices
        .into_iter()
        .next()
        .map(|choice| choice.message.content.trim().to_owned())
        .filter(|content| !content.is_empty())
        .ok_or(TranslationError::EmptyResponse)
}

#[cfg(test)]
mod tests {
    use super::build_prompt;

    #[test]
    fn prompt_uses_manual_target_language() {
        let prompt = build_prompt("自动检测", "日语");
        assert!(prompt.contains("翻译为日语"));
        assert!(prompt.contains("自动识别源语言"));
    }

    #[test]
    fn prompt_keeps_configured_source_language() {
        let prompt = build_prompt("英语", "法语");
        assert!(prompt.contains("源语言是英语"));
        assert!(prompt.contains("翻译为法语"));
    }
}
