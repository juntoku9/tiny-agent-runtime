#![no_std]
//! Node protocol and tool-schema types, shared by the brain, tool-nodes, and
//! any future cloud component. Defined once, serialized with serde — so a
//! struct on the MCU and the JSON on the wire can never drift apart.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A tool's advertised contract. `input_schema` is JSON Schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolManifest {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// Input passed to a tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInput {
    pub args: Value,
}

/// Result returned from a tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub ok: bool,
    pub content: String,
}

/// Messages exchanged on the LAN node protocol (line-delimited JSON).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NodeMessage {
    /// A node announces itself and the tools it offers.
    Hello {
        node_id: String,
        capabilities: Vec<ToolManifest>,
    },
    /// The brain asks a node to run a tool. `cap` is a capability token.
    ToolCall {
        id: String,
        tool: String,
        input: ToolInput,
        cap: Option<String>,
    },
    /// A node returns the outcome of a tool call.
    ToolResultMsg { id: String, result: ToolResult },
}
