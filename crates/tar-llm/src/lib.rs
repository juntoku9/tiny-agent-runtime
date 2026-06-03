#![no_std]
//! LLM providers. The Anthropic Messages API is the default; an
//! OpenAI-compatible provider is the fallback. Built on the `HttpClient` HAL,
//! so the same provider code runs on every platform.

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use serde_json::{json, Value};

use tar_core::{CoreError, LlmProvider, Message, Role};
use tar_hal::HttpClient;
use tar_proto::ToolManifest;

const ANTHROPIC_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const MAX_TOKENS: u32 = 1024;

/// Anthropic Messages API provider.
pub struct AnthropicProvider<H: HttpClient> {
    http: H,
    api_key: String,
    model: String,
}

impl<H: HttpClient> AnthropicProvider<H> {
    pub fn new(http: H, api_key: String, model: String) -> Self {
        Self { http, api_key, model }
    }
}

impl<H: HttpClient> LlmProvider for AnthropicProvider<H> {
    async fn complete(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[ToolManifest],
    ) -> Result<Message, CoreError> {
        // Anthropic puts `system` at the top level, not in the messages array.
        let mut msgs: Vec<Value> = Vec::new();
        for m in messages {
            let role = match m.role {
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::System => continue,
            };
            msgs.push(json!({ "role": role, "content": m.content }));
        }

        let mut body = json!({
            "model": self.model,
            "max_tokens": MAX_TOKENS,
            "system": system,
            "messages": msgs,
        });
        if !tools.is_empty() {
            body["tools"] = serde_json::to_value(tools).unwrap_or(Value::Null);
        }
        let body_bytes =
            serde_json::to_vec(&body).map_err(|e| CoreError::Provider(e.to_string()))?;

        let headers = [
            ("x-api-key", self.api_key.as_str()),
            ("anthropic-version", ANTHROPIC_VERSION),
            ("content-type", "application/json"),
        ];

        let resp = self
            .http
            .post(ANTHROPIC_URL, &headers, &body_bytes)
            .await
            .map_err(|e| CoreError::Provider(format!("{:?}", e)))?;

        if resp.status != 200 {
            let snippet = core::str::from_utf8(&resp.body).unwrap_or("<non-utf8>");
            return Err(CoreError::Provider(format!("HTTP {}: {}", resp.status, snippet)));
        }

        let v: Value =
            serde_json::from_slice(&resp.body).map_err(|e| CoreError::Provider(e.to_string()))?;

        // Concatenate the text blocks of the response content. (tool_use blocks
        // are handled by the agent loop in Phase 2.)
        let mut text = String::new();
        if let Some(blocks) = v.get("content").and_then(|c| c.as_array()) {
            for b in blocks {
                if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                    if let Some(t) = b.get("text").and_then(|t| t.as_str()) {
                        text.push_str(t);
                    }
                }
            }
        }

        Ok(Message { role: Role::Assistant, content: text })
    }
}
