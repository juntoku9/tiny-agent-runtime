#![no_std]
//! Built-in tools. Each is behind a feature flag so a constrained MCU build
//! compiles only what fits.

extern crate alloc;

#[cfg(feature = "gpio")]
pub mod gpio;
