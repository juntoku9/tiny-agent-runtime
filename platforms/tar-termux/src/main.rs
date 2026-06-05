//! Android/Termux demo: the ReAct agent loop drives the phone's flashlight and
//! light sensor (via Termux:API), and can be reached from Telegram.
//!
//! Modes:
//!   * default                       → local flashlight-vs-light reflex cycles.
//!   * TAR_TG_TOKEN set              → Telegram bot; text it to act/read sensors.
//!   * `--features cloud` + API key  → a real cloud LLM becomes the brain.
//!
//! Telegram uses `curl`, so the default build needs no TLS/reqwest and no key.

use std::cell::Cell;
use std::env;
use std::future::Future;
use std::pin::Pin;
use std::process::{Command, Output};
use std::time::Duration;

use serde_json::{json, Value};
use tar_core::{
    AgentEvent, AgentLoop, Content, CoreError, LlmProvider, Observer, Role, Tool, Turn,
};
use tar_proto::{ToolInput, ToolManifest, ToolResult};

const DEFAULT_DARK_LUX: f64 = 10.0;
#[cfg_attr(not(feature = "cloud"), allow(dead_code))]
const LLM_SYSTEM: &str = "You are a tiny agent running on an Android phone via Termux. You can \
    read the ambient light sensor (read_light) and control the flashlight (flashlight). Be concise \
    and friendly. Use the tools when asked about light or to toggle the flashlight.";

fn log(tag: &str, msg: impl AsRef<str>) {
    println!("[{tag}] {}", msg.as_ref());
}

// ---- command runner with a timeout ----------------------------------------

fn have(cmd: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {cmd}"))
        .output()
        .map(|o| o.status.success() && !o.stdout.is_empty())
        .unwrap_or(false)
}

async fn run_cmd(cmd: &str, args: &[&str], timeout_s: u64) -> Result<Output, String> {
    let cmd_s = cmd.to_string();
    let args_v: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let job = tokio::task::spawn_blocking(move || Command::new(&cmd_s).args(&args_v).output());
    match tokio::time::timeout(Duration::from_secs(timeout_s), job).await {
        Ok(Ok(Ok(out))) => Ok(out),
        Ok(Ok(Err(e))) => Err(format!("could not run {cmd} ({e})")),
        Ok(Err(e)) => Err(format!("task error ({e})")),
        Err(_) => Err(format!("{cmd} timed out after {timeout_s}s")),
    }
}

// ---- Termux:API helpers ---------------------------------------------------

async fn torch(on: bool) -> Result<(), String> {
    let arg = if on { "on" } else { "off" };
    log("cmd", format!("termux-torch {arg}"));
    let out = run_cmd("termux-torch", &[arg], 6).await?;
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

async fn try_read(args: &[&str]) -> Option<f64> {
    log("cmd", format!("termux-sensor {}", args.join(" ")));
    let out = match run_cmd("termux-sensor", args, 8).await {
        Ok(o) => o,
        Err(e) => {
            log("sensor", e);
            return None;
        }
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    if stdout.trim().is_empty() {
        log("sensor", "empty output");
        return None;
    }
    let v: Value = match serde_json::from_str(stdout.trim()) {
        Ok(v) => v,
        Err(e) => {
            log("sensor", format!("could not parse JSON: {e}"));
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

async fn read_lux() -> Result<f64, String> {
    if let Some(l) = try_read(&["-s", "light", "-n", "1"]).await {
        return Ok(l);
    }
    if let Some(l) = try_read(&["-a", "-n", "1"]).await {
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
            match torch(on).await {
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
            match read_lux().await {
                Ok(lux) => ToolResult { ok: true, content: format!("{lux:.1} lux") },
                Err(e) => ToolResult { ok: false, content: e },
            }
        })
    }
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

// ---- Reflex brain for the standalone cycle demo (light -> flashlight) ------

struct ReflexProvider {
    step: Cell<u32>,
    dark_lux: f64,
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
                0 => vec![Content::ToolUse { id: "s1".into(), name: "read_light".into(), input: json!({}) }],
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

// ---- Command brain for Telegram (reads the user's text) -------------------

#[derive(Clone, Copy)]
enum Intent {
    On,
    Off,
    Status,
    Help,
}

fn classify(text: &str) -> Intent {
    let t = text.to_lowercase();
    let toks: Vec<&str> = t.split_whitespace().collect();
    if t.contains("help") {
        Intent::Help
    } else if toks.contains(&"off") {
        Intent::Off
    } else if toks.contains(&"on") {
        Intent::On
    } else {
        Intent::Status
    }
}

fn first_user_text(turns: &[Turn]) -> String {
    for turn in turns {
        if matches!(turn.role, Role::User) {
            for c in &turn.content {
                if let Content::Text(t) = c {
                    return t.clone();
                }
            }
        }
    }
    String::new()
}

struct CommandReflexProvider {
    step: Cell<u32>,
    dark_lux: f64,
}

impl LlmProvider for CommandReflexProvider {
    fn complete(
        &self,
        _system: &str,
        turns: &[Turn],
        _tools: &[ToolManifest],
    ) -> impl Future<Output = Result<Turn, CoreError>> {
        let n = self.step.get();
        self.step.set(n + 1);
        let intent = classify(&first_user_text(turns));
        let lux = last_lux(turns);
        let dark_lux = self.dark_lux;
        async move {
            let content = match (n, intent) {
                (0, Intent::Help) => vec![Content::Text(
                    "I'm your edge agent 🔦. Try: 'light on', 'light off', or 'status' (ambient light).".into(),
                )],
                (0, Intent::On) => {
                    vec![Content::ToolUse { id: "c".into(), name: "flashlight".into(), input: json!({ "on": true }) }]
                }
                (0, Intent::Off) => {
                    vec![Content::ToolUse { id: "c".into(), name: "flashlight".into(), input: json!({ "on": false }) }]
                }
                (0, Intent::Status) => {
                    vec![Content::ToolUse { id: "c".into(), name: "read_light".into(), input: json!({}) }]
                }
                (_, Intent::On) => vec![Content::Text("Flashlight is ON ✅".into())],
                (_, Intent::Off) => vec![Content::Text("Flashlight is OFF".into())],
                (_, Intent::Help) => vec![Content::Text("Try: 'light on', 'light off', or 'status'.".into())],
                (_, Intent::Status) => {
                    let l = lux.unwrap_or(-1.0);
                    let dark = l >= 0.0 && l < dark_lux;
                    let note = if l < 0.0 {
                        " (sensor read failed — is Termux:API installed?)".to_string()
                    } else {
                        String::new()
                    };
                    vec![Content::Text(format!(
                        "Ambient light: {:.0} lux — it's {}{}.",
                        l,
                        if dark { "dark 🌙" } else { "bright ☀️" },
                        note
                    ))]
                }
            };
            Ok(Turn { role: Role::Assistant, content })
        }
    }
}

// ---- Telegram channel (over curl — no reqwest needed) ---------------------

mod telegram {
    use super::run_cmd;
    use serde_json::Value;

    pub struct Update {
        pub update_id: i64,
        pub chat_id: i64,
        pub text: String,
    }

    async fn curl_get(url: &str, max: u64) -> Result<String, String> {
        let maxs = max.to_string();
        let out = run_cmd("curl", &["-s", "--max-time", &maxs, url], max + 5).await?;
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }

    fn ok_or_err(body: &str) -> Result<Value, String> {
        let v: Value = serde_json::from_str(body.trim()).map_err(|_| format!("bad response: {body}"))?;
        if v.get("ok").and_then(|x| x.as_bool()) != Some(true) {
            let desc = v.get("description").and_then(|x| x.as_str()).unwrap_or("unknown");
            return Err(desc.to_string());
        }
        Ok(v)
    }

    pub async fn get_me(token: &str) -> Result<String, String> {
        let v = ok_or_err(&curl_get(&format!("https://api.telegram.org/bot{token}/getMe"), 15).await?)?;
        Ok(v.get("result").and_then(|r| r.get("username")).and_then(|x| x.as_str()).unwrap_or("?").to_string())
    }

    pub async fn delete_webhook(token: &str) {
        let _ = curl_get(&format!("https://api.telegram.org/bot{token}/deleteWebhook"), 15).await;
    }

    pub async fn get_updates(token: &str, offset: i64) -> Result<Vec<Update>, String> {
        let url = format!("https://api.telegram.org/bot{token}/getUpdates?timeout=20&offset={offset}");
        let v = ok_or_err(&curl_get(&url, 30).await?)?;
        let mut out = Vec::new();
        for u in v.get("result").and_then(|r| r.as_array()).into_iter().flatten() {
            let update_id = u.get("update_id").and_then(|x| x.as_i64()).unwrap_or(0);
            let (chat_id, text) = u
                .get("message")
                .map(|m| {
                    (
                        m.get("chat").and_then(|c| c.get("id")).and_then(|x| x.as_i64()).unwrap_or(0),
                        m.get("text").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                    )
                })
                .unwrap_or((0, String::new()));
            out.push(Update { update_id, chat_id, text });
        }
        Ok(out)
    }

    pub async fn send_message(token: &str, chat_id: i64, text: &str) -> Result<(), String> {
        let url = format!("https://api.telegram.org/bot{token}/sendMessage");
        let chat = format!("chat_id={chat_id}");
        let txt = format!("text={text}");
        run_cmd(
            "curl",
            &["-s", "--max-time", "15", &url, "--data-urlencode", &chat, "--data-urlencode", &txt],
            20,
        )
        .await?;
        Ok(())
    }
}

fn parse_allow(s: Option<String>) -> Vec<i64> {
    s.map(|s| s.split(',').filter_map(|x| x.trim().parse().ok()).collect()).unwrap_or_default()
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
            super::log("llm", format!("HTTPS POST {url} (calling the cloud LLM)"));
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

// ---- Per-message handler --------------------------------------------------

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

async fn finish<P: LlmProvider>(provider: P, system: &str, text: &str) -> String {
    let tools: Vec<Box<dyn Tool>> = vec![Box::new(LightSensorTool), Box::new(FlashlightTool)];
    let agent = AgentLoop::new(provider, tools).with_observer(Box::new(Printer));
    match agent.run(system, text).await {
        Ok(t) if !t.trim().is_empty() => t,
        Ok(_) => "(done)".into(),
        Err(e) => format!("error: {e:?}"),
    }
}

async fn handle_message(text: &str, dark_lux: f64) -> String {
    #[cfg(feature = "cloud")]
    if let Ok(key) = env::var("ANTHROPIC_API_KEY") {
        if !key.is_empty() {
            let model = env::var("TAR_MODEL").unwrap_or_else(|_| "claude-sonnet-4-6".into());
            return finish(make_anthropic(key, model), LLM_SYSTEM, text).await;
        }
    }
    finish(CommandReflexProvider { step: Cell::new(0), dark_lux }, "", text).await
}

async fn run_telegram(token: String, dark_lux: f64) {
    log("tg", "checking bot token (getMe)...");
    match telegram::get_me(&token).await {
        Ok(name) => log("tg", format!("token OK — bot is @{name}")),
        Err(e) => {
            log("tg", format!("TOKEN/NETWORK ERROR: {e}"));
            log("tg", "fix the token (from @BotFather) and re-run. Stopping.");
            return;
        }
    }
    telegram::delete_webhook(&token).await;

    let allow = parse_allow(env::var("TAR_TG_ALLOW").ok());
    if allow.is_empty() {
        log("tg", "WARNING: TAR_TG_ALLOW not set — anyone who finds your bot can use it.");
        log("tg", "Message it once; the chat id is logged so you can set TAR_TG_ALLOW=<id>.");
    }
    #[cfg(feature = "cloud")]
    let llm = env::var("ANTHROPIC_API_KEY").map(|k| !k.is_empty()).unwrap_or(false);
    #[cfg(not(feature = "cloud"))]
    let llm = false;
    log("tg", if llm { "brain: CLOUD LLM" } else { "brain: local command reflex (try 'light on' / 'light off' / 'status')" });
    log("tg", "live — message your bot now. (Ctrl+C to stop.)");

    let mut offset: i64 = 0;
    let mut polls: u64 = 0;
    loop {
        let updates = match telegram::get_updates(&token, offset).await {
            Ok(u) => u,
            Err(e) => {
                log("tg", format!("getUpdates error: {e} — retrying in 3s"));
                tokio::time::sleep(Duration::from_secs(3)).await;
                continue;
            }
        };
        polls += 1;
        if updates.is_empty() {
            if polls % 4 == 0 {
                log("tg", "still listening (no messages yet)...");
            }
        } else {
            log("tg", format!("received {} update(s)", updates.len()));
        }
        for u in updates {
            offset = u.update_id + 1;
            if u.text.is_empty() {
                continue;
            }
            log("tg", format!("<- [{}] {}", u.chat_id, u.text));
            if !allow.is_empty() && !allow.contains(&u.chat_id) {
                log("tg", format!("chat {} not in allowlist — ignored", u.chat_id));
                let _ = telegram::send_message(&token, u.chat_id, "Not authorized.").await;
                continue;
            }
            let reply = handle_message(&u.text, dark_lux).await;
            log("tg", format!("-> [{}] {}", u.chat_id, reply));
            let _ = telegram::send_message(&token, u.chat_id, &reply).await;
        }
    }
}

// ---- main -----------------------------------------------------------------

async fn run_cycle<P: LlmProvider>(provider: P) {
    let tools: Vec<Box<dyn Tool>> = vec![Box::new(LightSensorTool), Box::new(FlashlightTool)];
    let agent = AgentLoop::new(provider, tools).with_observer(Box::new(Printer));
    let _ = agent.run("", "Check the ambient light and set the flashlight.").await;
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let dark_lux = env::var("TAR_DARK_LUX").ok().and_then(|s| s.parse().ok()).unwrap_or(DEFAULT_DARK_LUX);
    let cycles: u32 = env::var("TAR_CYCLES").ok().and_then(|s| s.parse().ok()).unwrap_or(8);

    log("info", "Tiny Agent Runtime — Termux demo");
    log("check", format!("termux-torch:  {}", have("termux-torch")));
    log("check", format!("termux-sensor: {}", have("termux-sensor")));
    log("check", "NOTE: the Termux:API *app* must be installed (F-Droid) + granted Camera/Sensors.");

    // Telegram mode (works in the default build via curl — no key required).
    if let Ok(token) = env::var("TAR_TG_TOKEN") {
        if !token.is_empty() {
            if !have("curl") {
                log("tg", "curl not found — run:  pkg install -y curl");
                return;
            }
            run_telegram(token, dark_lux).await;
            return;
        }
    }

    // Standalone flashlight reflex cycles.
    log("info", format!("no TAR_TG_TOKEN — running {cycles} flashlight cycles (dark < {dark_lux:.0} lux)"));
    log("selftest", "blinking flashlight for 0.7s...");
    let _ = torch(true).await;
    tokio::time::sleep(Duration::from_millis(700)).await;
    let _ = torch(false).await;
    match read_lux().await {
        Ok(l) => log("selftest", format!("ambient light = {l:.1} lux")),
        Err(e) => log("selftest", format!("light read FAILED: {e}")),
    }
    for i in 1..=cycles {
        log("cycle", format!("===== {i}/{cycles} ====="));
        run_cycle(ReflexProvider { step: Cell::new(0), dark_lux }).await;
        tokio::time::sleep(Duration::from_secs(4)).await;
    }
    let _ = torch(false).await;
    log("done", "tune with TAR_CYCLES / TAR_DARK_LUX");
}
