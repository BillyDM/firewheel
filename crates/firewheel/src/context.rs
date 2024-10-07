use std::time::{Duration, Instant};

use rtrb::PushError;

use crate::{
    graph::{AudioGraph, CompileGraphError},
    processor::{ContextToProcessorMsg, FwProcessor, ProcessorToContextMsg},
};

const DEFAULT_CHANNEL_CAPACITY: usize = 256;
const CLOSE_STREAM_TIMEOUT: Duration = Duration::from_secs(3);
const CLOSE_STREAM_SLEEP_INTERVAL: Duration = Duration::from_millis(2);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Config {
    pub num_graph_inputs: usize,
    pub num_graph_outputs: usize,
    pub max_block_frames: usize,
    pub initial_node_capacity: usize,
    pub initial_edge_capacity: usize,
    pub message_channel_capacity: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            num_graph_inputs: 0,
            num_graph_outputs: 2,
            max_block_frames: 256,
            initial_node_capacity: 64,
            initial_edge_capacity: 256,
            message_channel_capacity: DEFAULT_CHANNEL_CAPACITY,
        }
    }
}

pub struct InactiveFwCtx {
    graph: AudioGraph,
}

impl InactiveFwCtx {
    pub fn new(config: Config) -> Self {
        assert_ne!(config.message_channel_capacity, 0);

        Self {
            graph: AudioGraph::new(&config),
        }
    }

    pub fn graph(&self) -> &AudioGraph {
        &self.graph
    }

    pub fn graph_mut(&mut self) -> &mut AudioGraph {
        &mut self.graph
    }

    pub fn activate(
        self,
        sample_rate: u32,
        num_stream_in_channels: usize,
        num_stream_out_channels: usize,
    ) -> (ActiveFwCtx, FwProcessor) {
        let (to_executor_tx, from_graph_rx) =
            rtrb::RingBuffer::<ContextToProcessorMsg>::new(self.graph.message_channel_capacity);
        let (to_graph_tx, from_executor_rx) =
            rtrb::RingBuffer::<ProcessorToContextMsg>::new(self.graph.message_channel_capacity);

        let processor = FwProcessor::new(
            from_graph_rx,
            to_graph_tx,
            self.graph.current_node_capacity(),
            num_stream_in_channels,
            num_stream_out_channels,
            self.graph.max_block_frames(),
        );

        (
            ActiveFwCtx {
                inner: Some(ActiveFwCtxInner {
                    graph: self.graph,
                    to_executor_tx,
                    from_executor_rx,
                    sample_rate,
                }),
            },
            processor,
        )
    }
}

struct ActiveFwCtxInner {
    pub graph: AudioGraph,

    // TODO: Do research on whether `rtrb` is compatible with
    // webassembly. If not, use conditional compilation to
    // use a different channel type when targeting webassembly.
    to_executor_tx: rtrb::Producer<ContextToProcessorMsg>,
    from_executor_rx: rtrb::Consumer<ProcessorToContextMsg>,

    sample_rate: u32,
}

impl ActiveFwCtxInner {
    /// Update the firewheel context.
    ///
    /// This must be called reguarly (i.e. once every frame).
    fn update(&mut self) -> UpdateStatusInternal {
        let mut dropped = false;

        self.update_internal(&mut dropped);

        if dropped {
            self.graph.deactivate();
            return UpdateStatusInternal::Deactivated;
        }

        if self.graph.needs_compile() {
            match self.graph.compile(self.sample_rate) {
                Ok(schedule_data) => {
                    if let Err(e) = self
                        .to_executor_tx
                        .push(ContextToProcessorMsg::NewSchedule(schedule_data))
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
                    return UpdateStatusInternal::GraphError(e);
                }
            }
        }

        UpdateStatusInternal::Ok
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
    fn deactivate(mut self, stream_is_running: bool) -> InactiveFwCtx {
        let start = Instant::now();

        let mut dropped = false;

        if stream_is_running {
            loop {
                if let Err(_) = self.to_executor_tx.push(ContextToProcessorMsg::Stop) {
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
            self.update_internal(&mut dropped);

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

        InactiveFwCtx { graph: self.graph }
    }

    fn update_internal(&mut self, dropped: &mut bool) {
        while let Ok(msg) = self.from_executor_rx.pop() {
            match msg {
                ProcessorToContextMsg::ReturnSchedule(schedule_data) => {
                    self.graph.on_schedule_returned(schedule_data);
                }
                ProcessorToContextMsg::Dropped { nodes, .. } => {
                    self.graph.on_processor_dropped(nodes);
                    *dropped = true;
                }
            }
        }
    }
}

pub struct ActiveFwCtx {
    inner: Option<ActiveFwCtxInner>,
}

impl ActiveFwCtx {
    pub fn graph(&self) -> &AudioGraph {
        &self.inner.as_ref().unwrap().graph
    }

    pub fn graph_mut(&mut self) -> &mut AudioGraph {
        &mut self.inner.as_mut().unwrap().graph
    }

    /// Update the firewheel context.
    ///
    /// This must be called reguarly (i.e. once every frame).
    pub fn update(mut self) -> UpdateStatus {
        match self.inner.as_mut().unwrap().update() {
            UpdateStatusInternal::Ok => UpdateStatus::Ok {
                cx: self,
                graph_error: None,
            },
            UpdateStatusInternal::GraphError(e) => UpdateStatus::Ok {
                cx: self,
                graph_error: Some(e),
            },
            UpdateStatusInternal::Deactivated => UpdateStatus::Deactivated(InactiveFwCtx {
                graph: self.inner.take().unwrap().graph,
            }),
        }
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
    pub fn deactivate(mut self, stream_is_running: bool) -> InactiveFwCtx {
        let inner = self.inner.take().unwrap();
        inner.deactivate(stream_is_running)
    }
}

impl Drop for ActiveFwCtx {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.take() {
            inner.deactivate(true);
        }
    }
}

pub enum UpdateStatus {
    Ok {
        cx: ActiveFwCtx,
        graph_error: Option<CompileGraphError>,
    },
    Deactivated(InactiveFwCtx),
}

enum UpdateStatusInternal {
    Ok,
    GraphError(CompileGraphError),
    Deactivated,
}
