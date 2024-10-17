use std::any::Any;

use thunderdome::Arena;

use crate::graph::{NodeID, ScheduleHeapData};
use firewheel_core::{
    node::{AudioNodeProcessor, ProcInfo, StreamStatus},
    SilenceMask,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirewheelProcessorStatus {
    Ok,
    /// If this is returned, then the [`FirewheelProcessor`] must be dropped.
    DropProcessor,
}

pub struct FirewheelProcessor {
    nodes: Arena<Box<dyn AudioNodeProcessor>>,
    schedule_data: Option<Box<ScheduleHeapData>>,
    user_cx: Option<Box<dyn Any + Send>>,

    // TODO: Do research on whether `rtrb` is compatible with
    // webassembly. If not, use conditional compilation to
    // use a different channel type when targeting webassembly.
    from_graph_rx: rtrb::Consumer<ContextToProcessorMsg>,
    to_graph_tx: rtrb::Producer<ProcessorToContextMsg>,

    running: bool,
    max_block_frames: usize,
}

impl FirewheelProcessor {
    pub(crate) fn new(
        from_graph_rx: rtrb::Consumer<ContextToProcessorMsg>,
        to_graph_tx: rtrb::Producer<ProcessorToContextMsg>,
        node_capacity: usize,
        num_stream_in_channels: usize,
        num_stream_out_channels: usize,
        max_block_frames: usize,
        user_cx: Box<dyn Any + Send>,
    ) -> Self {
        assert!(num_stream_in_channels <= 64);
        assert!(num_stream_out_channels <= 64);

        Self {
            nodes: Arena::with_capacity(node_capacity * 2),
            schedule_data: None,
            user_cx: Some(user_cx),
            from_graph_rx,
            to_graph_tx,
            running: true,
            max_block_frames,
        }
    }

    /// Process the given buffers of audio data.
    ///
    /// If this returns [`ProcessStatus::DropProcessor`], then this
    /// [`FirewheelProcessor`] must be dropped.
    pub fn process_interleaved(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        num_in_channels: usize,
        num_out_channels: usize,
        frames: usize,
        stream_time_secs: f64,
        stream_status: StreamStatus,
    ) -> FirewheelProcessorStatus {
        if !self.running {
            output.fill(0.0);
            return FirewheelProcessorStatus::DropProcessor;
        }

        if self.schedule_data.is_none() {
            // See if we got a new schedule.
            self.poll_messages();

            if !self.running {
                output.fill(0.0);
                return FirewheelProcessorStatus::DropProcessor;
            }
        }

        if self.schedule_data.is_none() || frames == 0 {
            output.fill(0.0);
            return FirewheelProcessorStatus::Ok;
        };

        assert_eq!(input.len(), frames * num_in_channels);
        assert_eq!(output.len(), frames * num_out_channels);

        let mut frames_processed = 0;
        while frames_processed < frames {
            let block_frames = (frames - frames_processed).min(self.max_block_frames);

            // Prepare graph input buffers.
            self.schedule_data
                .as_mut()
                .unwrap()
                .schedule
                .prepare_graph_inputs(
                    block_frames,
                    num_in_channels,
                    |channels: &mut [&mut [f32]]| -> SilenceMask {
                        firewheel_core::util::deinterleave(
                            channels.iter_mut().map(|ch| &mut ch[..block_frames]),
                            &input[frames_processed * num_in_channels
                                ..(frames_processed + block_frames) * num_in_channels],
                            num_in_channels,
                            true,
                        )
                    },
                );

            self.process_block(block_frames, stream_time_secs, stream_status);

            // Copy the output of the graph to the output buffer.
            self.schedule_data
                .as_mut()
                .unwrap()
                .schedule
                .read_graph_outputs(
                    block_frames,
                    num_out_channels,
                    |channels: &[&[f32]], silence_mask| {
                        if channels.len() == 2 && num_out_channels == 2 {
                            // Use optimized stereo interleaving since it is the most
                            // common case.
                            firewheel_core::util::interleave_stereo(
                                &channels[0][..block_frames],
                                &channels[1][..block_frames],
                                &mut output[frames_processed * num_out_channels
                                    ..(frames_processed + block_frames) * num_out_channels],
                                Some(silence_mask),
                            );
                        } else {
                            firewheel_core::util::interleave(
                                channels.iter().map(|ch| &ch[..block_frames]),
                                &mut output[frames_processed * num_out_channels
                                    ..(frames_processed + block_frames) * num_out_channels],
                                num_out_channels,
                                Some(silence_mask),
                            );
                        }
                    },
                );

            if !self.running {
                if frames_processed < frames {
                    output[frames_processed * num_out_channels..].fill(0.0);
                }
                break;
            }

            frames_processed += block_frames;
        }

        if self.running {
            FirewheelProcessorStatus::Ok
        } else {
            FirewheelProcessorStatus::DropProcessor
        }
    }

    fn poll_messages(&mut self) {
        while let Ok(msg) = self.from_graph_rx.pop() {
            match msg {
                ContextToProcessorMsg::NewSchedule(mut new_schedule_data) => {
                    assert_eq!(
                        new_schedule_data.schedule.max_block_frames(),
                        self.max_block_frames
                    );

                    if let Some(mut old_schedule_data) = self.schedule_data.take() {
                        std::mem::swap(
                            &mut old_schedule_data.removed_node_processors,
                            &mut new_schedule_data.removed_node_processors,
                        );

                        for node_id in new_schedule_data.nodes_to_remove.iter() {
                            if let Some(processor) = self.nodes.remove(node_id.idx) {
                                old_schedule_data
                                    .removed_node_processors
                                    .push((*node_id, processor));
                            }
                        }

                        self.to_graph_tx
                            .push(ProcessorToContextMsg::ReturnSchedule(old_schedule_data))
                            .unwrap();
                    }

                    for (node_id, processor) in new_schedule_data.new_node_processors.drain(..) {
                        assert!(self.nodes.insert_at(node_id.idx, processor).is_none());
                    }

                    self.schedule_data = Some(new_schedule_data);
                }
                ContextToProcessorMsg::Stop => {
                    self.running = false;
                }
            }
        }
    }

    fn process_block(
        &mut self,
        block_frames: usize,
        stream_time_secs: f64,
        stream_status: StreamStatus,
    ) {
        self.poll_messages();

        if !self.running {
            return;
        }

        let Some(schedule_data) = &mut self.schedule_data else {
            return;
        };

        let user_cx = self.user_cx.as_mut().unwrap();

        schedule_data.schedule.process(
            block_frames,
            |node_id: NodeID,
             in_silence_mask: SilenceMask,
             inputs: &[&[f32]],
             outputs: &mut [&mut [f32]]|
             -> SilenceMask {
                let mut out_silence_mask = SilenceMask::NONE_SILENT;

                let proc_info = ProcInfo {
                    in_silence_mask,
                    out_silence_mask: &mut out_silence_mask,
                    stream_time_secs,
                    stream_status,
                    cx: user_cx,
                };

                self.nodes[node_id.idx].process(block_frames, inputs, outputs, proc_info);

                out_silence_mask
            },
        );
    }
}

impl Drop for FirewheelProcessor {
    fn drop(&mut self) {
        // Make sure the nodes are not deallocated in the audio thread.
        let mut nodes = Arena::new();
        std::mem::swap(&mut nodes, &mut self.nodes);

        let _ = self.to_graph_tx.push(ProcessorToContextMsg::Dropped {
            nodes,
            _schedule_data: self.schedule_data.take(),
            user_cx: self.user_cx.take(),
        });
    }
}

pub(crate) enum ContextToProcessorMsg {
    NewSchedule(Box<ScheduleHeapData>),
    Stop,
}

pub(crate) enum ProcessorToContextMsg {
    ReturnSchedule(Box<ScheduleHeapData>),
    Dropped {
        nodes: Arena<Box<dyn AudioNodeProcessor>>,
        _schedule_data: Option<Box<ScheduleHeapData>>,
        user_cx: Option<Box<dyn Any + Send>>,
    },
}
