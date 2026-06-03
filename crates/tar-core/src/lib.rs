#![no_std]
//! The portable agent core. Pure logic with no I/O: it depends only on the
//! HAL traits and the shared protocol types. The same sources compile for an
//! MCU (`no_std`) and a Linux SoC.

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::future::Future;
use core::pin::Pin;

use serde_json::Value;
use tar_hal::Storage;
use tar_proto::{ToolInput, ToolManifest, ToolResult};

/// Conversation roles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
    System,
}

/// A piece of turn content. Rich enough to carry the tool-use protocol so the
/// agent loop is provider-agnostic; the provider maps these to/from its wire
/// format (e.g. Anthropic content blocks).
#[derive(Debug, Clone)]
pub enum Content {
    Text(String),
    ToolUse { id: String, name: String, input: Value },
    ToolResult { tool_use_id: String, content: String },
}

/// One conversation turn: a role plus a sequence of content blocks.
#[derive(Debug, Clone)]
pub struct Turn {
    pub role: Role,
    pub content: Vec<Content>,
}

/// A capability the agent can invoke. Local pins and remote tool-nodes
/// implement the *same* trait; "remote" is just a different `call` transport.
///
/// `call` is boxed (rather than an `async fn`) so a registry can hold
/// `Box<dyn Tool>`: async fns in traits are not yet dyn-compatible.
pub trait Tool {
    fn manifest(&self) -> ToolManifest;
    fn call<'a>(
        &'a self,
        input: ToolInput,
    ) -> Pin<Box<dyn Future<Output = ToolResult> + 'a>>;
}

/// A cloud (or local) LLM provider. Implemented in `tar-llm`. Returns the
/// assistant turn; the agent loop inspects it for `ToolUse` blocks.
pub trait LlmProvider {
    fn complete(
        &self,
        system: &str,
        turns: &[Turn],
        tools: &[ToolManifest],
    ) -> impl Future<Output = Result<Turn, CoreError>>;
}

#[derive(Debug, Clone)]
pub enum CoreError {
    Provider(String),
    BudgetExhausted,
    /// Transport / (de)serialization failure on the node protocol.
    Io(String),
}

pub mod node;

/// Bounds the ReAct loop so a runaway model cannot spend without limit.
#[derive(Debug, Clone, Copy)]
pub struct Budget {
    pub max_iterations: u8,
}

impl Default for Budget {
    fn default() -> Self {
        Self { max_iterations: 8 }
    }
}

/// Builds the system prompt from persisted bootstrap files + memory.
///
/// Phase 1 concatenates the personality/user/memory Markdown read via
/// `Storage`; the skeleton fixes the contract.
pub fn build_system_prompt(storage: &dyn Storage) -> String {
    let _ = storage;
    String::new()
}

/// The ReAct agent loop, generic over its LLM provider. Holds a registry of
/// tools (local or remote) and runs reason → act → observe until the model
/// ends its turn or the budget is spent.
pub struct AgentLoop<P: LlmProvider> {
    provider: P,
    tools: Vec<Box<dyn Tool>>,
    budget: Budget,
}

impl<P: LlmProvider> AgentLoop<P> {
    pub fn new(provider: P, tools: Vec<Box<dyn Tool>>) -> Self {
        Self { provider, tools, budget: Budget::default() }
    }

    /// Tool manifests advertised to the provider, built from the live registry.
    pub fn manifests(&self) -> Vec<ToolManifest> {
        self.tools.iter().map(|t| t.manifest()).collect()
    }

    /// Execute a tool by name against the registry.
    async fn dispatch(&self, name: &str, input: ToolInput) -> ToolResult {
        for t in &self.tools {
            if t.manifest().name == name {
                return t.call(input).await;
            }
        }
        ToolResult { ok: false, content: format!("unknown tool: {}", name) }
    }

    /// Run the ReAct loop on a system prompt and a single user message.
    ///
    /// Each turn: call the provider; if the assistant returns `tool_use`
    /// blocks, execute them, append the assistant turn and a `tool_result`
    /// turn, and loop; otherwise return the concatenated assistant text.
    pub async fn run(&self, system: &str, user_text: &str) -> Result<String, CoreError> {
        let manifests = self.manifests();

        let mut first = Vec::new();
        first.push(Content::Text(String::from(user_text)));
        let mut turns: Vec<Turn> = Vec::new();
        turns.push(Turn { role: Role::User, content: first });

        let mut iterations: u8 = 0;
        while iterations < self.budget.max_iterations {
            iterations += 1;

            let assistant = self.provider.complete(system, &turns, &manifests).await?;

            let mut calls: Vec<(String, String, Value)> = Vec::new();
            let mut text = String::new();
            for c in &assistant.content {
                match c {
                    Content::Text(t) => text.push_str(t),
                    Content::ToolUse { id, name, input } => {
                        calls.push((id.clone(), name.clone(), input.clone()))
                    }
                    Content::ToolResult { .. } => {}
                }
            }

            if calls.is_empty() {
                return Ok(text);
            }

            let mut results: Vec<Content> = Vec::new();
            for (id, name, input) in calls {
                let r = self.dispatch(&name, ToolInput { args: input }).await;
                results.push(Content::ToolResult { tool_use_id: id, content: r.content });
            }

            turns.push(assistant);
            turns.push(Turn { role: Role::User, content: results });
        }

        Err(CoreError::BudgetExhausted)
    }
}
