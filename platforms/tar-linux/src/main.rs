//! Linux LLM demo (Phase 1): run the portable `tar-core` agent loop against the
//! cloud LLM through the reqwest `HttpClient` HAL.

use std::env;

use tar_core::{AgentLoop, Message, Role};
use tar_linux::http::LinuxHttp;
use tar_llm::AnthropicProvider;

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
