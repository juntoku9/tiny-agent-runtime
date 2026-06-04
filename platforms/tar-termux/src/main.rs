//! Android/Termux demo: the real ReAct agent loop drives the phone's flashlight
//! based on its ambient-light sensor — entirely on-device, no network, no key.
//!
//! The flashlight and light sensor are exposed as agent tools (via Termux:API).
//! The "brain" here is a local reflex provider so it runs offline; swap in the
//! cloud LLM later by pointing the same tools at `AnthropicProvider`.

use std::cell::Cell;
use std::env;
use std::future::Future;
use std::pin::Pin;
use std::process::Command;
use std::time::Duration;

use serde_json::{json, Value};
use tar_core::{
    AgentEvent, AgentLoop, Content, CoreError, LlmProvider, Observer, Role, Tool, Turn,
};
use tar_proto::{ToolInput, ToolManifest, ToolResult};

const DEFAULT_DARK_LUX: f64 = 10.0;

// ---- Termux:API helpers ---------------------------------------------------

fn torch(on: bool) -> Result<(), String> {
    let status = Command::new("termux-torch")
        .arg(if on { "on" } else { "off" })
        .status()
        .map_err(|e| format!("termux-torch failed ({e}) — is the Termux:API app installed?"))?;
    if status.success() {
        Ok(())
    } else {
        Err("termux-torch returned an error".into())
    }
}

fn read_lux() -> Result<f64, String> {
    let out = Command::new("termux-sensor")
        .args(["-s", "light", "-n", "1"])
        .output()
        .map_err(|e| format!("termux-sensor failed ({e}) — is the Termux:API app installed?"))?;
    let v: Value =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("parse sensor output: {e}"))?;
    // Format: { "<sensor name>": { "values": [ lux, ... ] }, ... }
    for (_name, sensor) in v.as_object().ok_or("unexpected sensor output")? {
        if let Some(lux) = sensor
            .get("values")
            .and_then(|x| x.as_array())
            .and_then(|a| a.first())
            .and_then(|x| x.as_f64())
        {
            return Ok(lux);
        }
    }
    Err("no light value in sensor output".into())
}

// ---- Tools ----------------------------------------------------------------

struct FlashlightTool;

impl Tool for FlashlightTool {
    fn manifest(&self) -> ToolManifest {
        ToolManifest {
            name: "flashlight".into(),
            description: "Turn the phone flashlight on or off.".into(),
            input_schema: json!({
                "type": "object",
                "properties": { "on": { "type": "boolean" } },
                "required": ["on"]
            }),
        }
    }

    fn call<'a>(&'a self, input: ToolInput) -> Pin<Box<dyn Future<Output = ToolResult> + 'a>> {
        Box::pin(async move {
            let on = input.args.get("on").and_then(|v| v.as_bool()).unwrap_or(false);
            match torch(on) {
                Ok(()) => ToolResult {
                    ok: true,
                    content: if on { "flashlight ON".into() } else { "flashlight OFF".into() },
                },
                Err(e) => ToolResult { ok: false, content: e },
            }
        })
    }
}

struct LightSensorTool;

impl Tool for LightSensorTool {
    fn manifest(&self) -> ToolManifest {
        ToolManifest {
            name: "read_light".into(),
            description: "Read the ambient light level in lux.".into(),
            input_schema: json!({ "type": "object", "properties": {} }),
        }
    }

    fn call<'a>(&'a self, _input: ToolInput) -> Pin<Box<dyn Future<Output = ToolResult> + 'a>> {
        Box::pin(async move {
            match read_lux() {
                Ok(lux) => ToolResult { ok: true, content: format!("{lux:.1} lux") },
                Err(e) => ToolResult { ok: false, content: e },
            }
        })
    }
}

// ---- Reflex "brain" -------------------------------------------------------
//
// A minimal LlmProvider so the loop runs offline: read the light, then turn the
// flashlight on iff it's dark, then report. Swap this for AnthropicProvider to
// let the cloud model make the call instead.

struct ReflexProvider {
    step: Cell<u32>,
    dark_lux: f64,
}

/// Pull the most recent lux value out of a prior tool result ("12.3 lux").
fn last_lux(turns: &[Turn]) -> Option<f64> {
    for turn in turns.iter().rev() {
        for c in &turn.content {
            if let Content::ToolResult { content, .. } = c {
                if let Some(f) = content.split_whitespace().next().and_then(|s| s.parse().ok()) {
                    return Some(f);
                }
            }
        }
    }
    None
}

impl LlmProvider for ReflexProvider {
    fn complete(
        &self,
        _system: &str,
        turns: &[Turn],
        _tools: &[ToolManifest],
    ) -> impl Future<Output = Result<Turn, CoreError>> {
        let n = self.step.get();
        self.step.set(n + 1);
        let lux = last_lux(turns);
        let dark_lux = self.dark_lux;
        async move {
            let content = match n {
                0 => vec![Content::ToolUse {
                    id: "s1".into(),
                    name: "read_light".into(),
                    input: json!({}),
                }],
                1 => {
                    let dark = lux.map(|l| l < dark_lux).unwrap_or(false);
                    vec![Content::ToolUse {
                        id: "s2".into(),
                        name: "flashlight".into(),
                        input: json!({ "on": dark }),
                    }]
                }
                _ => {
                    let l = lux.unwrap_or(-1.0);
                    let dark = l >= 0.0 && l < dark_lux;
                    vec![Content::Text(format!(
                        "Ambient {:.1} lux → it's {} → flashlight {}.",
                        l,
                        if dark { "dark" } else { "bright" },
                        if dark { "ON" } else { "OFF" }
                    ))]
                }
            };
            Ok(Turn { role: Role::Assistant, content })
        }
    }
}

// ---- Pretty console output for the loop -----------------------------------

struct Printer;

impl Observer for Printer {
    fn on_event(&self, event: &AgentEvent) {
        match event {
            AgentEvent::ToolCall { name, input } => println!("  -> {name}({input})"),
            AgentEvent::ToolResult { name, ok, content } => {
                println!("  <- {name}: {content}{}", if *ok { "" } else { "  [error]" })
            }
            AgentEvent::Finished(t) => println!("  = {t}"),
            _ => {}
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let dark_lux = env::var("TAR_DARK_LUX").ok().and_then(|s| s.parse().ok()).unwrap_or(DEFAULT_DARK_LUX);
    let cycles: u32 = env::var("TAR_CYCLES").ok().and_then(|s| s.parse().ok()).unwrap_or(8);

    println!("Tiny Agent Runtime - Termux flashlight demo");
    println!("Cover the light sensor to make it dark (threshold = {dark_lux:.0} lux).\n");

    // Hardware self-test: prove the flashlight + sensor work before the agent runs.
    print!("self-test: blinking flashlight... ");
    let _ = torch(true);
    tokio::time::sleep(Duration::from_millis(700)).await;
    let _ = torch(false);
    match read_lux() {
        Ok(l) => println!("ok. ambient = {l:.1} lux\n"),
        Err(e) => println!("\n  sensor error: {e}\n"),
    }

    // Run the agent loop repeatedly so you can cover/uncover the sensor and watch.
    for i in 1..=cycles {
        println!("cycle {i}/{cycles}:");
        let tools: Vec<Box<dyn Tool>> =
            vec![Box::new(LightSensorTool), Box::new(FlashlightTool)];
        let agent = AgentLoop::new(ReflexProvider { step: Cell::new(0), dark_lux }, tools)
            .with_observer(Box::new(Printer));
        let _ = agent.run("Keep the flashlight on only when it is dark.", "check and act").await;
        tokio::time::sleep(Duration::from_secs(4)).await;
    }

    let _ = torch(false);
    println!("\ndone. tune with TAR_CYCLES=<n> and TAR_DARK_LUX=<lux>.");
}
