#![no_std]
//! Capability + rate-limit + confirmation policy for tool calls, with extra
//! scrutiny on physical actuators. Evaluated before any tool executes, so a
//! prompt-injected model cannot freely toggle a relay or a lock.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Outcome of a policy check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    /// Requires explicit human confirmation before executing.
    Confirm,
    Deny(DenyReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DenyReason {
    NotInAllowlist,
    MissingCapability,
    RateLimited,
}

/// A per-tool allowlist with a max-calls-per-window rate limit and a set of
/// tools that require human confirmation before they run.
pub struct PolicyEngine {
    allowed_tools: Vec<String>,
    require_confirm: Vec<String>,
    max_calls_per_window: u32,
    calls_this_window: u32,
}

impl PolicyEngine {
    pub fn new(
        allowed_tools: Vec<String>,
        require_confirm: Vec<String>,
        max_calls_per_window: u32,
    ) -> Self {
        Self {
            allowed_tools,
            require_confirm,
            max_calls_per_window,
            calls_this_window: 0,
        }
    }

    /// Evaluate a tool call. `has_capability` is whether the caller presented a
    /// valid capability token for this tool.
    pub fn evaluate(&mut self, tool: &str, has_capability: bool) -> Decision {
        if !self.allowed_tools.iter().any(|t| t == tool) {
            return Decision::Deny(DenyReason::NotInAllowlist);
        }
        if !has_capability {
            return Decision::Deny(DenyReason::MissingCapability);
        }
        if self.calls_this_window >= self.max_calls_per_window {
            return Decision::Deny(DenyReason::RateLimited);
        }
        self.calls_this_window += 1;
        if self.require_confirm.iter().any(|t| t == tool) {
            return Decision::Confirm;
        }
        Decision::Allow
    }

    /// Reset the rate-limit counter at the start of a new window.
    pub fn reset_window(&mut self) {
        self.calls_this_window = 0;
    }
}
