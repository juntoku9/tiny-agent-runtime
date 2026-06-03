#![no_std]
//! LLM providers. The Anthropic Messages API is the default; an
//! OpenAI-compatible provider is the fallback. Built on the `HttpClient` HAL,
//! so the same provider code runs on every platform.

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use serde_json::{json, Value};

use tar_core::{Content, CoreError, LlmProvider, Role, Turn};
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

/// Map a core `Turn` into an Anthropic message object (content as a block array).
fn turn_to_message(turn: &Turn) -> Value {
    let role = match turn.role {
        Role::Assistant => "assistant",
        _ => "user",
    };
    let mut blocks: Vec<Value> = Vec::new();
    for c in &turn.content {
        match c {
            Content::Text(t) => blocks.push(json!({ "type": "text", "text": t })),
            Content::ToolUse { id, name, input } => {
                blocks.push(json!({ "type": "tool_use", "id": id, "name": name, "input": input }))
            }
            Content::ToolResult { tool_use_id, content } => blocks
                .push(json!({ "type": "tool_result", "tool_use_id": tool_use_id, "content": content })),
        }
    }
    json!({ "role": role, "content": blocks })
}

impl<H: HttpClient> LlmProvider for AnthropicProvider<H> {
    async fn complete(
        &self,
        system: &str,
        turns: &[Turn],
        tools: &[ToolManifest],
    ) -> Result<Turn, CoreError> {
        let msgs: Vec<Value> = turns.iter().map(turn_to_message).collect();

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

        // Parse the response content blocks into a core assistant turn.
        let mut content: Vec<Content> = Vec::new();
        if let Some(blocks) = v.get("content").and_then(|c| c.as_array()) {
            for b in blocks {
                match b.get("type").and_then(|t| t.as_str()) {
                    Some("text") => {
                        if let Some(t) = b.get("text").and_then(|x| x.as_str()) {
                            content.push(Content::Text(t.to_string()));
                        }
                    }
                    Some("tool_use") => {
                        let id = b.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
                        let name = b.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string();
                        let input = b.get("input").cloned().unwrap_or(Value::Null);
                        content.push(Content::ToolUse { id, name, input });
                    }
                    _ => {}
                }
            }
        }

        Ok(Turn { role: Role::Assistant, content })
    }
}
