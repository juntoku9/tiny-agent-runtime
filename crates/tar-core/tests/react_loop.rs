//! Verifies the ReAct tool loop without any network: a mock provider asks for
//! a tool on the first turn and finishes on the second, and a mock tool records
//! that it was invoked.

use std::cell::Cell;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::json;
use tar_core::{AgentEvent, AgentLoop, Content, CoreError, LlmProvider, Observer, Role, Tool, Turn};
use tar_proto::{ToolInput, ToolManifest, ToolResult};

/// Records the names of the events the loop emits.
struct RecordingObserver {
    log: Arc<Mutex<Vec<String>>>,
}

impl Observer for RecordingObserver {
    fn on_event(&self, event: &AgentEvent) {
        let tag = match event {
            AgentEvent::Iteration(n) => format!("iteration:{n}"),
            AgentEvent::AssistantText(t) => format!("text:{t}"),
            AgentEvent::ToolCall { name, .. } => format!("tool_call:{name}"),
            AgentEvent::ToolResult { name, ok, .. } => format!("tool_result:{name}:{ok}"),
            AgentEvent::Finished(t) => format!("finished:{t}"),
            AgentEvent::BudgetExhausted => "budget".to_string(),
        };
        self.log.lock().unwrap().push(tag);
    }
}

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
    let log = Arc::new(Mutex::new(Vec::new()));
    let agent = AgentLoop::new(MockProvider { calls: Cell::new(0) }, tools)
        .with_observer(Box::new(RecordingObserver { log: log.clone() }));

    let out = pollster::block_on(agent.run("sys", "go")).expect("loop should finish");

    assert_eq!(out, "done", "final text should be the model's closing turn");
    assert!(called.load(Ordering::SeqCst), "the tool must have been invoked");

    let events = log.lock().unwrap();
    assert!(events.iter().any(|e| e == "tool_call:echo"), "should emit the tool call: {events:?}");
    assert!(
        events.iter().any(|e| e == "tool_result:echo:true"),
        "should emit the tool result: {events:?}"
    );
    assert!(events.iter().any(|e| e == "finished:done"), "should emit the finish: {events:?}");
}
