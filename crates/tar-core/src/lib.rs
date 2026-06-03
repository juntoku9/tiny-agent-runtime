#![no_std]
//! The portable agent core. Pure logic with no I/O: it depends only on the
//! HAL traits and the shared protocol types. The same sources compile for an
//! MCU (`no_std`) and a Linux SoC.

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::future::Future;
use core::pin::Pin;

use tar_hal::Storage;
use tar_proto::{ToolInput, ToolManifest, ToolResult};

/// Conversation roles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
    System,
}

/// A single conversation turn.
#[derive(Debug, Clone)]
pub struct Message {
    pub role: Role,
    pub content: String,
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

/// A cloud (or local) LLM provider. Implemented in `tar-llm`.
pub trait LlmProvider {
    /// One non-streaming completion over the given messages and tools.
    fn complete(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[ToolManifest],
    ) -> impl Future<Output = Result<Message, CoreError>>;
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
/// tools (local or remote) and, once complete, runs reason → act → observe
/// until the model ends its turn or the budget is spent.
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

    /// Run the agent over a system prompt and history.
    ///
    /// Phase 0 issues a single completion; Phase 1 expands this body into the
    /// full tool-execution loop (parse `tool_use`, dispatch tools, feed results
    /// back) bounded by [`Budget`].
    pub async fn run(&self, system: &str, history: &[Message]) -> Result<Message, CoreError> {
        if self.budget.max_iterations == 0 {
            return Err(CoreError::BudgetExhausted);
        }
        let tools = self.manifests();
        // Phase 0: a single completion. Phase 1 loops up to
        // `budget.max_iterations`, dispatching tools between turns.
        self.provider.complete(system, history, &tools).await
    }
}
