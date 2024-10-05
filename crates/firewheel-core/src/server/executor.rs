use thunderdome::Arena;

use crate::{
    node::{AudioNodeProcessor, ProcInfo},
    SilenceMask,
};

use super::{compiler::CompiledSchedule, NodeID};

pub struct AudioGraphExecutor {
    nodes: Arena<Box<dyn AudioNodeProcessor>>,
    schedule_data: Option<ScheduleHeapData>,

    // TODO: Do research on whether `rtrb` is compatible with
    // webassembly. If not, use conditional compilation to
    // use a different channel type when targeting webassembly.
    from_graph_rx: rtrb::Consumer<GraphToExecutorMsg>,
    to_graph_tx: rtrb::Producer<ExecutorToGraphMsg>,

    max_block_frames: usize,

    stream_in_buffer_list: Option<Vec<&'static mut [f32]>>,
    stream_out_buffer_list: Option<Vec<&'static [f32]>>,

    running: bool,
}

impl AudioGraphExecutor {
    pub(crate) fn new(
        from_graph_rx: rtrb::Consumer<GraphToExecutorMsg>,
        to_graph_tx: rtrb::Producer<ExecutorToGraphMsg>,
        max_node_capacity: usize,
        num_stream_in_channels: u32,
        num_stream_out_channels: u32,
        max_block_frames: usize,
    ) -> Self {
        Self {
            nodes: Arena::with_capacity(max_node_capacity),
            schedule_data: None,
            from_graph_rx,
            to_graph_tx,
            max_block_frames,
            stream_in_buffer_list: Some(Vec::with_capacity(num_stream_in_channels as usize)),
            stream_out_buffer_list: Some(Vec::with_capacity(num_stream_out_channels as usize)),
            running: true,
        }
    }

    pub fn process_interleaved(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        num_in_channels: usize,
        num_out_channels: usize,
        frames: usize,
    ) {
        if self.schedule_data.is_none() || frames == 0 || !self.running {
            output.fill(0.0);
            return;
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
                    &mut self.stream_in_buffer_list,
                    |channels: &mut [&mut [f32]]| -> SilenceMask {
                        crate::util::deinterleave(
                            channels.iter_mut().map(|ch| &mut **ch),
                            &input[frames_processed * num_in_channels
                                ..(frames_processed + block_frames) * num_in_channels],
                            num_in_channels,
                            true,
                        )
                    },
                );

            self.process_block(block_frames);

            // Copy the output of the graph to the output buffer.
            self.schedule_data
                .as_mut()
                .unwrap()
                .schedule
                .read_graph_outputs(
                    block_frames,
                    num_out_channels,
                    &mut self.stream_out_buffer_list,
                    |channels: &[&[f32]], silence_mask| {
                        if channels.len() == 2 && num_out_channels == 2 {
                            // Use optimized stereo interleaving since it is the most
                            // common case.
                            crate::util::interleave_stereo(
                                &channels[0],
                                &channels[1],
                                &mut output[frames_processed * num_out_channels
                                    ..(frames_processed + block_frames) * num_out_channels],
                                Some(silence_mask),
                            );
                        } else {
                            crate::util::interleave(
                                channels.iter().map(|ch| &**ch),
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

        if !self.running {
            self.to_graph_tx.push(ExecutorToGraphMsg::Stopped).unwrap();
        }
    }

    fn process_block(&mut self, block_frames: usize) {
        while let Ok(msg) = self.from_graph_rx.pop() {
            match msg {
                GraphToExecutorMsg::NewSchedule(mut new_schedule_data) => {
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
                            if let Some(processor) = self.nodes.remove(node_id.0) {
                                old_schedule_data
                                    .removed_node_processors
                                    .push((*node_id, processor));
                            }
                        }

                        self.to_graph_tx
                            .push(ExecutorToGraphMsg::ReturnSchedule(old_schedule_data))
                            .unwrap();
                    }

                    for (node_id, processor) in new_schedule_data.nodes_to_add.drain(..) {
                        assert!(self.nodes.insert_at(node_id.0, processor).is_none());
                    }

                    self.schedule_data = Some(new_schedule_data);
                }
                GraphToExecutorMsg::Stop => {
                    self.running = false;
                }
            }
        }

        if !self.running {
            return;
        }

        let Some(schedule_data) = &mut self.schedule_data else {
            return;
        };

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
                };

                self.nodes[node_id.0].process(block_frames, proc_info, inputs, outputs);

                out_silence_mask
            },
        );
    }
}

impl Drop for AudioGraphExecutor {
    fn drop(&mut self) {
        // Make sure the nodes are not deallocated in the audio thread.
        let mut nodes = Arena::new();
        std::mem::swap(&mut nodes, &mut self.nodes);

        let _ = self.to_graph_tx.push(ExecutorToGraphMsg::Dropped {
            nodes,
            _schedule_data: self.schedule_data.take(),
        });
    }
}

pub(crate) struct ScheduleHeapData {
    schedule: CompiledSchedule,
    nodes_to_remove: Vec<NodeID>,
    pub removed_node_processors: Vec<(NodeID, Box<dyn AudioNodeProcessor>)>,
    nodes_to_add: Vec<(NodeID, Box<dyn AudioNodeProcessor>)>,
}

impl ScheduleHeapData {
    pub fn new(
        schedule: CompiledSchedule,
        nodes_to_remove: Vec<NodeID>,
        nodes_to_add: Vec<(NodeID, Box<dyn AudioNodeProcessor>)>,
    ) -> Self {
        let num_nodes_to_remove = nodes_to_remove.len();

        Self {
            schedule,
            nodes_to_remove,
            removed_node_processors: Vec::with_capacity(num_nodes_to_remove),
            nodes_to_add,
        }
    }
}

pub(crate) enum GraphToExecutorMsg {
    NewSchedule(ScheduleHeapData),
    Stop,
}

pub(crate) enum ExecutorToGraphMsg {
    ReturnSchedule(ScheduleHeapData),
    Stopped,
    Dropped {
        nodes: Arena<Box<dyn AudioNodeProcessor>>,
        _schedule_data: Option<ScheduleHeapData>,
    },
}
