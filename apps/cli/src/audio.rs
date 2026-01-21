//! Audio device listing for CLI

use cpal::traits::{DeviceTrait, HostTrait};

/// Information about an audio device
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub name: String,
    pub is_default: bool,
}

/// List available input devices
pub fn list_input_devices() -> Vec<DeviceInfo> {
    let host = cpal::default_host();
    let mut devices = Vec::new();

    if let Ok(input_devices) = host.input_devices() {
        for device in input_devices {
            if let Ok(name) = device.name() {
                let is_default = host
                    .default_input_device()
                    .and_then(|d| d.name().ok())
                    .map(|n| n == name)
                    .unwrap_or(false);

                devices.push(DeviceInfo { name, is_default });
            }
        }
    }

    devices
}

/// Check if any input device is available
pub fn has_input_device() -> bool {
    let host = cpal::default_host();
    host.default_input_device().is_some()
}
