//! Android/Termux demo: the real ReAct agent loop drives the phone's flashlight
//! based on its ambient-light sensor — on-device, via Termux:API.
//!
//! Brains:
//!   * default build → a local *reflex* rule. NO LLM is called (offline).
//!   * `--features cloud` + ANTHROPIC_API_KEY → a real cloud LLM decides.
//!
//! Verbose logging shows every command, its raw output, and which brain runs.

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

fn log(tag: &str, msg: impl AsRef<str>) {
    println!("[{tag}] {}", msg.as_ref());
}

// ---- Termux:API helpers (with verbose logging) ----------------------------

fn have(cmd: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {cmd}"))
        .output()
        .map(|o| o.status.success() && !o.stdout.is_empty())
        .unwrap_or(false)
}

fn torch(on: bool) -> Result<(), String> {
    let arg = if on { "on" } else { "off" };
    log("cmd", format!("termux-torch {arg}"));
    let out = Command::new("termux-torch")
        .arg(arg)
        .output()
        .map_err(|e| format!("could not run termux-torch ({e}) — is the Termux:API app installed?"))?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !stderr.trim().is_empty() {
        log("torch.stderr", stderr.trim());
    }
    if out.status.success() {
        log("torch", if on { ">>> FLASHLIGHT ON <<<" } else { "flashlight off" });
        Ok(())
    } else {
        Err("termux-torch returned a non-zero status".into())
    }
}

/// Run termux-sensor with the given args and pull out the first light reading.
fn try_read(args: &[&str]) -> Option<f64> {
    log("cmd", format!("termux-sensor {}", args.join(" ")));
    let out = Command::new("termux-sensor").args(args).output().ok()?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !stderr.trim().is_empty() {
        log("sensor.stderr", stderr.trim());
    }
    if stdout.trim().is_empty() {
        log("sensor", "empty output");
        return None;
    }
    let v: Value = match serde_json::from_str(stdout.trim()) {
        Ok(v) => v,
        Err(e) => {
            log("sensor", format!("could not parse JSON: {e}; raw: {}", stdout.trim()));
            return None;
        }
    };
    let obj = v.as_object()?;
    let mut light_value = None;
    let mut any_value = None;
    for (name, sensor) in obj {
        if let Some(lux) = sensor
            .get("values")
            .and_then(|x| x.as_array())
            .and_then(|a| a.first())
            .and_then(|x| x.as_f64())
        {
            log("sensor", format!("{name} = {lux}"));
            any_value.get_or_insert(lux);
            if name.to_lowercase().contains("light") {
                light_value.get_or_insert(lux);
            }
        }
    }
    light_value.or(any_value)
}

fn read_lux() -> Result<f64, String> {
    if let Some(l) = try_read(&["-s", "light", "-n", "1"]) {
        return Ok(l);
    }
    log("sensor", "no 'light' match — scanning all sensors (-a)...");
    if let Some(l) = try_read(&["-a", "-n", "1"]) {
        return Ok(l);
    }
    Err("could not read a light value — is the Termux:API app installed and permitted?".into())
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

// ---- Reflex "brain" (no LLM) ----------------------------------------------

struct ReflexProvider {
    step: Cell<u32>,
    dark_lux: f64,
}

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
            log("brain", format!("reflex deciding (step {n}, last lux = {lux:?})"));
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
                        "Ambient {:.1} lux → {} → flashlight {}.",
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

// ---- Optional cloud-LLM brain --------------------------------------------

#[cfg(feature = "cloud")]
mod cloud {
    use std::future::Future;
    use tar_hal::{HalError, HalResult, HttpClient, HttpResponse};

    pub struct TermuxHttp {
        client: reqwest::Client,
    }
    impl TermuxHttp {
        pub fn new() -> Self {
            Self { client: reqwest::Client::new() }
        }
    }
    impl HttpClient for TermuxHttp {
        fn post(
            &self,
            url: &str,
            headers: &[(&str, &str)],
            body: &[u8],
        ) -> impl Future<Output = HalResult<HttpResponse>> {
            let client = self.client.clone();
            let url = url.to_string();
            let headers: Vec<(String, String)> =
                headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
            let body = body.to_vec();
            super::log("llm", format!("HTTPS POST {url}  (calling the cloud LLM)"));
            async move {
                let mut req = client.post(&url).body(body);
                for (k, v) in &headers {
                    req = req.header(k, v);
                }
                let resp = req.send().await.map_err(|e| HalError::Io(e.to_string()))?;
                let status = resp.status().as_u16();
                super::log("llm", format!("cloud LLM responded: HTTP {status}"));
                let bytes = resp.bytes().await.map_err(|e| HalError::Io(e.to_string()))?;
                Ok(HttpResponse { status, body: bytes.to_vec() })
            }
        }
    }
}

#[cfg(feature = "cloud")]
fn make_anthropic(key: String, model: String) -> tar_llm::AnthropicProvider<cloud::TermuxHttp> {
    tar_llm::AnthropicProvider::new(cloud::TermuxHttp::new(), key, model)
}

// ---- Loop plumbing --------------------------------------------------------

struct Printer;

impl Observer for Printer {
    fn on_event(&self, event: &AgentEvent) {
        match event {
            AgentEvent::Iteration(n) => log("loop", format!("--- iteration {n} ---")),
            AgentEvent::AssistantText(t) => log("agent", format!("says: {t}")),
            AgentEvent::ToolCall { name, input } => log("agent", format!("calls {name}({input})")),
            AgentEvent::ToolResult { name, ok, content } => {
                log("agent", format!("{name} -> {content}{}", if *ok { "" } else { "  [ERROR]" }))
            }
            AgentEvent::Finished(t) => log("agent", format!("decision: {t}")),
            AgentEvent::BudgetExhausted => log("agent", "budget exhausted"),
        }
    }
}

async fn run_cycle<P: LlmProvider>(provider: P, system: &str) {
    let tools: Vec<Box<dyn Tool>> = vec![Box::new(LightSensorTool), Box::new(FlashlightTool)];
    let agent = AgentLoop::new(provider, tools).with_observer(Box::new(Printer));
    if let Err(e) = agent.run(system, "Check the ambient light and set the flashlight.").await {
        log("error", format!("{e:?}"));
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let dark_lux = env::var("TAR_DARK_LUX").ok().and_then(|s| s.parse().ok()).unwrap_or(DEFAULT_DARK_LUX);
    let cycles: u32 = env::var("TAR_CYCLES").ok().and_then(|s| s.parse().ok()).unwrap_or(8);

    log("info", "Tiny Agent Runtime — Termux flashlight demo");
    log("info", format!("dark threshold = {dark_lux:.0} lux; cover the light sensor to make it dark"));

    // Environment diagnostics — this is usually where "nothing happens" gets explained.
    let have_torch = have("termux-torch");
    let have_sensor = have("termux-sensor");
    log("check", format!("termux-torch installed:  {have_torch}"));
    log("check", format!("termux-sensor installed: {have_sensor}"));
    if !have_torch || !have_sensor {
        log("check", "MISSING. Run:  pkg install -y termux-api");
        log("check", "AND install the 'Termux:API' app from F-Droid (separate from the package).");
    }

    // Which brain?
    #[cfg(feature = "cloud")]
    let cloud = match env::var("ANTHROPIC_API_KEY") {
        Ok(k) if !k.is_empty() => {
            let model = env::var("TAR_MODEL").unwrap_or_else(|_| "claude-sonnet-4-6".into());
            log("brain", format!("CLOUD LLM — {model} runs each cycle over HTTPS to api.anthropic.com"));
            Some((k, model))
        }
        _ => {
            log("brain", "LOCAL REFLEX — no LLM is called. Set ANTHROPIC_API_KEY to use the cloud model.");
            None
        }
    };
    #[cfg(not(feature = "cloud"))]
    log("brain", "LOCAL REFLEX — NO LLM is called (offline). Rebuild with `--features cloud` + ANTHROPIC_API_KEY for a real LLM.");

    let system = format!(
        "You control a phone via two tools: read_light (returns ambient lux) and \
         flashlight (on/off). Turn the flashlight ON only when it is dark — below \
         {dark_lux} lux. Each turn: call read_light, then call flashlight with the \
         correct state, then briefly state the result."
    );

    // Self-test so you can see the hardware respond before the agent runs.
    log("selftest", "blinking flashlight for 0.7s...");
    let _ = torch(true);
    tokio::time::sleep(Duration::from_millis(700)).await;
    let _ = torch(false);
    match read_lux() {
        Ok(l) => log("selftest", format!("ambient light = {l:.1} lux")),
        Err(e) => log("selftest", format!("light read FAILED: {e}")),
    }

    for i in 1..=cycles {
        log("cycle", format!("===== {i}/{cycles} ====="));
        #[cfg(feature = "cloud")]
        match &cloud {
            Some((k, m)) => run_cycle(make_anthropic(k.clone(), m.clone()), &system).await,
            None => run_cycle(ReflexProvider { step: Cell::new(0), dark_lux }, &system).await,
        }
        #[cfg(not(feature = "cloud"))]
        run_cycle(ReflexProvider { step: Cell::new(0), dark_lux }, &system).await;

        tokio::time::sleep(Duration::from_secs(4)).await;
    }

    let _ = torch(false);
    log("done", "tune with TAR_CYCLES=<n> TAR_DARK_LUX=<lux>");
}
