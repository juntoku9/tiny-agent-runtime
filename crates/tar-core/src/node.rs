//! LAN node protocol. A tool-node serves tool calls over a `Transport`; a
//! brain invokes those tools through the *same* `Tool` trait it uses for local
//! tools. The mechanism is transport-agnostic — platforms supply the transport
//! (TCP on Linux, a socket on the MCU).

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::future::Future;
use core::pin::Pin;

use tar_hal::Transport;
use tar_proto::{NodeMessage, ToolInput, ToolManifest, ToolResult};

use crate::{CoreError, Tool};

fn io<E: core::fmt::Debug>(e: E) -> CoreError {
    CoreError::Io(format!("{:?}", e))
}

/// Serves a tool registry to a connected brain. Sends a `Hello` advertising its
/// capabilities on start, then answers `ToolCall` frames until the link drops.
pub struct NodeServer<T: Transport> {
    node_id: String,
    transport: T,
    tools: Vec<Box<dyn Tool>>,
}

impl<T: Transport> NodeServer<T> {
    pub fn new(node_id: impl Into<String>, transport: T, tools: Vec<Box<dyn Tool>>) -> Self {
        Self { node_id: node_id.into(), transport, tools }
    }

    pub async fn serve(&self) -> Result<(), CoreError> {
        let capabilities: Vec<ToolManifest> =
            self.tools.iter().map(|t| t.manifest()).collect();
        let hello = NodeMessage::Hello { node_id: self.node_id.clone(), capabilities };
        self.transport.send(&serde_json::to_vec(&hello).map_err(io)?).await.map_err(io)?;

        loop {
            let frame = self.transport.recv().await.map_err(io)?;
            let msg: NodeMessage = serde_json::from_slice(&frame).map_err(io)?;
            if let NodeMessage::ToolCall { id, tool, input, .. } = msg {
                let result = self.dispatch(&tool, input).await;
                let out = NodeMessage::ToolResultMsg { id, result };
                self.transport.send(&serde_json::to_vec(&out).map_err(io)?).await.map_err(io)?;
            }
        }
    }

    async fn dispatch(&self, tool: &str, input: ToolInput) -> ToolResult {
        for t in &self.tools {
            if t.manifest().name == tool {
                return t.call(input).await;
            }
        }
        ToolResult { ok: false, content: format!("unknown tool: {}", tool) }
    }
}

/// Reads the node's `Hello` and returns the tools it advertises.
pub async fn handshake<T: Transport>(transport: &T) -> Result<Vec<ToolManifest>, CoreError> {
    let frame = transport.recv().await.map_err(io)?;
    match serde_json::from_slice(&frame).map_err(io)? {
        NodeMessage::Hello { capabilities, .. } => Ok(capabilities),
        _ => Err(CoreError::Io("expected hello frame".to_string())),
    }
}

/// A remote tool: implements the same `Tool` trait as a local tool, but
/// forwards each call to a tool-node over a transport. This is the payoff of
/// the design — the agent loop cannot tell a remote pin from a local one.
pub struct RemoteTool<T: Transport> {
    manifest: ToolManifest,
    transport: T,
}

impl<T: Transport> RemoteTool<T> {
    pub fn new(manifest: ToolManifest, transport: T) -> Self {
        Self { manifest, transport }
    }
}

impl<T: Transport> Tool for RemoteTool<T> {
    fn manifest(&self) -> ToolManifest {
        self.manifest.clone()
    }

    fn call<'a>(&'a self, input: ToolInput) -> Pin<Box<dyn Future<Output = ToolResult> + 'a>> {
        Box::pin(async move {
            let call = NodeMessage::ToolCall {
                id: "rpc".to_string(),
                tool: self.manifest.name.clone(),
                input,
                cap: None,
            };
            let frame = match serde_json::to_vec(&call) {
                Ok(f) => f,
                Err(e) => return ToolResult { ok: false, content: format!("encode: {:?}", e) },
            };
            if let Err(e) = self.transport.send(&frame).await {
                return ToolResult { ok: false, content: format!("send: {:?}", e) };
            }
            let frame = match self.transport.recv().await {
                Ok(f) => f,
                Err(e) => return ToolResult { ok: false, content: format!("recv: {:?}", e) },
            };
            match serde_json::from_slice::<NodeMessage>(&frame) {
                Ok(NodeMessage::ToolResultMsg { result, .. }) => result,
                Ok(_) => ToolResult { ok: false, content: "unexpected frame".to_string() },
                Err(e) => ToolResult { ok: false, content: format!("decode: {:?}", e) },
            }
        })
    }
}
