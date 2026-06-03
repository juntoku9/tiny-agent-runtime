#![no_std]
//! ESP32 platform crate.
//!
//! Phase 1 adds `esp-idf-svc` and implements the HAL traits (a WiFi/TLS HTTP
//! client, NVS/SPIFFS storage, a GPIO actuator, and OTA). It is kept
//! dependency-free for now so the workspace builds on a host without the
//! Xtensa toolchain installed.
