//! Android/Termux demo: the real ReAct agent loop drives the phone's flashlight
//! based on its ambient-light sensor — on-device, via Termux:API.
//!
//! Brains:
//!   * default build → a local *reflex* rule. NO LLM is called (offline).
//!   * `--features cloud` + ANTHROPIC_API_KEY → a real cloud LLM decides.
//!
//! Every external command runs with a timeout and verbose logging, so a
//! missing/unpermitted Termux:API app can't freeze the demo — it reports why.

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

/// Run a command in a blocking thread with a timeout, so a hung Termux:API
/// call (missing or unpermitted app) can't freeze the whole demo.
async fn run_cmd(cmd: &str, args: &[&str], timeout_s: u64) -> Result<Output, String> {
    let cmd_s = cmd.to_string();
    let args_v: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let job = tokio::task::spawn_blocking(move || Command::new(&cmd_s).args(&args_v).output());
    match tokio::time::timeout(Duration::from_secs(timeout_s), job).await {
        Ok(Ok(Ok(out))) => Ok(out),
        Ok(Ok(Err(e))) => Err(format!("could not run {cmd} ({e})")),
        Ok(Err(e)) => Err(format!("task error ({e})")),
        Err(_) => Err(format!(
            "timed out after {timeout_s}s — the Termux:API APP is likely not installed or not \
             permitted (grant it Camera + Sensors in Android settings)"
        )),
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

async fn read_lux() -> Result<f64, String> {
    if let Some(l) = try_read(&["-s", "light", "-n", "1"]).await {
        return Ok(l);
    }
    log("sensor", "no 'light' match — scanning all sensors (-a)...");
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

// ---- Telegram channel -----------------------------------------------------
//
// Long-poll getUpdates, run each message through the agent loop (with the phone
// tools available), reply with sendMessage. Restricted by a chat-id allowlist.

#[cfg(feature = "cloud")]
mod telegram {
    use serde_json::{json, Value};
    use std::time::Duration;

    pub struct Update {
        pub update_id: i64,
        pub chat_id: i64,
        pub text: String,
    }

    /// Verify the token; returns the bot's @username on success.
    pub async fn get_me(client: &reqwest::Client, token: &str) -> Result<String, String> {
        let url = format!("https://api.telegram.org/bot{token}/getMe");
        let resp = client.get(&url).timeout(Duration::from_secs(15)).send().await.map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        let body = resp.text().await.map_err(|e| e.to_string())?;
        let v: Value = serde_json::from_str(&body).map_err(|_| format!("HTTP {status}: {body}"))?;
        if v.get("ok").and_then(|x| x.as_bool()) != Some(true) {
            let desc = v.get("description").and_then(|x| x.as_str()).unwrap_or("unknown");
            return Err(format!("HTTP {status}: {desc} (is the token correct?)"));
        }
        Ok(v.get("result").and_then(|r| r.get("username")).and_then(|x| x.as_str()).unwrap_or("?").to_string())
    }

    /// Remove any webhook so getUpdates works (a set webhook causes 409s).
    pub async fn delete_webhook(client: &reqwest::Client, token: &str) {
        let url = format!("https://api.telegram.org/bot{token}/deleteWebhook");
        let _ = client.get(&url).timeout(Duration::from_secs(15)).send().await;
    }

    pub async fn get_updates(
        client: &reqwest::Client,
        token: &str,
        offset: i64,
    ) -> Result<Vec<Update>, String> {
        let url = format!("https://api.telegram.org/bot{token}/getUpdates?timeout=30&offset={offset}");
        let resp = client
            .get(&url)
            .timeout(Duration::from_secs(40))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        let body = resp.text().await.map_err(|e| e.to_string())?;
        let v: Value = serde_json::from_str(&body).map_err(|_| format!("HTTP {status}: {body}"))?;
        if v.get("ok").and_then(|x| x.as_bool()) != Some(true) {
            let desc = v.get("description").and_then(|x| x.as_str()).unwrap_or("unknown");
            return Err(format!("Telegram error (HTTP {status}): {desc}"));
        }
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

    pub async fn send_message(
        client: &reqwest::Client,
        token: &str,
        chat_id: i64,
        text: &str,
    ) -> Result<(), String> {
        let url = format!("https://api.telegram.org/bot{token}/sendMessage");
        client
            .post(&url)
            .header("content-type", "application/json")
            .body(json!({ "chat_id": chat_id, "text": text }).to_string())
            .send()
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[cfg(feature = "cloud")]
fn parse_allow(s: Option<String>) -> Vec<i64> {
    s.map(|s| s.split(',').filter_map(|x| x.trim().parse().ok()).collect())
        .unwrap_or_default()
}

#[cfg(feature = "cloud")]
async fn run_telegram(token: String, key: String, model: String, allow: Vec<i64>) {
    let client = reqwest::Client::new();
    let system = "You are a tiny agent running on an Android phone via Termux. You can read the \
                  ambient light sensor (read_light) and control the phone's flashlight (flashlight). \
                  Be concise and friendly. Use the tools when asked about light or to toggle the light.";
    log("tg", "checking bot token (getMe)...");
    match telegram::get_me(&client, &token).await {
        Ok(name) => log("tg", format!("token OK — bot is @{name}")),
        Err(e) => {
            log("tg", format!("TOKEN/NETWORK ERROR: {e}"));
            log("tg", "fix the token (from @BotFather) and re-run. Stopping.");
            return;
        }
    }
    telegram::delete_webhook(&client, &token).await;
    log("tg", "live — message your bot now. (Ctrl+C to stop.)");
    let mut offset: i64 = 0;
    let mut polls: u64 = 0;
    loop {
        let updates = match telegram::get_updates(&client, &token, offset).await {
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
                let _ = telegram::send_message(&client, &token, u.chat_id, "Not authorized.").await;
                continue;
            }
            let tools: Vec<Box<dyn Tool>> = vec![Box::new(LightSensorTool), Box::new(FlashlightTool)];
            let agent = AgentLoop::new(make_anthropic(key.clone(), model.clone()), tools)
                .with_observer(Box::new(Printer));
            let reply = match agent.run(system, &u.text).await {
                Ok(t) if !t.trim().is_empty() => t,
                Ok(_) => "(done)".to_string(),
                Err(e) => format!("error: {e:?}"),
            };
            log("tg", format!("-> [{}] {}", u.chat_id, reply));
            let _ = telegram::send_message(&client, &token, u.chat_id, &reply).await;
        }
    }
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

    let have_torch = have("termux-torch");
    let have_sensor = have("termux-sensor");
    log("check", format!("termux-torch command:  {have_torch}"));
    log("check", format!("termux-sensor command: {have_sensor}"));
    log("check", "NOTE: the commands existing is NOT enough — the Termux:API *app* must also be");
    log("check", "installed from F-Droid and granted Camera + Sensors permission, or calls hang.");

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

    // Telegram mode: if a bot token is set, chat with the agent instead of the
    // local flashlight cycle. Needs the cloud LLM (and the `cloud` feature).
    #[cfg(feature = "cloud")]
    if let Some((key, model)) = cloud.clone() {
        if let Ok(token) = env::var("TAR_TG_TOKEN") {
            if !token.is_empty() {
                let allow = parse_allow(env::var("TAR_TG_ALLOW").ok());
                if allow.is_empty() {
                    log("tg", "WARNING: TAR_TG_ALLOW not set — anyone who finds your bot can use it.");
                    log("tg", "Message it once; its chat id is logged so you can set TAR_TG_ALLOW=<id>.");
                }
                run_telegram(token, key, model, allow).await;
                return;
            }
        }
    }
    #[cfg(feature = "cloud")]
    if env::var("TAR_TG_TOKEN").is_ok() && cloud.is_none() {
        log("tg", "TAR_TG_TOKEN is set but ANTHROPIC_API_KEY is missing — Telegram mode needs both.");
    }

    let system = format!(
        "You control a phone via two tools: read_light (returns ambient lux) and \
         flashlight (on/off). Turn the flashlight ON only when it is dark — below \
         {dark_lux} lux. Each turn: call read_light, then call flashlight with the \
         correct state, then briefly state the result."
    );

    log("selftest", "blinking flashlight for 0.7s (watch for the light)...");
    let _ = torch(true).await;
    tokio::time::sleep(Duration::from_millis(700)).await;
    let _ = torch(false).await;
    match read_lux().await {
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

    let _ = torch(false).await;
    log("done", "tune with TAR_CYCLES=<n> TAR_DARK_LUX=<lux>");
}
