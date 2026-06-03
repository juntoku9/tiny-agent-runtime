//! Simulated GPIO actuator — an in-memory pin map standing in for real
//! hardware so the node protocol can be exercised on a dev machine. Cloneable
//! so several tools can share the same pin state.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tar_hal::{Actuator, HalError, HalResult};

#[derive(Clone, Default)]
pub struct SimActuator {
    pins: Arc<Mutex<HashMap<u16, bool>>>,
}

impl SimActuator {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Actuator for SimActuator {
    fn set(&self, channel: u16, level: bool) -> HalResult<()> {
        self.pins
            .lock()
            .map_err(|_| HalError::Io("pin lock poisoned".to_string()))?
            .insert(channel, level);
        eprintln!("[node] pin {} -> {}", channel, if level { "HIGH" } else { "LOW" });
        Ok(())
    }

    fn get(&self, channel: u16) -> HalResult<bool> {
        let g = self.pins.lock().map_err(|_| HalError::Io("pin lock poisoned".to_string()))?;
        Ok(*g.get(&channel).unwrap_or(&false))
    }
}
