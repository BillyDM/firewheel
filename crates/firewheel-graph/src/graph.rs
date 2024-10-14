mod compiler;
mod error;

use std::fmt::Debug;
use std::hash::Hash;

use ahash::{AHashMap, AHashSet};
use thunderdome::Arena;

use crate::basic_nodes::DummyAudioNode;
use firewheel_core::node::{AudioNode, AudioNodeProcessor};

pub(crate) use self::compiler::{CompiledSchedule, ScheduleHeapData};

pub use self::compiler::{Edge, EdgeID, InPortIdx, NodeEntry, OutPortIdx};
pub use self::error::{AddEdgeError, CompileGraphError};

/// A globally unique identifier for a node.
#[derive(Clone, Copy)]
pub struct NodeID {
    pub idx: thunderdome::Index,
    pub debug_name: &'static str,
}

impl NodeID {
    pub const DANGLING: Self = Self {
        idx: thunderdome::Index::DANGLING,
        debug_name: "dangling",
    };
}

impl Default for NodeID {
    fn default() -> Self {
        Self::DANGLING
    }
}

impl PartialEq for NodeID {
    fn eq(&self, other: &Self) -> bool {
        self.idx == other.idx
    }
}

impl Eq for NodeID {}

impl Ord for NodeID {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.idx.cmp(&other.idx)
    }
}

impl PartialOrd for NodeID {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Hash for NodeID {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.idx.hash(state);
    }
}

impl Debug for NodeID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}-{}-{}",
            self.debug_name,
            self.idx.slot(),
            self.idx.generation()
        )
    }
}

pub struct NodeWeight<C> {
    pub node: Box<dyn AudioNode<C>>,
    pub activated: bool,
}

#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
struct EdgeHash {
    pub src_node: NodeID,
    pub src_port: OutPortIdx,
    pub dst_node: NodeID,
    pub dst_port: InPortIdx,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioGraphConfig {
    pub num_graph_inputs: usize,
    pub num_graph_outputs: usize,
    pub initial_node_capacity: usize,
    pub initial_edge_capacity: usize,
}

impl Default for AudioGraphConfig {
    fn default() -> Self {
        Self {
            num_graph_inputs: 0,
            num_graph_outputs: 2,
            initial_node_capacity: 64,
            initial_edge_capacity: 256,
        }
    }
}

pub struct AudioGraph<C> {
    nodes: Arena<NodeEntry<NodeWeight<C>>>,
    edges: Arena<Edge>,
    connected_input_ports: AHashSet<(NodeID, InPortIdx)>,
    existing_edges: AHashMap<EdgeHash, EdgeID>,

    graph_in_id: NodeID,
    graph_out_id: NodeID,
    needs_compile: bool,

    nodes_to_remove_from_schedule: Vec<NodeID>,
    nodes_to_activate: Vec<NodeID>,
    active_nodes_to_remove: AHashMap<NodeID, NodeEntry<NodeWeight<C>>>,
}

impl<C: 'static> AudioGraph<C> {
    pub(crate) fn new(config: &AudioGraphConfig) -> Self {
        let mut nodes = Arena::with_capacity(config.initial_node_capacity);

        let graph_in_id = NodeID {
            idx: nodes.insert(NodeEntry::new(
                0,
                config.num_graph_inputs,
                NodeWeight {
                    node: Box::new(DummyAudioNode),
                    activated: false,
                },
            )),
            debug_name: "graph_in",
        };
        nodes[graph_in_id.idx].id = graph_in_id;

        let graph_out_id = NodeID {
            idx: nodes.insert(NodeEntry::new(
                config.num_graph_outputs,
                0,
                NodeWeight {
                    node: Box::new(DummyAudioNode),
                    activated: false,
                },
            )),
            debug_name: "graph_out",
        };
        nodes[graph_out_id.idx].id = graph_out_id;

        Self {
            nodes,
            edges: Arena::with_capacity(config.initial_edge_capacity),
            connected_input_ports: AHashSet::with_capacity(config.initial_edge_capacity),
            existing_edges: AHashMap::with_capacity(config.initial_edge_capacity),
            graph_in_id,
            graph_out_id,
            needs_compile: true,
            nodes_to_remove_from_schedule: Vec::new(),
            nodes_to_activate: vec![graph_in_id, graph_out_id],
            active_nodes_to_remove: AHashMap::with_capacity(config.initial_edge_capacity),
        }
    }

    /// Remove all existing nodes from the graph.
    pub fn reset(&mut self) {
        let nodes_to_remove = self
            .nodes
            .iter()
            .map(|(_, node_entry)| node_entry.id)
            .filter(|&id| id != self.graph_in_id && id != self.graph_out_id)
            .collect::<Vec<_>>();

        for node_id in nodes_to_remove {
            self.remove_node(node_id).unwrap();
        }
    }

    pub(crate) fn current_node_capacity(&self) -> usize {
        self.nodes.capacity()
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
    pub fn add_node(
        &mut self,
        num_inputs: usize,
        num_outputs: usize,
        node: impl Into<Box<dyn AudioNode<C>>>,
    ) -> NodeID {
        let node: Box<dyn AudioNode<C>> = node.into();
        let debug_name = node.debug_name();

        let new_id = NodeID {
            idx: self.nodes.insert(NodeEntry::new(
                num_inputs,
                num_outputs,
                NodeWeight {
                    node: node.into(),
                    activated: false,
                },
            )),
            debug_name,
        };
        self.nodes[new_id.idx].id = new_id;

        self.nodes_to_activate.push(new_id);

        self.needs_compile = true;

        new_id
    }

    /// Get an immutable reference to the node.
    ///
    /// This will return `None` if a node with the given ID does not
    /// exist in the graph.
    pub fn node(&self, node_id: NodeID) -> Option<&Box<dyn AudioNode<C>>> {
        self.nodes.get(node_id.idx).map(|n| &n.weight.node)
    }

    /// Get a mutable reference to the node.
    ///
    /// This will return `None` if a node with the given ID does not
    /// exist in the graph.
    pub fn node_mut(&mut self, node_id: NodeID) -> Option<&mut Box<dyn AudioNode<C>>> {
        self.nodes.get_mut(node_id.idx).map(|n| &mut n.weight.node)
    }

    /// Get info about a node.
    ///
    /// This will return `None` if a node with the given ID does not
    /// exist in the graph.
    pub fn node_info(&self, node_id: NodeID) -> Option<&NodeEntry<NodeWeight<C>>> {
        self.nodes.get(node_id.idx)
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

        let node_entry = self.nodes.remove(node_id.idx).ok_or(())?;

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
    pub fn nodes<'a>(&'a self) -> impl Iterator<Item = &'a NodeEntry<NodeWeight<C>>> {
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
    pub fn set_num_inputs(
        &mut self,
        node_id: NodeID,
        num_inputs: usize,
    ) -> Result<Vec<EdgeID>, ()> {
        if node_id == self.graph_in_id {
            return Err(());
        }

        let num_inputs = num_inputs as u32;

        let node_entry = self.nodes.get_mut(node_id.idx).ok_or(())?;

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

        self.nodes[node_id.idx].num_inputs = num_inputs;

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
        num_outputs: usize,
    ) -> Result<Vec<EdgeID>, ()> {
        if node_id == self.graph_out_id {
            return Err(());
        }

        let node_entry = self.nodes.get_mut(node_id.idx).ok_or(())?;

        let num_outputs = num_outputs as u32;

        let old_num_outputs = node_entry.num_outputs;
        let mut removed_edges = Vec::new();
        if num_outputs < old_num_outputs {
            for port_idx in num_outputs..old_num_outputs {
                removed_edges
                    .append(&mut self.remove_edges_with_output_port(node_id, OutPortIdx(port_idx)));
            }
        }

        self.nodes[node_id.idx].num_outputs = num_outputs;

        self.needs_compile = true;
        Ok(removed_edges)
    }

    /// Add a connection (edge) to the graph.
    ///
    /// * `src_node_id` - The ID of the source node.
    /// * `src_port_idx` - The index of the source port. This must be an output
    /// port on the source node.
    /// * `dst_node_id` - The ID of the destination node.
    /// * `dst_port_idx` - The index of the destination port. This must be an
    /// input port on the destination node.
    /// * `check_for_cycles` - If `true`, then this will run a check to
    /// see if adding this edge will create a cycle in the graph, and
    /// return an error if it does. Note, checking for cycles can be quite
    /// expensive, so avoid enabling this when calling this method many times
    /// in a row.
    ///
    /// If successful, this returns the globally unique identifier assigned
    /// to this edge.
    ///
    /// If this returns an error, then the audio graph has not been
    /// modified.
    pub fn connect(
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
            .get(src_node.idx)
            .ok_or(AddEdgeError::SrcNodeNotFound(src_node))?;
        let dst_node_entry = self
            .nodes
            .get(dst_node.idx)
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

        if src_node.idx == dst_node.idx {
            return Err(AddEdgeError::CycleDetected);
        }

        if self.existing_edges.contains_key(&EdgeHash {
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
        self.existing_edges.insert(
            EdgeHash {
                src_node,
                src_port,
                dst_node,
                dst_port,
            },
            new_edge_id,
        );

        if check_for_cycles {
            if self.cycle_detected() {
                self.edges.remove(new_edge_id.0);

                return Err(AddEdgeError::CycleDetected);
            }
        }

        self.needs_compile = true;

        Ok(new_edge_id)
    }

    /// Remove a connection (edge) from the graph.
    ///
    /// If the edge did not exist in the graph, then `false` will be
    /// returned.
    pub fn disconnect(
        &mut self,
        src_node: NodeID,
        src_port: impl Into<OutPortIdx>,
        dst_node: NodeID,
        dst_port: impl Into<InPortIdx>,
    ) -> bool {
        if let Some(edge_id) = self.existing_edges.remove(&EdgeHash {
            src_node,
            src_port: src_port.into(),
            dst_node,
            dst_port: dst_port.into(),
        }) {
            self.disconnect_by_edge_id(edge_id);
            true
        } else {
            false
        }
    }

    /// Remove a connection (edge) from the graph by the [EdgeID].
    ///
    /// If the edge did not exist in the graph, then `false` will be
    /// returned.
    pub fn disconnect_by_edge_id(&mut self, edge_id: EdgeID) -> bool {
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
            self.disconnect_by_edge_id(*edge_id);
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
            self.disconnect_by_edge_id(*edge_id);
        }

        edges_to_remove
    }

    pub fn cycle_detected(&mut self) -> bool {
        compiler::cycle_detected::<NodeWeight<C>>(
            &mut self.nodes,
            &mut self.edges,
            self.graph_in_id,
            self.graph_out_id,
        )
    }

    pub(crate) fn needs_compile(&self) -> bool {
        self.needs_compile
    }

    pub(crate) fn compile(
        &mut self,
        sample_rate: u32,
        max_block_frames: usize,
    ) -> Result<ScheduleHeapData<C>, CompileGraphError> {
        let schedule = self.compile_internal(max_block_frames)?;

        let mut new_node_processors = Vec::with_capacity(self.nodes_to_activate.len());
        for node_id in self.nodes_to_activate.iter() {
            if let Some(node_entry) = self.nodes.get_mut(node_id.idx) {
                match node_entry.weight.node.activate(
                    sample_rate,
                    max_block_frames,
                    node_entry.num_inputs as usize,
                    node_entry.num_outputs as usize,
                ) {
                    Ok(processor) => new_node_processors.push((*node_id, processor)),
                    Err(e) => {
                        for (n_id, processor) in new_node_processors.drain(..) {
                            self.nodes[n_id.idx].weight.node.deactivate(Some(processor));
                        }

                        return Err(CompileGraphError::NodeActivationFailed(*node_id, e));
                    }
                }
            }
        }

        let schedule_data = ScheduleHeapData::new(
            schedule,
            self.nodes_to_remove_from_schedule.clone(),
            new_node_processors,
        );

        self.needs_compile = false;
        self.nodes_to_activate.clear();
        self.nodes_to_remove_from_schedule.clear();

        log::debug!("compiled new audio graph: {:?}", &schedule_data);

        Ok(schedule_data)
    }

    fn compile_internal(
        &mut self,
        max_block_frames: usize,
    ) -> Result<CompiledSchedule, CompileGraphError> {
        assert!(max_block_frames > 0);

        compiler::compile(
            &mut self.nodes,
            &mut self.edges,
            self.graph_in_id,
            self.graph_out_id,
            max_block_frames,
        )
    }

    pub(crate) fn on_schedule_returned(&mut self, mut schedule_data: Box<ScheduleHeapData<C>>) {
        for (node_id, processor) in schedule_data.removed_node_processors.drain(..) {
            if let Some(mut node_entry) = self.active_nodes_to_remove.remove(&node_id) {
                node_entry.weight.node.deactivate(Some(processor));
                node_entry.weight.activated = false;
            } else if let Some(node_entry) = self.nodes.get_mut(node_id.idx) {
                if node_entry.weight.activated {
                    node_entry.weight.node.deactivate(Some(processor));
                    node_entry.weight.activated = false;

                    self.nodes_to_activate.push(node_id);
                }
            }
        }
    }

    pub(crate) fn on_processor_dropped(
        &mut self,
        mut nodes: Arena<Box<dyn AudioNodeProcessor<C>>>,
    ) {
        for (node_id, processor) in nodes.drain() {
            if let Some(node_entry) = self.nodes.get_mut(node_id) {
                if node_entry.weight.activated {
                    node_entry.weight.node.deactivate(Some(processor));
                    node_entry.weight.activated = false;
                }
            }
        }
    }

    pub(crate) fn deactivate(&mut self) {
        self.active_nodes_to_remove.clear();
        self.nodes_to_remove_from_schedule.clear();
        self.needs_compile = true;

        for (node_id, node_entry) in self.nodes.iter_mut() {
            if node_entry.weight.activated {
                node_entry.weight.node.deactivate(None);
                node_entry.weight.activated = false;
            }

            let debug_name = node_entry.weight.node.debug_name();

            self.nodes_to_activate.push(NodeID {
                idx: node_id,
                debug_name,
            });
        }
    }
}
