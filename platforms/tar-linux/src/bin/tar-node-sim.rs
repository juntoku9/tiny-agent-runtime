//! Tool-node simulator (Phase 2): serves GPIO tools over the LAN node protocol,
//! standing in for a cheap MCU controller chip. Also serves a live web
//! dashboard of pin states so you can watch the agent drive the hardware.

use tar_core::node::NodeServer;
use tar_core::Tool;
use tar_linux::sim::SimActuator;
use tar_linux::transport::TcpTransport;
use tar_tools::gpio::{GpioReadTool, GpioTool};

const NODE_HTML: &str = r##"<!doctype html><html><head><meta charset="utf-8">
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
  if(!keys.length){p.innerHTML='<div style="color:#6b7280">no pins touched yet — run the agent</div>';}
  for(const k of keys){const on=s.pins[k];const d=document.createElement('div');
   d.className='pin'+(on?' on':'');
   d.innerHTML='<div class="dot"></div><div class="lbl">pin '+k+'</div><div class="st">'+(on?'HIGH':'LOW')+'</div>';
   p.appendChild(d);}
  document.getElementById('log').innerHTML=s.events.slice().reverse().map(e=>'<li>'+e+'</li>').join('');
 }catch(e){}
}
setInterval(tick,300);tick();
</script></body></html>"##;

#[tokio::main]
async fn main() {
    let addr = std::env::var("TAR_NODE_ADDR").unwrap_or_else(|_| "127.0.0.1:18810".to_string());
    let dash_addr = std::env::var("TAR_DASH_ADDR").unwrap_or_else(|_| "127.0.0.1:8090".to_string());

    // Shared pin state: the tools mutate it, the dashboard reads it.
    let sim = SimActuator::new();

    {
        let sim = sim.clone();
        tokio::spawn(async move { tar_linux::dashboard::serve(dash_addr, NODE_HTML, move || sim.state_json()).await });
    }

    let listener = match TcpTransport::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[node] bind {} failed: {}", addr, e);
            std::process::exit(1);
        }
    };
    eprintln!("[node] node-a listening on {}", addr);

    // Accept brains one at a time; stay up across runs so the dashboard persists.
    loop {
        let transport = match TcpTransport::accept(&listener).await {
            Ok(t) => t,
            Err(e) => {
                eprintln!("[node] accept error: {}", e);
                continue;
            }
        };
        eprintln!("[node] brain connected");

        let tools: Vec<Box<dyn Tool>> = vec![
            Box::new(GpioTool::new(sim.clone())),
            Box::new(GpioReadTool::new(sim.clone())),
        ];
        let server = NodeServer::new("node-a", transport, tools);
        if let Err(e) = server.serve().await {
            eprintln!("[node] brain disconnected ({:?}); waiting for next", e);
        }
    }
}
