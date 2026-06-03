#![no_std]
//! Hardware abstraction layer (HAL) for the Tiny Agent Runtime.
//!
//! These traits are the portability seam: `tar-core` depends only on them,
//! and each platform crate provides the implementations. I/O flows inward
//! through these traits and nowhere else, which is what lets the identical
//! agent sources run on an MCU and on a Linux camera SoC.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::future::Future;

/// Errors surfaced by HAL implementations.
#[derive(Debug, Clone)]
pub enum HalError {
    /// The resource (file, pin, device) was not found.
    NotFound,
    /// The operation is not permitted (policy, range, or hardware).
    Denied,
    /// An I/O or transport failure, with a human-readable detail.
    Io(String),
}

pub type HalResult<T> = Result<T, HalError>;

/// Persistent key/path storage (flash files on MCU, filesystem on Linux).
pub trait Storage {
    fn read(&self, path: &str) -> HalResult<Vec<u8>>;
    fn write(&self, path: &str, data: &[u8]) -> HalResult<()>;
    fn list(&self, prefix: &str) -> HalResult<Vec<String>>;
}

/// Wall-clock time source.
pub trait Clock {
    /// Unix time in seconds.
    fn now_unix(&self) -> u64;
}

/// A single HTTP response.
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Outbound HTTPS client — the only outbound path, used to reach the cloud LLM.
pub trait HttpClient {
    fn post(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> impl Future<Output = HalResult<HttpResponse>>;
}

/// Bidirectional frame transport for the node protocol (TCP/WebSocket on LAN).
pub trait Transport {
    fn send(&self, frame: &[u8]) -> impl Future<Output = HalResult<()>>;
    fn recv(&self) -> impl Future<Output = HalResult<Vec<u8>>>;
}

/// Digital actuator / sensor channel (a GPIO pin or equivalent).
pub trait Actuator {
    fn set(&self, channel: u16, level: bool) -> HalResult<()>;
    fn get(&self, channel: u16) -> HalResult<bool>;
}

/// Still-image capture (an RTSP/V4L2 snapshot on Linux).
pub trait Camera {
    /// Capture one frame, returning encoded image bytes (e.g. JPEG).
    fn snapshot(&self) -> impl Future<Output = HalResult<Vec<u8>>>;
}
