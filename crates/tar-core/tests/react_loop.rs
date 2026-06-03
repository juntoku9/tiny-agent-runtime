//! Verifies the ReAct tool loop without any network: a mock provider asks for
//! a tool on the first turn and finishes on the second, and a mock tool records
//! that it was invoked.

use std::cell::Cell;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde_json::json;
use tar_core::{AgentLoop, Content, CoreError, LlmProvider, Role, Tool, Turn};
use tar_proto::{ToolInput, ToolManifest, ToolResult};

/// First call → ask for the `echo` tool. Second call → finish with text.
struct MockProvider {
    calls: Cell<u32>,
}

impl LlmProvider for MockProvider {
    fn complete(
        &self,
        _system: &str,
        _turns: &[Turn],
        _tools: &[ToolManifest],
    ) -> impl Future<Output = Result<Turn, CoreError>> {
        let n = self.calls.get();
        self.calls.set(n + 1);
        async move {
            let content = if n == 0 {
                vec![Content::ToolUse {
                    id: "t1".to_string(),
                    name: "echo".to_string(),
                    input: json!({ "msg": "hi" }),
                }]
            } else {
                vec![Content::Text("done".to_string())]
            };
            Ok(Turn { role: Role::Assistant, content })
        }
    }
}

struct EchoTool {
    called: Arc<AtomicBool>,
}

impl Tool for EchoTool {
    fn manifest(&self) -> ToolManifest {
        ToolManifest {
            name: "echo".to_string(),
            description: "echo".to_string(),
            input_schema: json!({ "type": "object" }),
        }
    }

    fn call<'a>(
        &'a self,
        _input: ToolInput,
    ) -> Pin<Box<dyn Future<Output = ToolResult> + 'a>> {
        self.called.store(true, Ordering::SeqCst);
        Box::pin(async { ToolResult { ok: true, content: "echoed".to_string() } })
    }
}

#[test]
fn react_loop_executes_tools_then_finishes() {
    let called = Arc::new(AtomicBool::new(false));
    let tools: Vec<Box<dyn Tool>> = vec![Box::new(EchoTool { called: called.clone() })];
    let agent = AgentLoop::new(MockProvider { calls: Cell::new(0) }, tools);

    let out = pollster::block_on(agent.run("sys", "go")).expect("loop should finish");

    assert_eq!(out, "done", "final text should be the model's closing turn");
    assert!(called.load(Ordering::SeqCst), "the tool must have been invoked");
}
