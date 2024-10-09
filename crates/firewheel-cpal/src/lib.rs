use std::{fmt::Debug, time::Duration};

use cpal::traits::{DeviceTrait, HostTrait};
use firewheel_graph::{
    backend::DeviceInfo,
    graph::{AudioGraph, AudioGraphConfig, CompileGraphError},
    processor::{FwProcessor, FwProcessorStatus},
    ActiveFwCtx, InactiveFwCtx,
};

const BUILD_STREAM_TIMEOUT: Duration = Duration::from_secs(5);
const MSG_CHANNEL_CAPACITY: usize = 4;

pub struct InactiveFwCpalCtx<C> {
    cx: InactiveFwCtx<C>,
}

impl<C: 'static + Send> InactiveFwCpalCtx<C> {
    pub fn new(graph_config: AudioGraphConfig) -> Self {
        Self {
            cx: InactiveFwCtx::new(graph_config),
        }
    }

    pub fn graph(&self) -> &AudioGraph<C> {
        self.cx.graph()
    }

    pub fn graph_mut(&mut self) -> &mut AudioGraph<C> {
        self.cx.graph_mut()
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

    pub fn activate(
        self,
        output_device: Option<&String>,
        fallback: bool,
        user_cx: C,
    ) -> Result<ActiveFwCpalCtx<C>, (InactiveFwCpalCtx<C>, C, ActivateError)> {
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
                    } else if fallback {
                        log::warn!("Could not find requested audio output device: {}. Falling back to default device...", &output_device_name);
                    } else {
                        return Err((
                            self,
                            user_cx,
                            ActivateError::DeviceNotFound(output_device_name.clone()),
                        ));
                    }
                }
                Err(e) => {
                    if fallback {
                        log::error!("Failed to get output audio devices: {}. Falling back to default device...", e);
                    } else {
                        return Err((self, user_cx, e.into()));
                    }
                }
            }
        }

        if device.is_none() {
            let Some(default_device) = host.default_output_device() else {
                if fallback {
                    log::error!("No default audio output device found. Falling back to dummy output device...");
                    // TODO: Use dummy audio backend as fallback.
                    todo!()
                } else {
                    return Err((self, user_cx, ActivateError::DefaultDeviceNotFound));
                }
            };
            device = Some(default_device);
        }
        let device = device.unwrap();

        let config = match device.default_output_config() {
            Ok(c) => c,
            Err(e) => {
                if fallback {
                    log::error!(
                        "Failed to get default config for output audio device: {}. Falling back to dummy output device...",
                        e
                    );
                    // TODO: Use dummy audio backend as fallback.
                    todo!()
                } else {
                    return Err((self, user_cx, e.into()));
                }
            }
        };

        let config = config.config();

        let num_in_channels = 0;
        let num_out_channels = config.channels as usize;

        assert_ne!(num_out_channels, 0);

        let out_device_name = device.name().unwrap_or_else(|_| "unkown".into());

        log::info!(
            "Starting output audio stream with device \"{}\" with configuration {:?}",
            &out_device_name,
            &config
        );

        let (mut to_stream_tx, mut from_ctx_rx) =
            rtrb::RingBuffer::<CtxToStreamMsg<C>>::new(MSG_CHANNEL_CAPACITY);
        let (mut err_to_cx_tx, from_err_rx) =
            rtrb::RingBuffer::<cpal::StreamError>::new(MSG_CHANNEL_CAPACITY);

        let mut processor: Option<FwProcessor<C>> = None;

        let stream = match device.build_output_stream(
            &config,
            move |data: &mut [f32], info: &cpal::OutputCallbackInfo| {
                data_callback(
                    data,
                    num_in_channels,
                    num_out_channels,
                    info,
                    &mut from_ctx_rx,
                    &mut processor,
                )
            },
            move |err| {
                let _ = err_to_cx_tx.push(err);
            },
            Some(BUILD_STREAM_TIMEOUT),
        ) {
            Ok(s) => s,
            Err(e) => {
                if fallback {
                    log::error!("Failed to start output audio stream: {}. Falling back to dummy output device...", e);
                    // TODO: Use dummy audio backend as fallback.
                    todo!()
                } else {
                    return Err((self, user_cx, e.into()));
                }
            }
        };

        let (cx, processor) = self.cx.activate(
            config.sample_rate.0,
            num_in_channels,
            num_out_channels,
            user_cx,
        );

        to_stream_tx
            .push(CtxToStreamMsg::NewProcessor(processor))
            .unwrap();

        Ok(ActiveFwCpalCtx {
            inner: Some(ActiveFwCpalCtxInner {
                cx,
                _to_stream_tx: to_stream_tx,
                from_err_rx,
                out_device_name,
                config,
            }),
            stream: Some(stream),
        })
    }
}

// Implement Debug so `unwrap()` can be used.
impl<C> Debug for InactiveFwCpalCtx<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "InactiveFwCpalCtx")
    }
}

fn data_callback<C>(
    output: &mut [f32],
    num_in_channels: usize,
    num_out_channels: usize,
    // TODO: use provided timestamp in some way
    _info: &cpal::OutputCallbackInfo,
    from_ctx_rx: &mut rtrb::Consumer<CtxToStreamMsg<C>>,
    processor: &mut Option<FwProcessor<C>>,
) {
    while let Ok(msg) = from_ctx_rx.pop() {
        let CtxToStreamMsg::NewProcessor(p) = msg;
        *processor = Some(p);
    }

    let mut drop_processor = false;
    if let Some(processor) = processor {
        let frames = output.len() / num_out_channels;

        match processor.process_interleaved(&[], output, num_in_channels, num_out_channels, frames)
        {
            FwProcessorStatus::Ok => {}
            FwProcessorStatus::DropProcessor => drop_processor = true,
        }
    } else {
        output.fill(0.0);
        return;
    }

    if drop_processor {
        *processor = None;
    }
}

struct ActiveFwCpalCtxInner<C: 'static> {
    pub cx: ActiveFwCtx<C>,
    _to_stream_tx: rtrb::Producer<CtxToStreamMsg<C>>,
    from_err_rx: rtrb::Consumer<cpal::StreamError>,
    out_device_name: String,
    config: cpal::StreamConfig,
}

pub struct ActiveFwCpalCtx<C: 'static> {
    inner: Option<ActiveFwCpalCtxInner<C>>,
    stream: Option<cpal::Stream>,
}

impl<C> ActiveFwCpalCtx<C> {
    pub fn graph(&self) -> &AudioGraph<C> {
        self.inner.as_ref().unwrap().cx.graph()
    }

    pub fn graph_mut(&mut self) -> &mut AudioGraph<C> {
        self.inner.as_mut().unwrap().cx.graph_mut()
    }

    pub fn out_device_name(&self) -> &str {
        &self.inner.as_ref().unwrap().out_device_name
    }

    pub fn stream_config(&self) -> &cpal::StreamConfig {
        &self.inner.as_ref().unwrap().config
    }

    /// Update the firewheel context.
    ///
    /// This must be called reguarly (i.e. once every frame).
    pub fn update(mut self) -> UpdateStatus<C> {
        let inner = self.inner.take().unwrap();
        let stream = self.stream.take();

        let ActiveFwCpalCtxInner {
            cx,
            _to_stream_tx,
            mut from_err_rx,
            out_device_name,
            config,
        } = inner;

        if let Ok(e) = from_err_rx.pop() {
            let (cx, user_cx) = cx.deactivate(false);
            let _ = stream;

            return UpdateStatus::Deactivated {
                cx: InactiveFwCpalCtx { cx },
                user_cx,
                error_msg: Some(e),
            };
        }

        match cx.update() {
            firewheel_graph::context::UpdateStatus::Ok { cx, graph_error } => UpdateStatus::Ok {
                cx: Self {
                    inner: Some(ActiveFwCpalCtxInner {
                        cx,
                        _to_stream_tx,
                        from_err_rx,
                        out_device_name,
                        config,
                    }),
                    stream,
                },
                graph_error,
            },
            firewheel_graph::context::UpdateStatus::Deactivated { cx, user_cx } => {
                let _ = stream;

                UpdateStatus::Deactivated {
                    cx: InactiveFwCpalCtx { cx },
                    user_cx,
                    error_msg: None,
                }
            }
        }
    }

    pub fn deactivate(mut self) -> (InactiveFwCpalCtx<C>, Option<C>) {
        let inner = self.inner.take().unwrap();
        let (cx, user_cx) = inner.cx.deactivate(true);

        let _ = self.stream.take();

        (InactiveFwCpalCtx { cx }, user_cx)
    }
}

impl<C: 'static> Drop for ActiveFwCpalCtx<C> {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.take() {
            inner.cx.deactivate(self.stream.is_some());
        };
    }
}

pub enum UpdateStatus<C: 'static> {
    Ok {
        cx: ActiveFwCpalCtx<C>,
        graph_error: Option<CompileGraphError>,
    },
    Deactivated {
        cx: InactiveFwCpalCtx<C>,
        user_cx: Option<C>,
        error_msg: Option<cpal::StreamError>,
    },
}

enum CtxToStreamMsg<C: 'static> {
    NewProcessor(FwProcessor<C>),
}

/// An error occured while trying to activate an [`InactiveFwCpalCtx`]
#[derive(Debug, thiserror::Error)]
pub enum ActivateError {
    #[error("The requested audio device was not found: {0}")]
    DeviceNotFound(String),
    #[error("Could not get audio devices: {0}")]
    FailedToGetDevices(#[from] cpal::DevicesError),
    #[error("Failed to get default audio output device")]
    DefaultDeviceNotFound,
    #[error("Failed to get audio device config: {0}")]
    FailedToGetConfig(#[from] cpal::DefaultStreamConfigError),
    #[error("Failed to build audio stream: {0}")]
    BuildStreamError(#[from] cpal::BuildStreamError),
}
