use thunderdome::Arena;

use crate::graph::{NodeID, ScheduleHeapData};
use firewheel_core::{
    node::{AudioNodeProcessor, ProcInfo, StreamStatus},
    BlockFrames, SilenceMask,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FwProcessorStatus {
    Ok,
    /// If this is returned, then the [`FwProcessor`] must be dropped.
    DropProcessor,
}

pub struct FwProcessor<C: 'static, const MBF: usize> {
    nodes: Arena<Box<dyn AudioNodeProcessor<C, MBF>>>,
    schedule_data: Option<Box<ScheduleHeapData<C, MBF>>>,
    user_cx: Option<C>,

    // TODO: Do research on whether `rtrb` is compatible with
    // webassembly. If not, use conditional compilation to
    // use a different channel type when targeting webassembly.
    from_graph_rx: rtrb::Consumer<ContextToProcessorMsg<C, MBF>>,
    to_graph_tx: rtrb::Producer<ProcessorToContextMsg<C, MBF>>,

    running: bool,
}

impl<C, const MBF: usize> FwProcessor<C, MBF> {
    pub(crate) fn new(
        from_graph_rx: rtrb::Consumer<ContextToProcessorMsg<C, MBF>>,
        to_graph_tx: rtrb::Producer<ProcessorToContextMsg<C, MBF>>,
        node_capacity: usize,
        num_stream_in_channels: usize,
        num_stream_out_channels: usize,
        user_cx: C,
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
        }
    }

    /// Process the given buffers of audio data.
    ///
    /// If this returns [`ProcessStatus::DropProcessor`], then this
    /// [`FwProcessor`] must be dropped.
    pub fn process_interleaved(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        num_in_channels: usize,
        num_out_channels: usize,
        frames: usize,
        stream_time_secs: f64,
        stream_status: StreamStatus,
    ) -> FwProcessorStatus {
        if !self.running {
            output.fill(0.0);
            return FwProcessorStatus::DropProcessor;
        }

        if self.schedule_data.is_none() {
            // See if we got a new schedule.
            self.poll_messages();

            if !self.running {
                output.fill(0.0);
                return FwProcessorStatus::DropProcessor;
            }
        }

        if self.schedule_data.is_none() || frames == 0 {
            output.fill(0.0);
            return FwProcessorStatus::Ok;
        };

        assert_eq!(input.len(), frames * num_in_channels);
        assert_eq!(output.len(), frames * num_out_channels);

        let mut frames_processed = 0;
        while frames_processed < frames {
            let block_frames = BlockFrames::new(frames - frames_processed);
            let frames = block_frames.get();

            // Prepare graph input buffers.
            self.schedule_data
                .as_mut()
                .unwrap()
                .schedule
                .prepare_graph_inputs(
                    num_in_channels,
                    |channels: &mut [&mut [f32; MBF]]| -> SilenceMask {
                        firewheel_core::util::deinterleave(
                            channels.iter_mut().map(|ch| &mut ch[..frames]),
                            &input[frames_processed * num_in_channels
                                ..(frames_processed + frames) * num_in_channels],
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
                    num_out_channels,
                    |channels: &[&[f32; MBF]], silence_mask| {
                        if channels.len() == 2 && num_out_channels == 2 {
                            // Use optimized stereo interleaving since it is the most
                            // common case.
                            firewheel_core::util::interleave_stereo(
                                &channels[0][..frames],
                                &channels[1][..frames],
                                &mut output[frames_processed * num_out_channels
                                    ..(frames_processed + frames) * num_out_channels],
                                Some(silence_mask),
                            );
                        } else {
                            firewheel_core::util::interleave(
                                channels.iter().map(|ch| &ch[..frames]),
                                &mut output[frames_processed * num_out_channels
                                    ..(frames_processed + frames) * num_out_channels],
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

            frames_processed += frames;
        }

        if self.running {
            FwProcessorStatus::Ok
        } else {
            FwProcessorStatus::DropProcessor
        }
    }

    fn poll_messages(&mut self) {
        while let Ok(msg) = self.from_graph_rx.pop() {
            match msg {
                ContextToProcessorMsg::NewSchedule(mut new_schedule_data) => {
                    if let Some(mut old_schedule_data) = self.schedule_data.take() {
                        std::mem::swap(
                            &mut old_schedule_data.removed_node_processors,
                            &mut new_schedule_data.removed_node_processors,
                        );

                        for node_id in new_schedule_data.nodes_to_remove.iter() {
                            if let Some(processor) = self.nodes.remove(node_id.0) {
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
                        assert!(self.nodes.insert_at(node_id.0, processor).is_none());
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
        block_frames: BlockFrames<MBF>,
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
             inputs: &[&[f32; MBF]],
             outputs: &mut [&mut [f32; MBF]]|
             -> SilenceMask {
                let mut out_silence_mask = SilenceMask::NONE_SILENT;

                let proc_info = ProcInfo {
                    in_silence_mask,
                    out_silence_mask: &mut out_silence_mask,
                    stream_time_secs,
                    stream_status,
                    cx: user_cx,
                };

                self.nodes[node_id.0].process(block_frames, inputs, outputs, proc_info);

                out_silence_mask
            },
        );
    }
}

impl<C, const MBF: usize> Drop for FwProcessor<C, MBF> {
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

pub(crate) enum ContextToProcessorMsg<C, const MBF: usize> {
    NewSchedule(Box<ScheduleHeapData<C, MBF>>),
    Stop,
}

pub(crate) enum ProcessorToContextMsg<C, const MBF: usize> {
    ReturnSchedule(Box<ScheduleHeapData<C, MBF>>),
    Dropped {
        nodes: Arena<Box<dyn AudioNodeProcessor<C, MBF>>>,
        _schedule_data: Option<Box<ScheduleHeapData<C, MBF>>>,
        user_cx: Option<C>,
    },
}
