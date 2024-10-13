use arrayvec::ArrayVec;
use smallvec::SmallVec;
use std::{cell::UnsafeCell, fmt::Debug};

use firewheel_core::{node::AudioNodeProcessor, BlockFrames, SilenceMask};

use super::NodeID;

/// A [ScheduledNode] is a [Node] that has been assigned buffers
/// and a place in the schedule.
#[derive(Clone)]
pub(super) struct ScheduledNode {
    /// The node ID
    pub id: NodeID,

    /// The assigned input buffers.
    pub input_buffers: SmallVec<[InBufferAssignment; 4]>,
    /// The assigned output buffers.
    pub output_buffers: SmallVec<[OutBufferAssignment; 4]>,
}

impl ScheduledNode {
    pub fn new(id: NodeID) -> Self {
        Self {
            id,
            input_buffers: SmallVec::new(),
            output_buffers: SmallVec::new(),
        }
    }
}

impl Debug for ScheduledNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{{ {:?}", &self.id)?;

        if !self.input_buffers.is_empty() {
            write!(f, " | in: [")?;

            write!(f, "{}", self.input_buffers[0].buffer_index)?;
            for b in self.input_buffers.iter().skip(1) {
                write!(f, ", {}", b.buffer_index)?;
            }

            write!(f, "]")?;
        }

        if !self.output_buffers.is_empty() {
            write!(f, " | out: [")?;

            write!(f, "{}", self.output_buffers[0].buffer_index)?;
            for b in self.output_buffers.iter().skip(1) {
                write!(f, ", {}", b.buffer_index)?;
            }
            
            write!(f, "]")?;
        }

        if !self.input_buffers.is_empty() {
            write!(f, " | in_clear: [")?;

            write!(
                f,
                "{}",
                if self.input_buffers[0].should_clear {
                    'y'
                } else {
                    'n'
                }
            )?;
            for b in self.input_buffers.iter().skip(1) {
                write!(f, ", {}", if b.should_clear { 'y' } else { 'n' })?;
            }

            write!(f, "]")?;
        }

        if !self.input_buffers.is_empty() {
            write!(f, " | in_gen: [")?;

            write!(f, "{}", self.input_buffers[0].generation)?;
            for b in self.input_buffers.iter().skip(1) {
                write!(f, ", {}", b.generation)?;
            }

            write!(f, "]")?;
        }

        if !self.output_buffers.is_empty() {
            write!(f, " | out_gen: [")?;

            write!(f, "{}", self.output_buffers[0].generation)?;
            for b in self.output_buffers.iter().skip(1) {
                write!(f, ", {}", b.generation)?;
            }

            write!(f, "]")?;
        }

        write!(f, " }}")
    }
}

/// Represents a single buffer assigned to an input port
#[derive(Copy, Clone, Debug)]
pub(super) struct InBufferAssignment {
    /// The index of the buffer assigned
    pub buffer_index: usize,
    /// Whether the engine should clear the buffer before
    /// passing it to a process
    pub should_clear: bool,
    /// Buffers are reused, the "generation" represents
    /// how many times this buffer has been used before
    /// this assignment. Kept for debugging and visualization.
    pub generation: usize,
}

/// Represents a single buffer assigned to an output port
#[derive(Copy, Clone, Debug)]
pub(super) struct OutBufferAssignment {
    /// The index of the buffer assigned
    pub buffer_index: usize,
    /// Buffers are reused, the "generation" represents
    /// how many times this buffer has been used before
    /// this assignment. Kept for debugging and visualization.
    pub generation: usize,
}

pub struct ScheduleHeapData<C, const MBF: usize> {
    pub schedule: CompiledSchedule<MBF>,
    pub nodes_to_remove: Vec<NodeID>,
    pub removed_node_processors: Vec<(NodeID, Box<dyn AudioNodeProcessor<C, MBF>>)>,
    pub new_node_processors: Vec<(NodeID, Box<dyn AudioNodeProcessor<C, MBF>>)>,
}

impl<C, const MBF: usize> ScheduleHeapData<C, MBF> {
    pub fn new(
        schedule: CompiledSchedule<MBF>,
        nodes_to_remove: Vec<NodeID>,
        new_node_processors: Vec<(NodeID, Box<dyn AudioNodeProcessor<C, MBF>>)>,
    ) -> Self {
        let num_nodes_to_remove = nodes_to_remove.len();

        Self {
            schedule,
            nodes_to_remove,
            removed_node_processors: Vec::with_capacity(num_nodes_to_remove),
            new_node_processors,
        }
    }
}

impl<C, const MBF: usize> Debug for ScheduleHeapData<C, MBF> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let new_node_processors: Vec<NodeID> =
            self.new_node_processors.iter().map(|(id, _)| *id).collect();

        f.debug_struct("ScheduleHeapData")
            .field("schedule", &self.schedule)
            .field("nodes_to_remove", &self.nodes_to_remove)
            .field("new_node_processors", &new_node_processors)
            .finish()
    }
}

/// A [CompiledSchedule] is the output of the graph compiler.
pub struct CompiledSchedule<const MBF: usize> {
    schedule: Vec<ScheduledNode>,

    buffers: Vec<UnsafeCell<[f32; MBF]>>,
    buffer_silence_flags: Vec<bool>,
}

impl<const MBF: usize> Debug for CompiledSchedule<MBF> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "CompiledSchedule {{")?;

        writeln!(f, "    schedule: {{")?;

        for n in self.schedule.iter() {
            writeln!(f, "        {:?}", n)?;
        }

        writeln!(f, "    }}")?;

        writeln!(f, "    num_buffers: {}", self.buffers.len())?;

        writeln!(f, "}}")
    }
}

impl<const MBF: usize> CompiledSchedule<MBF> {
    pub(super) fn new(schedule: Vec<ScheduledNode>, num_buffers: usize) -> Self {
        Self {
            schedule,
            buffers: (0..num_buffers)
                .map(|_| UnsafeCell::new([0.0; MBF]))
                .collect(),
            buffer_silence_flags: vec![false; num_buffers],
        }
    }

    pub fn prepare_graph_inputs(
        &mut self,
        num_stream_inputs: usize,
        fill_inputs: impl FnOnce(&mut [&mut [f32; MBF]]) -> SilenceMask,
    ) {
        let graph_in_node = self.schedule.first().unwrap();

        let mut inputs: ArrayVec<&mut [f32; MBF], 64> = ArrayVec::new();

        let fill_input_len = num_stream_inputs.min(graph_in_node.output_buffers.len());

        for i in 0..fill_input_len {
            inputs.push(buffer_mut(
                &self.buffers,
                graph_in_node.output_buffers[i].buffer_index,
            ));
        }

        let silence_mask = (fill_inputs)(inputs.as_mut_slice());

        for i in 0..fill_input_len {
            let buffer_index = graph_in_node.output_buffers[i].buffer_index;
            *silence_mask_mut(&mut self.buffer_silence_flags, buffer_index) =
                silence_mask.is_channel_silent(i);
        }

        if fill_input_len < graph_in_node.output_buffers.len() {
            for b in graph_in_node.output_buffers.iter().skip(fill_input_len) {
                let buf_slice = buffer_mut(&self.buffers, b.buffer_index);
                buf_slice.fill(0.0);

                *silence_mask_mut(&mut self.buffer_silence_flags, b.buffer_index) = true;
            }
        }
    }

    pub fn read_graph_outputs(
        &mut self,
        num_stream_outputs: usize,
        read_outputs: impl FnOnce(&[&[f32; MBF]], SilenceMask),
    ) {
        let graph_out_node = self.schedule.last().unwrap();

        let mut outputs: ArrayVec<&[f32; MBF], 64> = ArrayVec::new();

        let mut silence_mask = SilenceMask::NONE_SILENT;

        let read_output_len = num_stream_outputs.min(graph_out_node.input_buffers.len());

        for i in 0..read_output_len {
            let buffer_index = graph_out_node.input_buffers[i].buffer_index;

            if *silence_mask_mut(&mut self.buffer_silence_flags, buffer_index) {
                silence_mask.set_channel(i, true);
            }

            outputs.push(buffer_mut(&self.buffers, buffer_index));
        }

        (read_outputs)(outputs.as_slice(), silence_mask);
    }

    pub fn process(
        &mut self,
        frames: BlockFrames<MBF>,
        mut process: impl FnMut(
            NodeID,
            SilenceMask,
            &[&[f32; MBF]],
            &mut [&mut [f32; MBF]],
        ) -> SilenceMask,
    ) {
        let frames = frames.get();

        let mut inputs: ArrayVec<&[f32; MBF], 64> = ArrayVec::new();
        let mut outputs: ArrayVec<&mut [f32; MBF], 64> = ArrayVec::new();

        for scheduled_node in self.schedule.iter() {
            let mut in_silence_mask = SilenceMask::NONE_SILENT;

            inputs.clear();
            outputs.clear();

            for (i, b) in scheduled_node.input_buffers.iter().enumerate() {
                let buf = buffer_mut(&self.buffers, b.buffer_index);
                let s = silence_mask_mut(&mut self.buffer_silence_flags, b.buffer_index);

                if b.should_clear {
                    buf[..frames].fill(0.0);
                    *s = true;
                }

                if *s {
                    in_silence_mask.set_channel(i, true);
                }

                inputs.push(buf);
            }

            for b in scheduled_node.output_buffers.iter() {
                outputs.push(buffer_mut(&self.buffers, b.buffer_index));
            }

            let out_silence_mask = (process)(
                scheduled_node.id,
                in_silence_mask,
                inputs.as_slice(),
                outputs.as_mut_slice(),
            );

            for (i, b) in scheduled_node.output_buffers.iter().enumerate() {
                *silence_mask_mut(&mut self.buffer_silence_flags, b.buffer_index) =
                    out_silence_mask.is_channel_silent(i);
            }
        }
    }
}

#[inline]
fn buffer_mut<'a, const MBF: usize>(
    buffers: &'a [UnsafeCell<[f32; MBF]>],
    buffer_index: usize,
) -> &'a mut [f32; MBF] {
    // SAFETY
    //
    // `buffer_index` is gauranteed to be valid because [`BufferAllocator`]
    // correctly counts the total number of buffers used, and therefore
    // `b.buffer_index` is gauranteed to be less than the value of
    // `num_buffers` that was passed into [`CompiledSchedule::new`].
    //
    // Due to the way [`GraphIR::solve_buffer_requirements`] works, no
    // two buffer indexes in a single `ScheduledNode` can alias. (A buffer
    // index can only be reused after `allocator.release()` is called for
    // that buffer, and that method only gets called *after* all buffer
    // assignments have already been populated for that `ScheduledNode`.)
    // Also, `self` is borrowed mutably here, ensuring that the caller cannot
    // call any other method on [`CompiledSchedule`] while those buffers are
    // still borrowed.
    unsafe { &mut *UnsafeCell::get(buffers.get_unchecked(buffer_index)) }
}

#[inline]
fn silence_mask_mut<'a>(buffer_silence_flags: &'a mut [bool], buffer_index: usize) -> &'a mut bool {
    // SAFETY
    //
    // `buffer_index` is gauranteed to be valid because [`BufferAllocator`]
    // correctly counts the total number of buffers used, and therefore
    // `b.buffer_index` is gauranteed to be less than the value of
    // `num_buffers` that was passed into [`CompiledSchedule::new`].
    unsafe { buffer_silence_flags.get_unchecked_mut(buffer_index) }
}

#[cfg(test)]
mod tests {
    use crate::{
        basic_nodes::DummyAudioNode,
        graph::{AddEdgeError, AudioGraph, AudioGraphConfig, EdgeID, InPortIdx, OutPortIdx},
    };

    use super::*;
    use ahash::AHashSet;

    // Simplest graph compile test:
    //
    //  ┌───┐  ┌───┐
    //  │ 0 ┼──► 1 │
    //  └───┘  └───┘
    #[test]
    fn simplest_graph_compile_test() {
        let mut graph = AudioGraph::new(&AudioGraphConfig {
            num_graph_inputs: 1,
            num_graph_outputs: 1,
            ..Default::default()
        });

        let node0 = graph.graph_in_node();
        let node1 = graph.graph_out_node();

        let edge0 = graph.connect(node0, 0, node1, 0, false).unwrap();

        let schedule = graph.compile_internal().unwrap();

        dbg!(&schedule);

        assert_eq!(schedule.schedule.len(), 2);
        assert!(schedule.buffers.len() > 0);

        // First node must be node 0
        assert_eq!(schedule.schedule[0].id, node0);
        // Last node must be node 1
        assert_eq!(schedule.schedule[1].id, node1);

        verify_node(node0, &[], &schedule, &graph);
        verify_node(node1, &[false], &schedule, &graph);

        verify_edge(edge0, &graph, &schedule);
    }

    // Graph compile test 1:
    //
    //              ┌───┐  ┌───┐
    //         ┌────►   ┼──►   │
    //       ┌─┼─┐  ┼ 3 ┼──►   │
    //   ┌───►   │  └───┘  │   │  ┌───┐
    // ┌─┼─┐ │ 1 │  ┌───┐  │ 5 ┼──►   │
    // │   │ └─┬─┘  ┼   ┼──►   ┼──► 6 │
    // │ 0 │   └────► 4 ┼──►   │  └───┘
    // └─┬─┘        └───┘  │   │
    //   │   ┌───┐         │   │
    //   └───► 2 ┼─────────►   │
    //       └───┘         └───┘
    #[test]
    fn graph_compile_test_1() {
        let mut graph = AudioGraph::new(&AudioGraphConfig {
            num_graph_inputs: 2,
            num_graph_outputs: 2,
            ..Default::default()
        });

        let node0 = graph.graph_in_node();
        let node1 = graph.add_node(1, 2, DummyAudioNode);
        let node2 = graph.add_node(1, 1, DummyAudioNode);
        let node3 = graph.add_node(2, 2, DummyAudioNode);
        let node4 = graph.add_node(2, 2, DummyAudioNode);
        let node5 = graph.add_node(5, 2, DummyAudioNode);
        let node6 = graph.graph_out_node();

        let edge0 = graph.connect(node0, 0, node1, 0, false).unwrap();
        let edge1 = graph.connect(node0, 1, node2, 0, false).unwrap();
        let edge2 = graph.connect(node1, 0, node3, 0, false).unwrap();
        let edge3 = graph.connect(node1, 1, node4, 1, false).unwrap();
        let edge4 = graph.connect(node3, 0, node5, 0, false).unwrap();
        let edge5 = graph.connect(node3, 1, node5, 1, false).unwrap();
        let edge6 = graph.connect(node4, 0, node5, 2, false).unwrap();
        let edge7 = graph.connect(node4, 1, node5, 3, false).unwrap();
        let edge8 = graph.connect(node2, 0, node5, 4, false).unwrap();
        let edge9 = graph.connect(node5, 0, node6, 0, false).unwrap();
        let edge10 = graph.connect(node5, 1, node6, 1, false).unwrap();

        let schedule = graph.compile_internal().unwrap();

        dbg!(&schedule);

        assert_eq!(schedule.schedule.len(), 7);
        // Node 5 needs at-least 7 buffers
        assert!(schedule.buffers.len() > 6);

        // First node must be node 0
        assert_eq!(schedule.schedule[0].id, node0);
        // Next two nodes must be 1 and 2
        assert!(schedule.schedule[1].id == node1 || schedule.schedule[1].id == node2);
        assert!(schedule.schedule[2].id == node1 || schedule.schedule[2].id == node2);
        // Next two nodes must be 3 and 4
        assert!(schedule.schedule[3].id == node3 || schedule.schedule[3].id == node4);
        assert!(schedule.schedule[4].id == node3 || schedule.schedule[4].id == node4);
        // Next node must be 5
        assert_eq!(schedule.schedule[5].id, node5);
        // Last node must be 6
        assert_eq!(schedule.schedule[6].id, node6);

        verify_node(node0, &[], &schedule, &graph);
        verify_node(node1, &[false], &schedule, &graph);
        verify_node(node2, &[false], &schedule, &graph);
        verify_node(node3, &[false, true], &schedule, &graph);
        verify_node(node4, &[true, false], &schedule, &graph);
        verify_node(
            node5,
            &[false, false, false, false, false],
            &schedule,
            &graph,
        );
        verify_node(node6, &[false, false], &schedule, &graph);

        verify_edge(edge0, &graph, &schedule);
        verify_edge(edge1, &graph, &schedule);
        verify_edge(edge2, &graph, &schedule);
        verify_edge(edge3, &graph, &schedule);
        verify_edge(edge4, &graph, &schedule);
        verify_edge(edge5, &graph, &schedule);
        verify_edge(edge6, &graph, &schedule);
        verify_edge(edge7, &graph, &schedule);
        verify_edge(edge8, &graph, &schedule);
        verify_edge(edge9, &graph, &schedule);
        verify_edge(edge10, &graph, &schedule);
    }

    // Graph compile test 2:
    //
    //          ┌───┐  ┌───┐
    //     ┌────►   ┼──►   │
    //   ┌─┼─┐  ┼ 2 ┼  ┼   │  ┌───┐
    //   |   │  └───┘  │   ┼──►   │
    //   │ 0 │  ┌───┐  │ 4 ┼  ┼ 5 │
    //   └─┬─┘  ┼   ┼  ┼   │  └───┘
    //     └────► 3 ┼──►   │  ┌───┐
    //          └───┘  │   ┼──► 6 ┼
    //   ┌───┐         │   │  └───┘
    //   ┼ 1 ┼─────────►   ┼
    //   └───┘         └───┘
    #[test]
    fn graph_compile_test_2() {
        let mut graph = AudioGraph::new(&AudioGraphConfig {
            num_graph_inputs: 2,
            num_graph_outputs: 2,
            ..Default::default()
        });

        let node0 = graph.graph_in_node();
        let node1 = graph.add_node(1, 1, DummyAudioNode);
        let node2 = graph.add_node(2, 2, DummyAudioNode);
        let node3 = graph.add_node(2, 2, DummyAudioNode);
        let node4 = graph.add_node(5, 4, DummyAudioNode);
        let node5 = graph.graph_out_node();
        let node6 = graph.add_node(1, 1, DummyAudioNode);

        let edge0 = graph.connect(node0, 0, node2, 0, false).unwrap();
        let edge1 = graph.connect(node0, 0, node3, 1, false).unwrap();
        let edge2 = graph.connect(node2, 0, node4, 0, false).unwrap();
        let edge3 = graph.connect(node3, 1, node4, 3, false).unwrap();
        let edge4 = graph.connect(node1, 0, node4, 4, false).unwrap();
        let edge5 = graph.connect(node4, 0, node5, 0, false).unwrap();
        let edge6 = graph.connect(node4, 2, node6, 0, false).unwrap();

        let schedule = graph.compile_internal().unwrap();

        dbg!(&schedule);

        assert_eq!(schedule.schedule.len(), 7);
        // Node 4 needs at-least 8 buffers
        assert!(schedule.buffers.len() > 7);

        // First two nodes must be 1 and 2
        assert!(schedule.schedule[0].id == node0 || schedule.schedule[0].id == node1);
        assert!(schedule.schedule[1].id == node0 || schedule.schedule[1].id == node1);
        // Next two nodes must be 2 and 3
        assert!(schedule.schedule[2].id == node2 || schedule.schedule[2].id == node3);
        assert!(schedule.schedule[3].id == node2 || schedule.schedule[3].id == node3);
        // Next node must be 4
        assert_eq!(schedule.schedule[4].id, node4);
        // Last two nodes must be 5 and 6
        assert!(schedule.schedule[5].id == node5 || schedule.schedule[5].id == node6);
        assert!(schedule.schedule[6].id == node5 || schedule.schedule[6].id == node6);

        verify_edge(edge0, &graph, &schedule);
        verify_edge(edge1, &graph, &schedule);
        verify_edge(edge2, &graph, &schedule);
        verify_edge(edge3, &graph, &schedule);
        verify_edge(edge4, &graph, &schedule);
        verify_edge(edge5, &graph, &schedule);
        verify_edge(edge6, &graph, &schedule);

        verify_node(node0, &[], &schedule, &graph);
        verify_node(node1, &[true], &schedule, &graph);
        verify_node(node2, &[false, true], &schedule, &graph);
        verify_node(node3, &[true, false], &schedule, &graph);
        verify_node(node4, &[false, true, true, false, false], &schedule, &graph);
        verify_node(node5, &[false, true], &schedule, &graph);
        verify_node(node6, &[false], &schedule, &graph);
    }

    fn verify_node(
        node_id: NodeID,
        in_ports_that_should_clear: &[bool],
        schedule: &CompiledSchedule<256>,
        graph: &AudioGraph<(), 256>,
    ) {
        let node = graph.node_info(node_id).unwrap();
        let scheduled_node = schedule.schedule.iter().find(|&s| s.id == node_id).unwrap();

        assert_eq!(scheduled_node.id, node_id);
        assert_eq!(scheduled_node.input_buffers.len(), node.num_inputs as usize);
        assert_eq!(
            scheduled_node.output_buffers.len(),
            node.num_outputs as usize
        );

        assert_eq!(in_ports_that_should_clear.len(), node.num_inputs as usize);

        for (buffer, should_clear) in scheduled_node
            .input_buffers
            .iter()
            .zip(in_ports_that_should_clear)
        {
            assert_eq!(buffer.should_clear, *should_clear);
        }

        let mut buffer_alias_check: AHashSet<usize> = AHashSet::default();

        for buffer in scheduled_node.input_buffers.iter() {
            assert!(buffer_alias_check.insert(buffer.buffer_index));
        }

        for buffer in scheduled_node.output_buffers.iter() {
            assert!(buffer_alias_check.insert(buffer.buffer_index));
        }
    }

    fn verify_edge(edge_id: EdgeID, graph: &AudioGraph<(), 256>, schedule: &CompiledSchedule<256>) {
        let edge = graph.edge(edge_id).unwrap();

        let mut src_buffer_idx = None;
        let mut dst_buffer_idx = None;
        for node in schedule.schedule.iter() {
            if node.id == edge.src_node {
                src_buffer_idx = Some(node.output_buffers[edge.src_port.0 as usize].buffer_index);
                if dst_buffer_idx.is_some() {
                    break;
                }
            } else if node.id == edge.dst_node {
                dst_buffer_idx = Some(node.input_buffers[edge.dst_port.0 as usize].buffer_index);
                if src_buffer_idx.is_some() {
                    break;
                }
            }
        }

        let src_buffer_idx = src_buffer_idx.unwrap();
        let dst_buffer_idx = dst_buffer_idx.unwrap();

        assert_eq!(src_buffer_idx, dst_buffer_idx);
    }

    #[test]
    fn many_to_one_detection() {
        let mut graph = AudioGraph::<(), 256>::new(&AudioGraphConfig {
            num_graph_inputs: 2,
            num_graph_outputs: 1,
            ..Default::default()
        });

        let node1 = graph.graph_in_node();
        let node2 = graph.graph_out_node();

        graph.connect(node1, 0, node2, 0, false).unwrap();

        if let Err(AddEdgeError::InputPortAlreadyConnected(node_id, port_id)) =
            graph.connect(node1, OutPortIdx(1), node2, InPortIdx(0), false)
        {
            assert_eq!(node_id, node2);
            assert_eq!(port_id, InPortIdx(0));
        } else {
            panic!("expected error");
        }
    }

    #[test]
    fn cycle_detection() {
        let mut graph = AudioGraph::<(), 256>::new(&AudioGraphConfig {
            num_graph_inputs: 0,
            num_graph_outputs: 2,
            ..Default::default()
        });

        let node1 = graph.add_node(1, 1, DummyAudioNode);
        let node2 = graph.add_node(2, 1, DummyAudioNode);
        let node3 = graph.add_node(1, 1, DummyAudioNode);

        graph.connect(node1, 0, node2, 0, false).unwrap();
        graph.connect(node2, 0, node3, 0, false).unwrap();
        let edge3 = graph.connect(node3, 0, node1, 0, false).unwrap();

        assert!(graph.cycle_detected());

        graph.disconnect_by_edge_id(edge3);

        assert!(!graph.cycle_detected());

        graph.connect(node3, 0, node2, 1, false).unwrap();

        assert!(graph.cycle_detected());
    }
}
