//! Brain (Phase 2): connect to a tool-node, discover its capabilities, build
//! remote tools (same `Tool` trait as local ones), then toggle and read back a
//! pin on the remote node — proving the LAN node protocol end to end.

use std::process::exit;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tar_core::node::{handshake, RemoteTool};
use tar_core::Tool;
use tar_linux::transport::TcpTransport;
use tar_proto::ToolInput;

async fn connect_with_retry(addr: &str) -> TcpTransport {
    for _ in 0..30 {
        if let Ok(t) = TcpTransport::connect(addr).await {
            return t;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    eprintln!("could not connect to node at {}", addr);
    exit(1);
}

#[tokio::main]
async fn main() {
    let addr = std::env::var("TAR_NODE_ADDR").unwrap_or_else(|_| "127.0.0.1:18810".to_string());
    let transport = Arc::new(connect_with_retry(&addr).await);

    // Dynamic capability discovery: the brain learns the tools at runtime.
    let caps = match handshake(&transport).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("handshake failed: {:?}", e);
            exit(1);
        }
    };
    println!("discovered {} tool(s) on node:", caps.len());
    for c in &caps {
        println!("  - {}: {}", c.name, c.description);
    }

    // Wrap each capability as a remote tool — identical to a local tool to the
    // rest of the system. One shared TCP connection via Arc.
    let tools: Vec<Box<dyn Tool>> = caps
        .iter()
        .map(|m| Box::new(RemoteTool::new(m.clone(), transport.clone())) as Box<dyn Tool>)
        .collect();

    // Turn the relay on pin 5 HIGH on the remote node.
    let writer = tools
        .iter()
        .find(|t| t.manifest().name == "gpio_write")
        .expect("node should advertise gpio_write");
    let wr = writer.call(ToolInput { args: json!({ "channel": 5, "level": true }) }).await;
    println!("gpio_write(pin 5, HIGH) -> ok={} {:?}", wr.ok, wr.content);

    // Read it back to confirm.
    let reader = tools
        .iter()
        .find(|t| t.manifest().name == "gpio_read")
        .expect("node should advertise gpio_read");
    let rd = reader.call(ToolInput { args: json!({ "channel": 5 }) }).await;
    println!("gpio_read(pin 5)        -> ok={} {:?}", rd.ok, rd.content);

    if rd.ok && rd.content == "1" {
        println!("VERIFIED: pin 5 reads HIGH on the remote node.");
    } else {
        eprintln!("FAILED: expected pin 5 HIGH, got {:?}", rd.content);
        exit(1);
    }
}
