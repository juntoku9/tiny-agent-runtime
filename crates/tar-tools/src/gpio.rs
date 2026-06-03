//! GPIO tool — exposes a digital actuator/sensor channel as an agent tool.
//! This is the canonical "IoT chip as a controller tool" implementation: it
//! implements `Tool` over the `Actuator` HAL, and works identically whether
//! the actuator is local or a remote tool-node behind a transport.

use alloc::boxed::Box;
use alloc::string::ToString;
use core::future::Future;
use core::pin::Pin;

use serde_json::json;
use tar_core::Tool;
use tar_hal::Actuator;
use tar_proto::{ToolInput, ToolManifest, ToolResult};

/// Sets a digital output channel high or low via the `Actuator` HAL.
pub struct GpioTool<A: Actuator> {
    actuator: A,
}

impl<A: Actuator> GpioTool<A> {
    pub fn new(actuator: A) -> Self {
        Self { actuator }
    }
}

impl<A: Actuator> Tool for GpioTool<A> {
    fn manifest(&self) -> ToolManifest {
        ToolManifest {
            name: "gpio_write".to_string(),
            description: "Set a digital output channel high or low.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "channel": { "type": "integer" },
                    "level": { "type": "boolean" }
                },
                "required": ["channel", "level"]
            }),
        }
    }

    fn call<'a>(
        &'a self,
        input: ToolInput,
    ) -> Pin<Box<dyn Future<Output = ToolResult> + 'a>> {
        Box::pin(async move {
            let channel = input.args.get("channel").and_then(|v| v.as_u64());
            let level = input.args.get("level").and_then(|v| v.as_bool());
            match (channel, level) {
                (Some(ch), Some(lv)) => match self.actuator.set(ch as u16, lv) {
                    Ok(()) => ToolResult { ok: true, content: "set".to_string() },
                    Err(_) => ToolResult { ok: false, content: "actuator error".to_string() },
                },
                _ => ToolResult { ok: false, content: "invalid input".to_string() },
            }
        })
    }
}
