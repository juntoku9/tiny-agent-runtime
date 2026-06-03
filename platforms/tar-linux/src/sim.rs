//! Simulated GPIO actuator — an in-memory pin map standing in for real
//! hardware so the node protocol can be exercised on a dev machine. Cloneable
//! so several tools (and the dashboard) can share the same pin state, and it
//! records a small event log for visualization.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tar_hal::{Actuator, HalError, HalResult};

#[derive(Clone, Default)]
pub struct SimActuator {
    pins: Arc<Mutex<HashMap<u16, bool>>>,
    log: Arc<Mutex<Vec<String>>>,
}

impl SimActuator {
    pub fn new() -> Self {
        Self::default()
    }

    fn record(&self, msg: String) {
        if let Ok(mut l) = self.log.lock() {
            l.push(msg);
            let len = l.len();
            if len > 50 {
                l.drain(0..len - 50);
            }
        }
    }

    /// Snapshot of pin states and recent events as JSON, for the dashboard.
    pub fn state_json(&self) -> String {
        let pins = self.pins.lock().map(|g| g.clone()).unwrap_or_default();
        let events = self.log.lock().map(|g| g.clone()).unwrap_or_default();
        let mut pin_map = serde_json::Map::new();
        for (k, v) in pins.iter() {
            pin_map.insert(k.to_string(), serde_json::Value::Bool(*v));
        }
        serde_json::json!({
            "node": "node-a",
            "pins": serde_json::Value::Object(pin_map),
            "events": events,
        })
        .to_string()
    }
}

impl Actuator for SimActuator {
    fn set(&self, channel: u16, level: bool) -> HalResult<()> {
        self.pins
            .lock()
            .map_err(|_| HalError::Io("pin lock poisoned".to_string()))?
            .insert(channel, level);
        let msg = format!("pin {} -> {}", channel, if level { "HIGH" } else { "LOW" });
        eprintln!("[node] {}", msg);
        self.record(msg);
        Ok(())
    }

    fn get(&self, channel: u16) -> HalResult<bool> {
        let level = *self
            .pins
            .lock()
            .map_err(|_| HalError::Io("pin lock poisoned".to_string()))?
            .get(&channel)
            .unwrap_or(&false);
        self.record(format!("read pin {} = {}", channel, if level { "HIGH" } else { "LOW" }));
        Ok(level)
    }
}
