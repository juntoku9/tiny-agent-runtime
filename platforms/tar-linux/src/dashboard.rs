//! Tiny zero-dependency HTTP dashboard for visualizing a tool-node's pin states
//! and event log live in a browser. Serves a single page that polls `/state`.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::sim::SimActuator;

const INDEX_HTML: &str = r##"<!doctype html><html><head><meta charset="utf-8">
<title>Tiny Agent Runtime — node</title>
<style>
 body{font-family:system-ui,sans-serif;background:#0b0e14;color:#cdd6f4;margin:24px}
 h1{font-size:18px;margin:0 0 4px} .sub{color:#6b7280;font-size:12px;margin-bottom:18px}
 .pins{display:flex;flex-wrap:wrap;gap:14px;margin:8px 0 22px}
 .pin{width:88px;height:92px;border-radius:14px;display:flex;flex-direction:column;
   align-items:center;justify-content:center;background:#1b1f2a;border:1px solid #2a2f3a}
 .dot{width:36px;height:36px;border-radius:50%;background:#39414f;transition:.15s}
 .on .dot{background:#a6e3a1;box-shadow:0 0 18px #a6e3a1}
 .lbl{margin-top:8px;font-size:12px;color:#9aa5b1} .st{font-size:11px;color:#6b7280}
 h2{font-size:13px;color:#9aa5b1;margin:0 0 8px}
 ul{list-style:none;padding:12px 16px;margin:0;font-family:ui-monospace,monospace;font-size:12px;
   line-height:1.6;max-height:260px;overflow:auto;background:#11151c;border-radius:10px}
</style></head><body>
<h1>Tiny Agent Runtime</h1><div class="sub" id="sub">node dashboard</div>
<div class="pins" id="pins"></div>
<h2>Events</h2><ul id="log"></ul>
<script>
async function tick(){
 try{
  const s=await (await fetch('/state')).json();
  document.getElementById('sub').textContent='node: '+s.node+'  ·  live (polling /state)';
  const p=document.getElementById('pins');p.innerHTML='';
  const keys=Object.keys(s.pins).sort((a,b)=>a-b);
  if(!keys.length){p.innerHTML='<div style="color:#6b7280">no pins touched yet — run the brain</div>';}
  for(const k of keys){const on=s.pins[k];const d=document.createElement('div');
   d.className='pin'+(on?' on':'');
   d.innerHTML='<div class="dot"></div><div class="lbl">pin '+k+'</div><div class="st">'+(on?'HIGH':'LOW')+'</div>';
   p.appendChild(d);}
  document.getElementById('log').innerHTML=s.events.slice().reverse().map(e=>'<li>'+e+'</li>').join('');
 }catch(e){}
}
setInterval(tick,300);tick();
</script></body></html>"##;

/// Serve the dashboard forever, reflecting the given actuator's live state.
pub async fn serve(addr: String, sim: SimActuator) {
    let listener = match TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[dash] bind {} failed: {}", addr, e);
            return;
        }
    };
    eprintln!("[dash] dashboard on http://{}", addr);
    loop {
        let (mut stream, _) = match listener.accept().await {
            Ok(s) => s,
            Err(_) => continue,
        };
        let sim = sim.clone();
        tokio::spawn(async move {
            let mut buf = [0u8; 2048];
            let n = match stream.read(&mut buf).await {
                Ok(n) => n,
                Err(_) => return,
            };
            let req = String::from_utf8_lossy(&buf[..n]);
            let path = req
                .lines()
                .next()
                .and_then(|l| l.split_whitespace().nth(1))
                .unwrap_or("/");
            let (ctype, body) = if path.starts_with("/state") {
                ("application/json", sim.state_json())
            } else {
                ("text/html; charset=utf-8", INDEX_HTML.to_string())
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                ctype,
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes()).await;
        });
    }
}
