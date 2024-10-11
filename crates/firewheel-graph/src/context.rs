use std::time::{Duration, Instant};

use rtrb::PushError;

use crate::{
    graph::{AudioGraph, AudioGraphConfig, CompileGraphError},
    processor::{ContextToProcessorMsg, FwProcessor, ProcessorToContextMsg},
};

const CHANNEL_CAPACITY: usize = 16;
const CLOSE_STREAM_TIMEOUT: Duration = Duration::from_secs(3);
const CLOSE_STREAM_SLEEP_INTERVAL: Duration = Duration::from_millis(2);

pub struct InactiveFwCtx<C, const MBF: usize> {
    graph: AudioGraph<C, MBF>,
}

impl<C: 'static, const MBF: usize> InactiveFwCtx<C, MBF> {
    pub fn new(graph_config: AudioGraphConfig) -> Self {
        Self {
            graph: AudioGraph::new(&graph_config),
        }
    }

    pub fn graph(&self) -> &AudioGraph<C, MBF> {
        &self.graph
    }

    pub fn graph_mut(&mut self) -> &mut AudioGraph<C, MBF> {
        &mut self.graph
    }

    pub fn activate(
        self,
        sample_rate: u32,
        num_stream_in_channels: usize,
        num_stream_out_channels: usize,
        user_cx: C,
    ) -> (ActiveFwCtx<C, MBF>, FwProcessor<C, MBF>) {
        let (to_executor_tx, from_graph_rx) =
            rtrb::RingBuffer::<ContextToProcessorMsg<C, MBF>>::new(CHANNEL_CAPACITY);
        let (to_graph_tx, from_executor_rx) =
            rtrb::RingBuffer::<ProcessorToContextMsg<C, MBF>>::new(CHANNEL_CAPACITY);

        let processor = FwProcessor::new(
            from_graph_rx,
            to_graph_tx,
            self.graph.current_node_capacity(),
            num_stream_in_channels,
            num_stream_out_channels,
            user_cx,
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

struct ActiveFwCtxInner<C, const MBF: usize> {
    pub graph: AudioGraph<C, MBF>,

    // TODO: Do research on whether `rtrb` is compatible with
    // webassembly. If not, use conditional compilation to
    // use a different channel type when targeting webassembly.
    to_executor_tx: rtrb::Producer<ContextToProcessorMsg<C, MBF>>,
    from_executor_rx: rtrb::Consumer<ProcessorToContextMsg<C, MBF>>,

    sample_rate: u32,
}

impl<C: 'static, const MBF: usize> ActiveFwCtxInner<C, MBF> {
    /// Update the firewheel context.
    ///
    /// This must be called reguarly (i.e. once every frame).
    fn update(&mut self) -> UpdateStatusInternal<C> {
        let mut dropped = false;
        let mut dropped_user_cx = None;

        self.update_internal(&mut dropped, &mut dropped_user_cx);

        if dropped {
            self.graph.deactivate();
            return UpdateStatusInternal::Deactivated(dropped_user_cx);
        }

        if self.graph.needs_compile() {
            match self.graph.compile(self.sample_rate) {
                Ok(schedule_data) => {
                    if let Err(e) = self
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
    fn deactivate(mut self, stream_is_running: bool) -> (InactiveFwCtx<C, MBF>, Option<C>) {
        let start = Instant::now();

        let mut dropped = false;
        let mut dropped_user_cx = None;

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

        (InactiveFwCtx { graph: self.graph }, dropped_user_cx)
    }

    fn update_internal(&mut self, dropped: &mut bool, dropped_user_cx: &mut Option<C>) {
        while let Ok(msg) = self.from_executor_rx.pop() {
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

pub struct ActiveFwCtx<C: 'static, const MBF: usize> {
    inner: Option<ActiveFwCtxInner<C, MBF>>,
}

impl<C: 'static, const MBF: usize> ActiveFwCtx<C, MBF> {
    pub fn graph(&self) -> &AudioGraph<C, MBF> {
        &self.inner.as_ref().unwrap().graph
    }

    pub fn graph_mut(&mut self) -> &mut AudioGraph<C, MBF> {
        &mut self.inner.as_mut().unwrap().graph
    }

    /// Update the firewheel context.
    ///
    /// This must be called reguarly (i.e. once every frame).
    pub fn update(mut self) -> UpdateStatus<C, MBF> {
        match self.inner.as_mut().unwrap().update() {
            UpdateStatusInternal::Ok => UpdateStatus::Ok {
                cx: self,
                graph_error: None,
            },
            UpdateStatusInternal::GraphError(e) => UpdateStatus::Ok {
                cx: self,
                graph_error: Some(e),
            },
            UpdateStatusInternal::Deactivated(user_cx) => UpdateStatus::Deactivated {
                cx: InactiveFwCtx {
                    graph: self.inner.take().unwrap().graph,
                },
                user_cx,
            },
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
    pub fn deactivate(mut self, stream_is_running: bool) -> (InactiveFwCtx<C, MBF>, Option<C>) {
        let inner = self.inner.take().unwrap();
        inner.deactivate(stream_is_running)
    }
}

impl<C: 'static, const MBF: usize> Drop for ActiveFwCtx<C, MBF> {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.take() {
            inner.deactivate(true);
        }
    }
}

pub enum FwCtx<C: 'static, const MBF: usize> {
    Inactive(InactiveFwCtx<C, MBF>),
    Active(ActiveFwCtx<C, MBF>),
}

impl<C: 'static, const MBF: usize> FwCtx<C, MBF> {
    pub fn new(graph_config: AudioGraphConfig) -> Self {
        Self::Inactive(InactiveFwCtx::new(graph_config))
    }
}

pub enum UpdateStatus<C: 'static, const MBF: usize> {
    Ok {
        cx: ActiveFwCtx<C, MBF>,
        graph_error: Option<CompileGraphError>,
    },
    Deactivated {
        cx: InactiveFwCtx<C, MBF>,
        user_cx: Option<C>,
    },
}

enum UpdateStatusInternal<C> {
    Ok,
    GraphError(CompileGraphError),
    Deactivated(Option<C>),
}
