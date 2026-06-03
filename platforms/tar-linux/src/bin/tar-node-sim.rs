//! Tool-node simulator (Phase 2): serves GPIO tools over the LAN node protocol,
//! standing in for a cheap MCU controller chip. Also serves a live web
//! dashboard of pin states so you can watch the agent drive the hardware.

use tar_core::node::NodeServer;
use tar_core::Tool;
use tar_linux::sim::SimActuator;
use tar_linux::transport::TcpTransport;
use tar_tools::gpio::{GpioReadTool, GpioTool};

#[tokio::main]
async fn main() {
    let addr = std::env::var("TAR_NODE_ADDR").unwrap_or_else(|_| "127.0.0.1:18810".to_string());
    let dash_addr = std::env::var("TAR_DASH_ADDR").unwrap_or_else(|_| "127.0.0.1:8090".to_string());

    // Shared pin state: the tools mutate it, the dashboard reads it.
    let sim = SimActuator::new();

    {
        let sim = sim.clone();
        tokio::spawn(async move { tar_linux::dashboard::serve(dash_addr, sim).await });
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
