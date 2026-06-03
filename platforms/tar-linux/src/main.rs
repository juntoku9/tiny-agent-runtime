//! Linux edge-SoC platform — the camera "brain".
//!
//! Phase 1 implements the HAL traits here (a Tokio-based HTTP client,
//! filesystem storage, an RTSP/V4L2 camera, and a TCP transport) and wires the
//! agent loop to the cloud LLM. The skeleton confirms the workspace links.

fn main() {
    println!(
        "tar-linux {} — platform skeleton (HAL impls land in Phase 1)",
        env!("CARGO_PKG_VERSION")
    );
}
