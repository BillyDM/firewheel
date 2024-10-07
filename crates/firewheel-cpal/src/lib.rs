use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait};

use firewheel::{backend::DeviceInfo, InactiveFwCtx};

const BUILD_STREAM_TIMEOUT: Duration = Duration::from_secs(5);

pub struct InactiveFwCpalCtx {
    pub cx: InactiveFwCtx,
}

impl InactiveFwCpalCtx {
    pub fn new(config: firewheel::Config) -> Self {
        Self {
            cx: InactiveFwCtx::new(config),
        }
    }

    pub fn available_output_devices(&self) -> Vec<DeviceInfo> {
        let mut devices = Vec::with_capacity(16);

        let host = cpal::default_host();

        let default_device_name = if let Some(default_device) = host.default_output_device() {
            match default_device.name() {
                Ok(n) => Some(n),
                Err(e) => {
                    log::warn!("Failed to get name of default audio output device: {}", e);
                    None
                }
            }
        } else {
            None
        };

        match host.output_devices() {
            Ok(output_devices) => {
                for device in output_devices {
                    let Ok(name) = device.name() else {
                        continue;
                    };

                    let is_default = if let Some(default_device_name) = &default_device_name {
                        &name == default_device_name
                    } else {
                        false
                    };

                    let default_out_config = match device.default_output_config() {
                        Ok(c) => c,
                        Err(e) => {
                            if is_default {
                                log::warn!("Failed to get default config for the default audio output device: {}", e);
                            }
                            continue;
                        }
                    };

                    devices.push(DeviceInfo {
                        name,
                        num_channels: default_out_config.channels(),
                        is_default,
                    })
                }
            }
            Err(e) => {
                log::error!("Failed to get output audio devices: {}", e);
            }
        }

        devices
    }

    pub fn activate(&mut self, output_device: Option<&String>, fallback: bool) -> Result<(), ()> {
        let host = cpal::default_host();

        let mut device = None;
        if let Some(output_device_name) = output_device {
            match host.output_devices() {
                Ok(mut output_devices) => {
                    if let Some(d) = output_devices.find(|d| {
                        if let Ok(name) = d.name() {
                            &name == output_device_name
                        } else {
                            false
                        }
                    }) {
                        device = Some(d);
                    } else if !fallback {
                        return Err(());
                    }
                }
                Err(e) => {
                    log::error!("Failed to get output audio devices: {}", e);

                    if !fallback {
                        return Err(());
                    }
                }
            }
        }

        if device.is_none() {
            let Some(default_device) = host.default_output_device() else {
                log::error!("No default audio output device found.");

                if fallback {
                    // TODO: Use dummy audio backend as fallback.
                    todo!()
                } else {
                    return Err(());
                }
            };
            device = Some(default_device);
        }
        let device = device.unwrap();

        let config = match device.default_output_config() {
            Ok(c) => c,
            Err(e) => {
                log::error!(
                    "Failed to get default config for output audio device: {}",
                    e
                );

                if fallback {
                    // TODO: Use dummy audio backend as fallback.
                    todo!()
                } else {
                    return Err(());
                }
            }
        };

        let config = config.config();

        log::info!(
            "Starting output audio stream with device {} with configuration {:?}",
            &device.name().unwrap_or_else(|_| "unkown".into()),
            &config
        );

        let stream = match device.build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| todo!(),
            move |err| todo!(),
            Some(BUILD_STREAM_TIMEOUT),
        ) {
            Ok(s) => s,
            Err(e) => {
                log::error!("Failed to start output audio stream: {}", e);

                if fallback {
                    // TODO: Use dummy audio backend as fallback.
                    todo!()
                } else {
                    return Err(());
                }
            }
        };

        todo!()
    }
}
