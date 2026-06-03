//! Linux LLM demo (Phase 1): run the portable `tar-core` agent loop against the
//! cloud LLM through the reqwest `HttpClient` HAL.

use std::env;

use tar_core::AgentLoop;
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
    // No tools here; connect remote tools (Phase 2) to let the model drive them.
    let agent = AgentLoop::new(provider, Vec::new());

    match agent
        .run(
            "You are a tiny agent running on an edge device.",
            "Say hello from the edge in one short sentence.",
        )
        .await
    {
        Ok(text) => println!("LLM: {}", text),
        Err(e) => {
            eprintln!("agent error: {:?}", e);
            std::process::exit(1);
        }
    }
}
