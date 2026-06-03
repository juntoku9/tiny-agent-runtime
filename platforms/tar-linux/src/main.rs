//! Linux edge-SoC platform — the camera "brain".
//!
//! Phase 1: a real `HttpClient` HAL impl (reqwest) so the portable `tar-core`
//! agent loop can reach the cloud LLM. Storage, camera, and transport HALs and
//! the full tool loop land in later phases.

use std::env;
use std::future::Future;

use tar_core::{AgentLoop, Message, Role};
use tar_hal::{HalError, HalResult, HttpClient, HttpResponse};
use tar_llm::AnthropicProvider;

/// reqwest-backed HTTP client — the only outbound path, used for the cloud LLM.
struct LinuxHttp {
    client: reqwest::Client,
}

impl LinuxHttp {
    fn new() -> Self {
        Self { client: reqwest::Client::new() }
    }
}

impl HttpClient for LinuxHttp {
    fn post(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> impl Future<Output = HalResult<HttpResponse>> {
        // Own the inputs so the returned future borrows nothing.
        let client = self.client.clone();
        let url = url.to_string();
        let headers: Vec<(String, String)> =
            headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let body = body.to_vec();
        async move {
            let mut req = client.post(&url).body(body);
            for (k, v) in &headers {
                req = req.header(k, v);
            }
            let resp = req.send().await.map_err(|e| HalError::Io(e.to_string()))?;
            let status = resp.status().as_u16();
            let bytes = resp.bytes().await.map_err(|e| HalError::Io(e.to_string()))?;
            Ok(HttpResponse { status, body: bytes.to_vec() })
        }
    }
}

#[tokio::main]
async fn main() {
    let api_key = match env::var("ANTHROPIC_API_KEY") {
        Ok(k) if !k.is_empty() => k,
        _ => {
            eprintln!("Set ANTHROPIC_API_KEY to talk to the cloud LLM.");
            std::process::exit(1);
        }
    };
    let model = env::var("TAR_MODEL").unwrap_or_else(|_| "claude-sonnet-4-6".to_string());

    let provider = AnthropicProvider::new(LinuxHttp::new(), api_key, model);
    // Phase 1 runs with no tools; Phase 2 adds the dynamic tool registry.
    let agent = AgentLoop::new(provider, Vec::new());

    let history = [Message {
        role: Role::User,
        content: "Say hello from the edge in one short sentence.".to_string(),
    }];

    match agent.run("You are a tiny agent running on an edge device.", &history).await {
        Ok(reply) => println!("LLM: {}", reply.content),
        Err(e) => {
            eprintln!("agent error: {:?}", e);
            std::process::exit(1);
        }
    }
}
