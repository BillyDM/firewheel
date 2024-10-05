mod compiler;
mod error;
mod executor;

use std::time::{Duration, Instant};

use ahash::{AHashMap, AHashSet};
use error::CompileGraphError;
use executor::{ExecutorToGraphMsg, GraphToExecutorMsg, ScheduleHeapData};
use thunderdome::Arena;

use compiler::CompiledSchedule;

pub use compiler::{Edge, EdgeID, InPortIdx, NodeEntry, NodeID, OutPortIdx};
pub use error::AddEdgeError;
pub use executor::AudioGraphExecutor;

use crate::{
    backend::PollStatus,
    node::{AudioNode, DummyAudioNode},
    AudioBackend, DEFAULT_MAX_BLOCK_FRAMES,
};

const CHANNEL_CAPACITY: usize = 256;
const CLOSE_STREAM_TIMEOUT: Duration = Duration::from_secs(5);
const CLOSE_STREAM_SLEEP_INTERVAL: Duration = Duration::from_millis(2);
const DEFAULT_NODE_CAPACITY: usize = 32;
const DEFAULT_EDGE_CAPACITY: usize = 128;

pub struct NodeWeight {
    pub node: Box<dyn AudioNode>,
    pub activated: bool,
}

#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
struct EdgeHash {
    pub src_node: NodeID,
    pub src_port: OutPortIdx,
    pub dst_node: NodeID,
    pub dst_port: InPortIdx,
}

struct Channel {
    // TODO: Do research on whether `rtrb` is compatible with
    // webassembly. If not, use conditional compilation to
    // use a different channel type when targeting webassembly.
    to_executor_tx: rtrb::Producer<GraphToExecutorMsg>,
    from_executor_rx: rtrb::Consumer<ExecutorToGraphMsg>,
}

/// The main server struct for Firewheel
pub struct FirewheelServer<B: AudioBackend> {
    nodes: Arena<NodeEntry<NodeWeight>>,
    edges: Arena<Edge>,
    connected_input_ports: AHashSet<(NodeID, InPortIdx)>,
    existing_edges: AHashSet<EdgeHash>,

    graph_in_id: NodeID,
    graph_out_id: NodeID,

    channel: Option<Channel>,
    stream_handle: Option<B::StreamHandle>,
    backend: B,

    needs_compile: bool,
    max_block_frames: usize,

    sample_rate: f64,
    nodes_to_remove_from_schedule: Vec<NodeID>,
    nodes_to_add_to_schedule: Vec<NodeID>,
    active_nodes_to_remove: AHashMap<NodeID, NodeEntry<NodeWeight>>,
}

impl<B: AudioBackend> FirewheelServer<B> {
    /// Construct a new [FirewheelServer]
    pub fn new(num_graph_inputs: u32, num_graph_outputs: u32) -> Self {
        Self::with_capacity(
            num_graph_inputs,
            num_graph_outputs,
            DEFAULT_NODE_CAPACITY,
            DEFAULT_EDGE_CAPACITY,
        )
    }

    /// Construct a new [FirewheelServer] with some initial allocated capacity.
    pub fn with_capacity(
        num_graph_inputs: u32,
        num_graph_outputs: u32,
        node_capacity: usize,
        edge_capacity: usize,
    ) -> Self {
        let mut nodes = Arena::with_capacity(node_capacity);

        let graph_in_id = NodeID(nodes.insert(NodeEntry::new(
            0,
            num_graph_inputs,
            NodeWeight {
                node: Box::new(DummyAudioNode),
                activated: false,
            },
        )));
        let graph_out_id = NodeID(nodes.insert(NodeEntry::new(
            num_graph_outputs,
            0,
            NodeWeight {
                node: Box::new(DummyAudioNode),
                activated: false,
            },
        )));

        Self {
            nodes,
            edges: Arena::with_capacity(edge_capacity),
            connected_input_ports: AHashSet::with_capacity(edge_capacity),
            existing_edges: AHashSet::with_capacity(edge_capacity),
            graph_in_id,
            graph_out_id,
            channel: None,
            stream_handle: None,
            backend: B::default(),
            needs_compile: false,
            max_block_frames: DEFAULT_MAX_BLOCK_FRAMES,
            sample_rate: 44100.0,
            nodes_to_remove_from_schedule: Vec::new(),
            nodes_to_add_to_schedule: vec![graph_in_id, graph_out_id],
            active_nodes_to_remove: AHashMap::with_capacity(node_capacity),
        }
    }

    pub fn start_stream(
        &mut self,
        num_stream_in_channels: u32,
        num_stream_out_channels: u32,
        max_block_frames: usize,
        sample_rate: f64,
        config: B::Config,
    ) -> Result<(), StartStreamError<B>> {
        if self.stream_handle.is_some() {
            return Err(StartStreamError::AlreadyStarted);
        }

        let (to_executor_tx, from_graph_rx) =
            rtrb::RingBuffer::<GraphToExecutorMsg>::new(CHANNEL_CAPACITY);
        let (to_graph_tx, from_executor_rx) =
            rtrb::RingBuffer::<ExecutorToGraphMsg>::new(CHANNEL_CAPACITY);

        self.channel = Some(Channel {
            to_executor_tx,
            from_executor_rx,
        });

        self.needs_compile = true;
        self.sample_rate = sample_rate;
        self.max_block_frames = max_block_frames;

        let executor = AudioGraphExecutor::new(
            from_graph_rx,
            to_graph_tx,
            self.nodes.capacity(),
            num_stream_in_channels,
            num_stream_out_channels,
            max_block_frames,
        );

        let stream_handle = match self.backend.start_stream(config, executor) {
            Ok(s) => s,
            Err(e) => {
                self.reset_stream_state();
                return Err(StartStreamError::BackendError(e));
            }
        };

        self.stream_handle = Some(stream_handle);

        self.compile_and_send_schedule()?;

        Ok(())
    }

    /// Close the stream
    ///
    /// This will block the thread until the stream is successfully closed.
    pub fn close_stream(&mut self) {
        if self.channel.is_none() {
            self.reset_stream_state();
            return;
        }

        let mut stopped = false;
        let mut dropped = false;

        let start = Instant::now();

        loop {
            if let Err(_) = self
                .channel
                .as_mut()
                .unwrap()
                .to_executor_tx
                .push(GraphToExecutorMsg::Stop)
            {
                log::error!("Audio graph message buffer is full");

                // TODO: I don't think sleep is supported in WASM, so we will
                // need to figure out something if that's the case.
                std::thread::sleep(CLOSE_STREAM_SLEEP_INTERVAL);

                if start.elapsed() > CLOSE_STREAM_TIMEOUT {
                    log::error!("Timed out trying to send stop message to audio graph executor");
                    break;
                }
            } else {
                break;
            }
        }

        while !dropped {
            if stopped {
                // The audio graph has successfully stopped processing. We
                // can now safely close the audio stream (dropping the handle
                // automatically closes the stream).
                self.stream_handle = None;
            }

            self.update_internal(&mut stopped, &mut dropped);

            // TODO: I don't think sleep is supported in WASM, so we will
            // need to figure out something if that's the case.
            std::thread::sleep(CLOSE_STREAM_SLEEP_INTERVAL);

            if start.elapsed() > CLOSE_STREAM_TIMEOUT {
                log::error!("Timed out waiting for audio stream to close");
                dropped = true;
            }
        }

        self.reset_stream_state();
    }

    fn reset_stream_state(&mut self) {
        self.channel = None;
        self.stream_handle = None;
        self.active_nodes_to_remove.clear();
        self.nodes_to_remove_from_schedule.clear();
        self.nodes_to_add_to_schedule.clear();
        self.needs_compile = true;

        for (node_id, node_entry) in self.nodes.iter_mut() {
            if node_entry.weight.activated {
                node_entry.weight.node.deactivate(None);
                node_entry.weight.activated = false;
            }

            self.nodes_to_add_to_schedule.push(NodeID(node_id));
        }
    }

    // TODO: Return status
    /// Update the audio graph.
    pub fn update(&mut self) {
        let mut stopped = false;
        let mut dropped = false;

        self.update_internal(&mut stopped, &mut dropped);

        if stopped || dropped {
            self.reset_stream_state();
        }

        if let Some(stream_handle) = &self.stream_handle {
            if let PollStatus::Err {
                msg,
                can_close_gracefully,
            } = self.backend.poll_for_errors(stream_handle)
            {
                log::error!("Audio stream error: {}", msg);

                if can_close_gracefully {
                    self.close_stream();
                } else {
                    self.reset_stream_state();
                }
            }
        }

        // TODO: Parameter stuff
    }

    fn update_internal(&mut self, stopped: &mut bool, dropped: &mut bool) {
        let Some(channel) = &mut self.channel else {
            return;
        };

        while let Ok(msg) = channel.from_executor_rx.pop() {
            match msg {
                ExecutorToGraphMsg::ReturnSchedule(mut schedule_data) => {
                    for (node_id, processor) in schedule_data.removed_node_processors.drain(..) {
                        if let Some(mut node_entry) = self.active_nodes_to_remove.remove(&node_id) {
                            node_entry.weight.node.deactivate(Some(processor));
                        }
                    }
                }
                ExecutorToGraphMsg::Stopped => *stopped = true,
                ExecutorToGraphMsg::Dropped { mut nodes, .. } => {
                    for (node_id, processor) in nodes.drain() {
                        if let Some(node_entry) = self.nodes.get_mut(node_id) {
                            if node_entry.weight.activated {
                                node_entry.weight.node.deactivate(Some(processor));
                                node_entry.weight.activated = false;
                            }
                        }
                    }

                    *dropped = true;
                }
            }
        }
    }

    /// The ID of the graph input node
    pub fn graph_in_node(&self) -> NodeID {
        self.graph_in_id
    }

    /// The ID of the graph output node
    pub fn graph_out_node(&self) -> NodeID {
        self.graph_out_id
    }

    /// Add a new [Node] the the audio graph.
    ///
    /// This will return the globally unique ID assigned to this node.
    pub fn add_node(&mut self, num_inputs: u32, num_outputs: u32, node: impl AudioNode) -> NodeID {
        self.needs_compile = true;

        let new_id = NodeID(self.nodes.insert(NodeEntry::new(
            num_inputs,
            num_outputs,
            NodeWeight {
                node: Box::new(node),
                activated: false,
            },
        )));
        self.nodes[new_id.0].id = new_id;

        self.nodes_to_add_to_schedule.push(new_id);

        new_id
    }

    /// Get an immutable reference to the node.
    ///
    /// This will return `None` if a node with the given ID does not
    /// exist in the graph.
    pub fn node(&self, node_id: NodeID) -> Option<&Box<dyn AudioNode>> {
        self.nodes.get(node_id.0).map(|n| &n.weight.node)
    }

    /// Get a mutable reference to the node.
    ///
    /// This will return `None` if a node with the given ID does not
    /// exist in the graph.
    pub fn node_mut(&mut self, node_id: NodeID) -> Option<&mut Box<dyn AudioNode>> {
        self.nodes.get_mut(node_id.0).map(|n| &mut n.weight.node)
    }

    /// Get info about a node.
    ///
    /// This will return `None` if a node with the given ID does not
    /// exist in the graph.
    pub fn node_info(&self, node_id: NodeID) -> Option<&NodeEntry<NodeWeight>> {
        self.nodes.get(node_id.0)
    }

    /// Remove the given node from the graph.
    ///
    /// This will automatically remove all edges from the graph that
    /// were connected to this node.
    ///
    /// On success, this returns a list of all edges that were removed
    /// from the graph as a result of removing this node.
    ///
    /// This will return an error if a node with the given ID does not
    /// exist in the graph, or if the ID is of the graph input or graph
    /// output node.
    pub fn remove_node(&mut self, node_id: NodeID) -> Result<Vec<EdgeID>, ()> {
        if node_id == self.graph_in_id || node_id == self.graph_out_id {
            return Err(());
        }

        let node_entry = self.nodes.remove(node_id.0).ok_or(())?;

        let mut removed_edges: Vec<EdgeID> = Vec::new();

        for port_idx in 0..node_entry.num_inputs {
            removed_edges
                .append(&mut self.remove_edges_with_input_port(node_id, InPortIdx(port_idx)));
        }
        for port_idx in 0..node_entry.num_outputs {
            removed_edges
                .append(&mut self.remove_edges_with_output_port(node_id, OutPortIdx(port_idx)));
        }

        for port_idx in 0..node_entry.num_inputs {
            self.connected_input_ports
                .remove(&(node_id, InPortIdx(port_idx)));
        }

        self.nodes_to_remove_from_schedule.push(node_id);

        if node_entry.weight.activated {
            self.active_nodes_to_remove.insert(node_id, node_entry);
        }

        self.needs_compile = true;
        Ok(removed_edges)
    }

    /// Get a list of all the existing nodes in the graph.
    pub fn nodes<'a>(&'a self) -> impl Iterator<Item = &'a NodeEntry<NodeWeight>> {
        self.nodes.iter().map(|(_, n)| n)
    }

    /// Get a list of all the existing edges in the graph.
    pub fn edges<'a>(&'a self) -> impl Iterator<Item = &'a Edge> {
        self.edges.iter().map(|(_, e)| e)
    }

    /// Set the number of input ports for a particular node in the graph.
    ///
    /// This will return an error if a node with the given ID does not
    /// exist in the graph, or if the ID is of the graph input node.
    pub fn set_num_inputs(&mut self, node_id: NodeID, num_inputs: u32) -> Result<Vec<EdgeID>, ()> {
        if node_id == self.graph_in_id {
            return Err(());
        }

        let node_entry = self.nodes.get_mut(node_id.0).ok_or(())?;

        let old_num_inputs = node_entry.num_inputs;
        let mut removed_edges = Vec::new();
        if num_inputs < old_num_inputs {
            for port_idx in num_inputs..old_num_inputs {
                removed_edges
                    .append(&mut self.remove_edges_with_input_port(node_id, InPortIdx(port_idx)));
                self.connected_input_ports
                    .remove(&(node_id, InPortIdx(port_idx)));
            }
        }

        self.nodes[node_id.0].num_inputs = num_inputs;

        self.needs_compile = true;
        Ok(removed_edges)
    }

    /// Set the number of output ports for a particular node in the graph.
    ///
    /// This will return an error if a node with the given ID does not
    /// exist in the graph, or if the ID is of the graph output node.
    pub fn set_num_outputs(
        &mut self,
        node_id: NodeID,
        num_outputs: u32,
    ) -> Result<Vec<EdgeID>, ()> {
        if node_id == self.graph_out_id {
            return Err(());
        }

        let node_entry = self.nodes.get_mut(node_id.0).ok_or(())?;

        let old_num_outputs = node_entry.num_outputs;
        let mut removed_edges = Vec::new();
        if num_outputs < old_num_outputs {
            for port_idx in num_outputs..old_num_outputs {
                removed_edges
                    .append(&mut self.remove_edges_with_output_port(node_id, OutPortIdx(port_idx)));
            }
        }

        self.nodes[node_id.0].num_outputs = num_outputs;

        self.needs_compile = true;
        Ok(removed_edges)
    }

    /// Add an [Edge] (port connection) to the graph.
    ///
    /// * `src_node_id` - The ID of the source node.
    /// * `src_port_idx` - The index of the source port. This must be an output
    /// port on the source node.
    /// * `dst_node_id` - The ID of the destination node.
    /// * `dst_port_idx` - The index of the destination port. This must be an
    /// input port on the destination node.
    /// * `check_for_cycles` - If `true`, then this will run a check to
    /// see if adding this edge will create a cycle in the graph, and
    /// return an error if it does.
    ///
    /// If successful, this returns the globally unique identifier assigned
    /// to this edge.
    ///
    /// If this returns an error, then the audio graph has not been
    /// modified.
    pub fn add_edge(
        &mut self,
        src_node: NodeID,
        src_port: impl Into<OutPortIdx>,
        dst_node: NodeID,
        dst_port: impl Into<InPortIdx>,
        check_for_cycles: bool,
    ) -> Result<EdgeID, AddEdgeError> {
        let src_port: OutPortIdx = src_port.into();
        let dst_port: InPortIdx = dst_port.into();

        let src_node_entry = self
            .nodes
            .get(src_node.0)
            .ok_or(AddEdgeError::SrcNodeNotFound(src_node))?;
        let dst_node_entry = self
            .nodes
            .get(dst_node.0)
            .ok_or(AddEdgeError::DstNodeNotFound(dst_node))?;

        if src_port.0 >= src_node_entry.num_outputs {
            return Err(AddEdgeError::OutPortOutOfRange {
                node: src_node,
                port_idx: src_port,
                num_out_ports: src_node_entry.num_outputs,
            });
        }
        if dst_port.0 >= dst_node_entry.num_inputs {
            return Err(AddEdgeError::InPortOutOfRange {
                node: dst_node,
                port_idx: dst_port,
                num_in_ports: dst_node_entry.num_inputs,
            });
        }

        if src_node.0 == dst_node.0 {
            return Err(AddEdgeError::CycleDetected);
        }

        if !self.existing_edges.insert(EdgeHash {
            src_node,
            src_port,
            dst_node,
            dst_port,
        }) {
            return Err(AddEdgeError::EdgeAlreadyExists);
        }

        if !self.connected_input_ports.insert((dst_node, dst_port)) {
            return Err(AddEdgeError::InputPortAlreadyConnected(dst_node, dst_port));
        }

        let new_edge_id = EdgeID(self.edges.insert(Edge {
            id: EdgeID(thunderdome::Index::DANGLING),
            src_node,
            src_port,
            dst_node,
            dst_port,
        }));
        self.edges[new_edge_id.0].id = new_edge_id;

        if check_for_cycles {
            if self.cycle_detected() {
                self.edges.remove(new_edge_id.0);

                return Err(AddEdgeError::CycleDetected);
            }
        }

        self.needs_compile = true;

        Ok(new_edge_id)
    }

    /// Remove the given [Edge] (port connection) from the graph.
    ///
    /// If the edge did not exist in the graph, then `false` will be
    /// returned.
    pub fn remove_edge(&mut self, edge_id: EdgeID) -> bool {
        if let Some(edge) = self.edges.remove(edge_id.0) {
            self.existing_edges.remove(&EdgeHash {
                src_node: edge.src_node,
                src_port: edge.src_port,
                dst_node: edge.dst_node,
                dst_port: edge.dst_port,
            });
            self.connected_input_ports
                .remove(&(edge.dst_node, edge.dst_port));

            self.needs_compile = true;

            true
        } else {
            false
        }
    }

    /// Get information about the given [Edge]
    pub fn edge(&self, edge_id: EdgeID) -> Option<&Edge> {
        self.edges.get(edge_id.0)
    }

    fn remove_edges_with_input_port(
        &mut self,
        node_id: NodeID,
        port_idx: InPortIdx,
    ) -> Vec<EdgeID> {
        let mut edges_to_remove: Vec<EdgeID> = Vec::new();

        // Remove all existing edges which have this port.
        for (edge_id, edge) in self.edges.iter() {
            if edge.dst_node == node_id && edge.dst_port == port_idx {
                edges_to_remove.push(EdgeID(edge_id));
            }
        }

        for edge_id in edges_to_remove.iter() {
            self.remove_edge(*edge_id);
        }

        edges_to_remove
    }

    fn remove_edges_with_output_port(
        &mut self,
        node_id: NodeID,
        port_idx: OutPortIdx,
    ) -> Vec<EdgeID> {
        let mut edges_to_remove: Vec<EdgeID> = Vec::new();

        // Remove all existing edges which have this port.
        for (edge_id, edge) in self.edges.iter() {
            if edge.src_node == node_id && edge.src_port == port_idx {
                edges_to_remove.push(EdgeID(edge_id));
            }
        }

        for edge_id in edges_to_remove.iter() {
            self.remove_edge(*edge_id);
        }

        edges_to_remove
    }

    pub fn cycle_detected(&mut self) -> bool {
        compiler::cycle_detected(
            &mut self.nodes,
            &mut self.edges,
            self.graph_in_id,
            self.graph_out_id,
            self.max_block_frames,
        )
    }

    fn compile_and_send_schedule(&mut self) -> Result<(), CompileGraphError> {
        if !self.needs_compile || self.channel.is_none() {
            return Ok(());
        }

        let schedule = self.compile()?;

        let mut nodes_to_add = Vec::with_capacity(self.nodes_to_add_to_schedule.len());
        for node_id in self.nodes_to_add_to_schedule.iter() {
            if let Some(node_entry) = self.nodes.get_mut(node_id.0) {
                match node_entry.weight.node.activate(
                    self.sample_rate,
                    self.max_block_frames,
                    node_entry.num_inputs as usize,
                    node_entry.num_outputs as usize,
                ) {
                    Ok(processor) => nodes_to_add.push((*node_id, processor)),
                    Err(e) => {
                        for (n_id, processor) in nodes_to_add.drain(..) {
                            self.nodes[n_id.0].weight.node.deactivate(Some(processor));
                        }

                        return Err(CompileGraphError::NodeActivationFailed(*node_id, e));
                    }
                }
            }
        }

        let schedule_data = ScheduleHeapData::new(
            schedule,
            self.nodes_to_remove_from_schedule.clone(),
            nodes_to_add,
        );

        self.channel
            .as_mut()
            .unwrap()
            .to_executor_tx
            .push(GraphToExecutorMsg::NewSchedule(schedule_data))
            .map_err(|_| CompileGraphError::MessageChannelFull)?;

        self.needs_compile = false;
        self.nodes_to_add_to_schedule.clear();
        self.nodes_to_remove_from_schedule.clear();

        // TODO

        Ok(())
    }

    /// Compile the graph into a schedule.
    fn compile(&mut self) -> Result<CompiledSchedule, CompileGraphError> {
        compiler::compile(
            &mut self.nodes,
            &mut self.edges,
            self.graph_in_id,
            self.graph_out_id,
            self.max_block_frames,
        )
    }
}

impl<B: AudioBackend> Drop for FirewheelServer<B> {
    fn drop(&mut self) {
        self.close_stream();
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StartStreamError<B: AudioBackend> {
    #[error("Audio stream has already been started")]
    AlreadyStarted,
    #[error("Backend error: {0}")]
    BackendError(B::StartStreamError),
    #[error("Graph error: {0}")]
    GraphError(#[from] CompileGraphError),
}
