//! Agent demo (Phase 2): runs the ReAct loop against the remote tool-node and
//! streams every loop event — iterations, assistant text, tool calls, tool
//! results, final answer — to a live web dashboard so you can watch it think.
//!
//! Without ANTHROPIC_API_KEY it runs a scripted provider (paced with delays)
//! so the loop is watchable offline; with a key, the real model drives.

use std::cell::Cell;
use std::env;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use tar_core::node::{handshake, RemoteTool};
use tar_core::{AgentEvent, AgentLoop, Content, CoreError, LlmProvider, Observer, Role, Tool, Turn};
use tar_linux::http::LinuxHttp;
use tar_linux::transport::TcpTransport;
use tar_llm::AnthropicProvider;
use tar_proto::ToolManifest;

const AGENT_HTML: &str = r##"<!doctype html><html><head><meta charset="utf-8">
<title>Tiny Agent Runtime — agent loop</title>
<style>
 body{font-family:system-ui,sans-serif;background:#0b0e14;color:#cdd6f4;margin:24px;max-width:760px}
 h1{font-size:18px;margin:0 0 2px} .sub{color:#6b7280;font-size:12px;margin-bottom:16px}
 .ev{padding:8px 12px;border-radius:10px;margin:8px 0;font-size:13px;line-height:1.45;border:1px solid #232838}
 .iter{background:#11151c;color:#7f93b0;font-weight:600;text-align:center;border-style:dashed}
 .assistant{background:#15212e;border-color:#1f3a4d}
 .toolcall{background:#241f12;border-color:#4d3f1f}
 .ok{background:#16241a;border-color:#2c4a30} .bad{background:#2a161a;border-color:#5a2730}
 .final{background:#1a2440;border-color:#2f3f6f;font-weight:600}
 .tag{font-size:11px;text-transform:uppercase;letter-spacing:.05em;color:#8a93a6;margin-right:6px}
 code{font-family:ui-monospace,monospace;font-size:12px;color:#e6c384}
</style></head><body>
<h1>Tiny Agent Runtime — agent loop</h1><div class="sub" id="sub">live (polling /state)</div>
<div id="t"></div>
<script>
function esc(s){return (s+'').replace(/[&<>]/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;'}[c]));}
function row(e){
 if(e.kind==='iteration') return '<div class="ev iter">— iteration '+e.n+' —</div>';
 if(e.kind==='assistant_text') return '<div class="ev assistant"><span class="tag">assistant</span>'+esc(e.text)+'</div>';
 if(e.kind==='tool_call') return '<div class="ev toolcall"><span class="tag">tool call</span><code>'+esc(e.name)+'('+esc(e.input)+')</code></div>';
 if(e.kind==='tool_result') return '<div class="ev '+(e.ok?'ok':'bad')+'"><span class="tag">result · '+esc(e.name)+'</span><code>'+esc(e.content)+'</code></div>';
 if(e.kind==='finished') return '<div class="ev final"><span class="tag">done</span>'+esc(e.text)+'</div>';
 if(e.kind==='budget') return '<div class="ev bad">budget exhausted</div>';
 return '';
}
async function tick(){
 try{const s=await (await fetch('/state')).json();
  document.getElementById('t').innerHTML=s.events.map(row).join('');
 }catch(e){}
}
setInterval(tick,300);tick();
</script></body></html>"##;

/// Shares the event buffer between the agent loop (observer) and the dashboard.
struct SharedLog(Arc<Mutex<Vec<Value>>>);

impl Observer for SharedLog {
    fn on_event(&self, event: &AgentEvent) {
        let v = match event {
            AgentEvent::Iteration(n) => json!({ "kind": "iteration", "n": n }),
            AgentEvent::AssistantText(t) => json!({ "kind": "assistant_text", "text": t }),
            AgentEvent::ToolCall { name, input } => {
                json!({ "kind": "tool_call", "name": name, "input": input })
            }
            AgentEvent::ToolResult { name, ok, content } => {
                json!({ "kind": "tool_result", "name": name, "ok": ok, "content": content })
            }
            AgentEvent::Finished(t) => json!({ "kind": "finished", "text": t }),
            AgentEvent::BudgetExhausted => json!({ "kind": "budget" }),
        };
        if let Ok(mut g) = self.0.lock() {
            g.push(v);
        }
    }
}

/// A scripted provider so the loop is watchable without a key: write pin 5 HIGH,
/// read it back, then finish. Paced with a delay so the steps are visible.
struct ScriptedProvider {
    step: Cell<u32>,
    delay: Duration,
}

impl LlmProvider for ScriptedProvider {
    fn complete(
        &self,
        _system: &str,
        _turns: &[Turn],
        _tools: &[ToolManifest],
    ) -> impl Future<Output = Result<Turn, CoreError>> {
        let n = self.step.get();
        self.step.set(n + 1);
        let delay = self.delay;
        async move {
            tokio::time::sleep(delay).await;
            let content = match n {
                0 => vec![Content::ToolUse {
                    id: "c1".to_string(),
                    name: "gpio_write".to_string(),
                    input: json!({ "channel": 5, "level": true }),
                }],
                1 => vec![Content::ToolUse {
                    id: "c2".to_string(),
                    name: "gpio_read".to_string(),
                    input: json!({ "channel": 5 }),
                }],
                _ => vec![Content::Text("Pin 5 is HIGH — task complete.".to_string())],
            };
            Ok(Turn { role: Role::Assistant, content })
        }
    }
}

async fn connect_with_retry(addr: &str) -> TcpTransport {
    for _ in 0..30 {
        if let Ok(t) = TcpTransport::connect(addr).await {
            return t;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    eprintln!("could not connect to node at {}", addr);
    std::process::exit(1);
}

async fn run_agent<P: LlmProvider>(
    provider: P,
    tools: Vec<Box<dyn Tool>>,
    observer: Box<dyn Observer>,
    prompt: &str,
) {
    let agent = AgentLoop::new(provider, tools).with_observer(observer);
    match agent
        .run("You control GPIO on a remote edge node via tools.", prompt)
        .await
    {
        Ok(text) => println!("[agent] final: {}", text),
        Err(e) => eprintln!("[agent] error: {:?}", e),
    }
}

#[tokio::main]
async fn main() {
    let node_addr = env::var("TAR_NODE_ADDR").unwrap_or_else(|_| "127.0.0.1:18810".to_string());
    let dash_addr = env::var("TAR_AGENT_ADDR").unwrap_or_else(|_| "127.0.0.1:8091".to_string());

    let buf: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    {
        let buf = buf.clone();
        let dash = dash_addr.clone();
        tokio::spawn(async move {
            tar_linux::dashboard::serve(dash, AGENT_HTML, move || {
                let g = buf.lock().unwrap();
                json!({ "events": *g }).to_string()
            })
            .await
        });
    }
    println!("agent dashboard: http://{}", dash_addr);

    let transport = Arc::new(connect_with_retry(&node_addr).await);
    let caps = match handshake(&transport).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("handshake failed: {:?}", e);
            std::process::exit(1);
        }
    };
    let tools: Vec<Box<dyn Tool>> = caps
        .iter()
        .map(|m| Box::new(RemoteTool::new(m.clone(), transport.clone())) as Box<dyn Tool>)
        .collect();
    let observer: Box<dyn Observer> = Box::new(SharedLog(buf.clone()));

    match env::var("ANTHROPIC_API_KEY") {
        Ok(key) if !key.is_empty() => {
            let model = env::var("TAR_MODEL").unwrap_or_else(|_| "claude-sonnet-4-6".to_string());
            println!("running with the cloud model ({model})");
            let prompt = "Turn GPIO pin 5 on (HIGH), then read it back and report its state.";
            run_agent(AnthropicProvider::new(LinuxHttp::new(), key, model), tools, observer, prompt).await;
        }
        _ => {
            println!("(no ANTHROPIC_API_KEY — running a scripted loop so you can watch it)");
            let provider = ScriptedProvider { step: Cell::new(0), delay: Duration::from_millis(900) };
            run_agent(provider, tools, observer, "demo").await;
        }
    }

    println!("agent run complete — dashboard still live at http://{}  (Ctrl+C to exit)", dash_addr);
    std::future::pending::<()>().await;
}
