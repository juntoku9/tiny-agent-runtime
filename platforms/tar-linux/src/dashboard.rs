//! Tiny zero-dependency HTTP dashboard server (tokio only). Serves a fixed HTML
//! page and a `/state` JSON endpoint produced by a caller-supplied closure, so
//! both the node (pin states) and the agent (loop timeline) reuse it.

use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Serve `html` at `/` and `state()` (JSON) at `/state`, forever.
pub async fn serve<F>(addr: String, html: &'static str, state: F)
where
    F: Fn() -> String + Send + Sync + 'static,
{
    let state = Arc::new(state);
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
        let state = state.clone();
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
                ("application/json", state())
            } else {
                ("text/html; charset=utf-8", html.to_string())
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{}",
                ctype,
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes()).await;
        });
    }
}
