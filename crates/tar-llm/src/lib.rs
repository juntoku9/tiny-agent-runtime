#![no_std]
//! LLM providers. The Anthropic Messages API is the default; an
//! OpenAI-compatible provider is the fallback. Built on the `HttpClient` HAL,
//! so the same provider code runs on every platform.

extern crate alloc;

use alloc::string::{String, ToString};

use tar_core::{CoreError, LlmProvider, Message};
use tar_hal::HttpClient;
use tar_proto::ToolManifest;

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
        // Phase 1: build the Anthropic request body, POST it via `self.http`,
        // and parse the text / tool_use content blocks from the response. The
        // skeleton fixes the provider shape only.
        let _ = (system, messages, tools, &self.api_key, &self.model, &self.http);
        Err(CoreError::Provider("not yet implemented".to_string()))
    }
}
