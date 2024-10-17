use std::{any::Any, fmt::Debug, time::Duration};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use firewheel_core::node::StreamStatus;
use firewheel_graph::{
    backend::DeviceInfo,
    graph::{AudioGraph, AudioGraphConfig},
    processor::{FirewheelProcessor, FirewheelProcessorStatus},
    FirewheelGraphCtx, UpdateStatus,
};

const BUILD_STREAM_TIMEOUT: Duration = Duration::from_secs(5);
const MSG_CHANNEL_CAPACITY: usize = 4;

struct ActiveState {
    _stream: cpal::Stream,
    _to_stream_tx: rtrb::Producer<CtxToStreamMsg>,
    from_err_rx: rtrb::Consumer<cpal::StreamError>,
    out_device_name: String,
    config: cpal::StreamConfig,
}

pub struct FirewheelCpalCtx {
    cx: FirewheelGraphCtx,
    active_state: Option<ActiveState>,
}

impl FirewheelCpalCtx {
    pub fn new(graph_config: AudioGraphConfig) -> Self {
        Self {
            cx: FirewheelGraphCtx::new(graph_config),
            active_state: None,
        }
    }

    pub fn graph(&self) -> &AudioGraph {
        &self.cx.graph
    }

    pub fn graph_mut(&mut self) -> &mut AudioGraph {
        &mut self.cx.graph
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

    /// Activate the context and start the audio stream.
    ///
    /// Returns an error if the context is already active.
    pub fn activate(
        &mut self,
        output_device: Option<&String>,
        fallback: bool,
        user_cx: Option<Box<dyn Any + Send>>,
    ) -> Result<(), (ActivateError, Option<Box<dyn Any + Send>>)> {
        if self.cx.is_activated() {
            return Err((ActivateError::AlreadyActivated, user_cx));
        }

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
                            ActivateError::DeviceNotFound(output_device_name.clone()),
                            user_cx,
                        ));
                    }
                }
                Err(e) => {
                    if fallback {
                        log::error!("Failed to get output audio devices: {}. Falling back to default device...", e);
                    } else {
                        return Err((e.into(), user_cx));
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
                    return Err((ActivateError::DefaultDeviceNotFound, user_cx));
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
                    return Err((e.into(), user_cx));
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

        let max_block_frames = match config.buffer_size {
            cpal::BufferSize::Default => 1024,
            cpal::BufferSize::Fixed(f) => f as usize,
        };

        let (mut to_stream_tx, from_ctx_rx) =
            rtrb::RingBuffer::<CtxToStreamMsg>::new(MSG_CHANNEL_CAPACITY);
        let (mut err_to_cx_tx, from_err_rx) =
            rtrb::RingBuffer::<cpal::StreamError>::new(MSG_CHANNEL_CAPACITY);

        let mut data_callback = DataCallback::new(
            num_in_channels,
            num_out_channels,
            from_ctx_rx,
            config.sample_rate.0,
        );

        let stream = match device.build_output_stream(
            &config,
            move |output: &mut [f32], info: &cpal::OutputCallbackInfo| {
                data_callback.callback(output, info);
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
                    return Err((e.into(), user_cx));
                }
            }
        };

        if let Err(e) = stream.play() {
            return Err((e.into(), user_cx));
        }

        let user_cx = user_cx.unwrap_or(Box::new(()));

        let processor = self
            .cx
            .activate(
                config.sample_rate.0,
                num_in_channels,
                num_out_channels,
                max_block_frames,
                user_cx,
            )
            .unwrap();

        to_stream_tx
            .push(CtxToStreamMsg::NewProcessor(processor))
            .unwrap();

        self.active_state = Some(ActiveState {
            _stream: stream,
            _to_stream_tx: to_stream_tx,
            from_err_rx,
            out_device_name,
            config,
        });

        Ok(())
    }

    /// Returns whether or not this context is currently activated.
    pub fn is_activated(&self) -> bool {
        self.cx.is_activated()
    }

    /// Get the name of the audio output device.
    ///
    /// Returns `None` if the context is not currently activated.
    pub fn out_device_name(&self) -> Option<&str> {
        self.active_state
            .as_ref()
            .map(|s| s.out_device_name.as_str())
    }

    /// Get the current configuration of the audio stream.
    ///
    /// Returns `None` if the context is not currently activated.
    pub fn stream_config(&self) -> Option<&cpal::StreamConfig> {
        self.active_state.as_ref().map(|s| &s.config)
    }

    /// Update the firewheel context.
    ///
    /// This must be called reguarly once the context has been activated
    /// (i.e. once every frame).
    pub fn update(&mut self) -> UpdateStatus {
        if let Some(state) = &mut self.active_state {
            if let Ok(e) = state.from_err_rx.pop() {
                let user_cx = self.cx.deactivate(false);
                self.active_state = None;

                return UpdateStatus::Deactivated {
                    error: Some(Box::new(e)),
                    returned_user_cx: user_cx,
                };
            }
        }

        match self.cx.update() {
            UpdateStatus::Active { graph_error } => UpdateStatus::Active { graph_error },
            UpdateStatus::Inactive => UpdateStatus::Inactive,
            UpdateStatus::Deactivated {
                returned_user_cx,
                error,
            } => {
                if self.active_state.is_some() {
                    self.active_state = None;
                }

                UpdateStatus::Deactivated {
                    error,
                    returned_user_cx,
                }
            }
        }
    }

    /// Deactivate the firewheel context and stop the audio stream.
    ///
    /// This will block the thread until either the processor has
    /// been successfully dropped or a timeout has been reached.
    ///
    /// If the stream is still currently running, then the context
    /// will attempt to cleanly deactivate the processor. If not,
    /// then the context will wait for either the processor to be
    /// dropped or a timeout being reached.
    ///
    /// If the context is already deactivated, then this will do
    /// nothing and return `None`.
    pub fn deactivate(&mut self) -> Option<Box<dyn Any + Send>> {
        if self.cx.is_activated() {
            let user_cx = self.cx.deactivate(self.active_state.is_some());
            self.active_state = None;
            user_cx
        } else {
            None
        }
    }
}

// Implement Debug so `unwrap()` can be used.
impl Debug for FirewheelCpalCtx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FirewheelCpalCtx")
    }
}

struct DataCallback {
    num_in_channels: usize,
    num_out_channels: usize,
    from_ctx_rx: rtrb::Consumer<CtxToStreamMsg>,
    processor: Option<FirewheelProcessor>,
    sample_rate_recip: f64,
    first_stream_instant: Option<cpal::StreamInstant>,
    predicted_stream_secs: f64,
    is_first_callback: bool,
}

impl DataCallback {
    fn new(
        num_in_channels: usize,
        num_out_channels: usize,
        from_ctx_rx: rtrb::Consumer<CtxToStreamMsg>,
        sample_rate: u32,
    ) -> Self {
        Self {
            num_in_channels,
            num_out_channels,
            from_ctx_rx,
            processor: None,
            sample_rate_recip: f64::from(sample_rate).recip(),
            first_stream_instant: None,
            predicted_stream_secs: 1.0,
            is_first_callback: true,
        }
    }

    fn callback(&mut self, output: &mut [f32], info: &cpal::OutputCallbackInfo) {
        while let Ok(msg) = self.from_ctx_rx.pop() {
            let CtxToStreamMsg::NewProcessor(p) = msg;
            self.processor = Some(p);
        }

        let frames = output.len() / self.num_out_channels;

        let (stream_time_secs, underflow) = if self.is_first_callback {
            // Apparently there is a bug in CPAL where the callback instant in
            // the first callback can be greater than in the second callback.
            //
            // Work around this by ignoring the first callback instant.
            self.is_first_callback = false;
            self.predicted_stream_secs = frames as f64 * self.sample_rate_recip;
            (0.0, false)
        } else if let Some(instant) = &self.first_stream_instant {
            let stream_time_secs = info
                .timestamp()
                .callback
                .duration_since(instant)
                .unwrap()
                .as_secs_f64();

            // If the stream time is significantly greater than the predicted stream
            // time, it means an underflow has occurred.
            let underrun = stream_time_secs > self.predicted_stream_secs;

            // Calculate the next predicted stream time to detect underflows.
            //
            // Add a little bit of wiggle room to account for tiny clock
            // innacuracies and rounding errors.
            self.predicted_stream_secs =
                stream_time_secs + (frames as f64 * self.sample_rate_recip * 1.2);

            (stream_time_secs, underrun)
        } else {
            self.first_stream_instant = Some(info.timestamp().callback);
            let stream_time_secs = self.predicted_stream_secs;
            self.predicted_stream_secs += frames as f64 * self.sample_rate_recip * 1.2;
            (stream_time_secs, false)
        };

        let mut drop_processor = false;
        if let Some(processor) = &mut self.processor {
            let mut stream_status = StreamStatus::empty();

            if underflow {
                stream_status.insert(StreamStatus::OUTPUT_UNDERFLOW);
            }

            match processor.process_interleaved(
                &[],
                output,
                self.num_in_channels,
                self.num_out_channels,
                frames,
                stream_time_secs,
                stream_status,
            ) {
                FirewheelProcessorStatus::Ok => {}
                FirewheelProcessorStatus::DropProcessor => drop_processor = true,
            }
        } else {
            output.fill(0.0);
            return;
        }

        if drop_processor {
            self.processor = None;
        }
    }
}

impl Drop for FirewheelCpalCtx {
    fn drop(&mut self) {
        if self.cx.is_activated() {
            self.cx.deactivate(self.active_state.is_some());
        }
    }
}
enum CtxToStreamMsg {
    NewProcessor(FirewheelProcessor),
}

/// An error occured while trying to activate an [`InactiveFwCpalCtx`]
#[derive(Debug, thiserror::Error)]
pub enum ActivateError {
    #[error("The firewheel context is already activated")]
    AlreadyActivated,
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
    #[error("Failed to play audio stream: {0}")]
    PlayStreamError(#[from] cpal::PlayStreamError),
}
