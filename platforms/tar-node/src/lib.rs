#![no_std]
//! Minimal tool-node firmware crate — the low-cost controller chip.
//!
//! A tool-node is a tool registry whose transport is the LAN socket: it
//! advertises its capabilities on connect and serves `tool_call` frames.
//! Phase 2 builds this on the ESP32 platform; kept dependency-free until then.
