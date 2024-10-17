use std::{
    any::Any,
    error::Error,
    time::{Duration, Instant},
};

use rtrb::PushError;

use crate::{
    graph::{AudioGraph, AudioGraphConfig, CompileGraphError},
    processor::{ContextToProcessorMsg, FirewheelProcessor, ProcessorToContextMsg},
};

const CHANNEL_CAPACITY: usize = 16;
const CLOSE_STREAM_TIMEOUT: Duration = Duration::from_secs(3);
const CLOSE_STREAM_SLEEP_INTERVAL: Duration = Duration::from_millis(2);

struct ActiveState {
    // TODO: Do research on whether `rtrb` is compatible with
    // webassembly. If not, use conditional compilation to
    // use a different channel type when targeting webassembly.
    to_executor_tx: rtrb::Producer<ContextToProcessorMsg>,
    from_executor_rx: rtrb::Consumer<ProcessorToContextMsg>,

    sample_rate: u32,
    max_block_frames: usize,
}

pub struct FirewheelGraphCtx {
    pub graph: AudioGraph,

    active_state: Option<ActiveState>,
}

impl FirewheelGraphCtx {
    pub fn new(graph_config: AudioGraphConfig) -> Self {
        Self {
            graph: AudioGraph::new(&graph_config),
            active_state: None,
        }
    }

    /// Activate the context and return the processor to send to the audio thread.
    ///
    /// Returns `None` if the context is already active.
    pub fn activate(
        &mut self,
        sample_rate: u32,
        num_stream_in_channels: usize,
        num_stream_out_channels: usize,
        max_block_frames: usize,
        user_cx: Box<dyn Any + Send>,
    ) -> Option<FirewheelProcessor> {
        assert_ne!(sample_rate, 0);
        assert!(max_block_frames > 0);

        if self.active_state.is_some() {
            return None;
        }

        let (to_executor_tx, from_graph_rx) =
            rtrb::RingBuffer::<ContextToProcessorMsg>::new(CHANNEL_CAPACITY);
        let (to_graph_tx, from_executor_rx) =
            rtrb::RingBuffer::<ProcessorToContextMsg>::new(CHANNEL_CAPACITY);

        self.active_state = Some(ActiveState {
            to_executor_tx,
            from_executor_rx,
            sample_rate,
            max_block_frames,
        });

        Some(FirewheelProcessor::new(
            from_graph_rx,
            to_graph_tx,
            self.graph.current_node_capacity(),
            num_stream_in_channels,
            num_stream_out_channels,
            max_block_frames,
            user_cx,
        ))
    }

    /// Returns whether or not this context is currently activated.
    pub fn is_activated(&self) -> bool {
        self.active_state.is_some()
    }

    /// Update the firewheel context.
    ///
    /// This must be called reguarly once the context has been activated
    /// (i.e. once every frame).
    pub fn update(&mut self) -> UpdateStatus {
        if self.active_state.is_none() {
            return UpdateStatus::Inactive;
        }

        let mut dropped = false;
        let mut dropped_user_cx = None;

        self.update_internal(&mut dropped, &mut dropped_user_cx);

        if dropped {
            self.graph.deactivate();
            self.active_state = None;
            return UpdateStatus::Deactivated {
                returned_user_cx: dropped_user_cx,
                error: None,
            };
        }

        let Some(state) = &mut self.active_state else {
            return UpdateStatus::Inactive;
        };

        if self.graph.needs_compile() {
            match self
                .graph
                .compile(state.sample_rate, state.max_block_frames)
            {
                Ok(schedule_data) => {
                    if let Err(e) = state
                        .to_executor_tx
                        .push(ContextToProcessorMsg::NewSchedule(Box::new(schedule_data)))
                    {
                        let PushError::Full(msg) = e;

                        log::error!(
                            "Failed to send new schedule: Firewheel message channel is full"
                        );

                        if let ContextToProcessorMsg::NewSchedule(schedule_data) = msg {
                            self.graph.on_schedule_returned(schedule_data);
                        }
                    }
                }
                Err(e) => {
                    return UpdateStatus::Active {
                        graph_error: Some(e),
                    };
                }
            }
        }

        UpdateStatus::Active { graph_error: None }
    }

    /// Deactivate the firewheel context.
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
    pub fn deactivate(&mut self, stream_is_running: bool) -> Option<Box<dyn Any + Send>> {
        let Some(state) = &mut self.active_state else {
            return None;
        };

        let start = Instant::now();

        let mut dropped = false;
        let mut dropped_user_cx = None;

        if stream_is_running {
            loop {
                if let Err(_) = state.to_executor_tx.push(ContextToProcessorMsg::Stop) {
                    log::error!("Failed to send stop signal: Firewheel message channel is full");

                    // TODO: I don't think sleep is supported in WASM, so we will
                    // need to figure out something if that's the case.
                    std::thread::sleep(CLOSE_STREAM_SLEEP_INTERVAL);

                    if start.elapsed() > CLOSE_STREAM_TIMEOUT {
                        log::error!("Timed out trying to send stop signal to firewheel processor");
                        dropped = true;
                        break;
                    }
                } else {
                    break;
                }
            }
        }

        while !dropped {
            self.update_internal(&mut dropped, &mut dropped_user_cx);

            if !dropped {
                // TODO: I don't think sleep is supported in WASM, so we will
                // need to figure out something if that's the case.
                std::thread::sleep(CLOSE_STREAM_SLEEP_INTERVAL);

                if start.elapsed() > CLOSE_STREAM_TIMEOUT {
                    log::error!("Timed out waiting for firewheel processor to drop");
                    dropped = true;
                }
            }
        }

        self.graph.deactivate();
        self.active_state = None;

        dropped_user_cx
    }

    fn update_internal(
        &mut self,
        dropped: &mut bool,
        dropped_user_cx: &mut Option<Box<dyn Any + Send>>,
    ) {
        let Some(state) = &mut self.active_state else {
            return;
        };

        while let Ok(msg) = state.from_executor_rx.pop() {
            match msg {
                ProcessorToContextMsg::ReturnSchedule(schedule_data) => {
                    self.graph.on_schedule_returned(schedule_data);
                }
                ProcessorToContextMsg::Dropped { nodes, user_cx, .. } => {
                    self.graph.on_processor_dropped(nodes);
                    *dropped = true;
                    *dropped_user_cx = user_cx;
                }
            }
        }
    }
}

impl Drop for FirewheelGraphCtx {
    fn drop(&mut self) {
        if self.is_activated() {
            self.deactivate(true);
        }
    }
}

pub enum UpdateStatus {
    Inactive,
    Active {
        graph_error: Option<CompileGraphError>,
    },
    Deactivated {
        error: Option<Box<dyn Error>>,
        returned_user_cx: Option<Box<dyn Any + Send>>,
    },
}
