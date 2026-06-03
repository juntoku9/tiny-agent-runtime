//! Tool-node simulator (Phase 2): serves GPIO tools over the LAN node protocol,
//! standing in for a cheap MCU controller chip. Exposes a simulated actuator so
//! the brain can toggle and read "pins" on a separate process.

use tar_core::node::NodeServer;
use tar_core::Tool;
use tar_linux::sim::SimActuator;
use tar_linux::transport::TcpTransport;
use tar_tools::gpio::{GpioReadTool, GpioTool};

#[tokio::main]
async fn main() {
    let addr = std::env::var("TAR_NODE_ADDR").unwrap_or_else(|_| "127.0.0.1:18810".to_string());
    eprintln!("[node] node-a listening on {}", addr);

    let transport = match TcpTransport::accept_one(&addr).await {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[node] listen error: {}", e);
            std::process::exit(1);
        }
    };
    eprintln!("[node] brain connected");

    let sim = SimActuator::new();
    let tools: Vec<Box<dyn Tool>> = vec![
        Box::new(GpioTool::new(sim.clone())),
        Box::new(GpioReadTool::new(sim.clone())),
    ];

    let server = NodeServer::new("node-a", transport, tools);
    if let Err(e) = server.serve().await {
        eprintln!("[node] serve ended: {:?}", e);
    }
}
