use smallvec::SmallVec;
use std::{collections::VecDeque, rc::Rc};
use thunderdome::Arena;

use super::{error::CompileGraphError, NodeID};

mod schedule;

pub use schedule::{CompiledSchedule, ScheduleHeapData};
use schedule::{InBufferAssignment, OutBufferAssignment, ScheduledNode};

pub struct NodeEntry<N> {
    pub id: NodeID,
    /// The number of input ports used by the node
    pub num_inputs: u32,
    /// The number of output ports used by the node
    pub num_outputs: u32,
    pub weight: N,
    /// The edges connected to this node's input ports.
    incoming: SmallVec<[Edge; 4]>,
    /// The edges connected to this node's output ports.
    outgoing: SmallVec<[Edge; 4]>,
}

impl<N> NodeEntry<N> {
    pub fn new(num_inputs: usize, num_outputs: usize, weight: N) -> Self {
        Self {
            id: NodeID {
                idx: thunderdome::Index::DANGLING,
                debug_name: "",
            },
            num_inputs: num_inputs as u32,
            num_outputs: num_outputs as u32,
            weight,
            incoming: SmallVec::new(),
            outgoing: SmallVec::new(),
        }
    }
}

/// The index for an input port on a particular [Node].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InPortIdx(pub u32);

impl From<usize> for InPortIdx {
    fn from(value: usize) -> Self {
        Self(value as u32)
    }
}

/// The index for an output port on a particular [Node].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OutPortIdx(pub u32);

impl From<usize> for OutPortIdx {
    fn from(value: usize) -> Self {
        Self(value as u32)
    }
}

/// A globally unique identifier for an [Edge].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EdgeID(pub(super) thunderdome::Index);

/// An [Edge] is a connection from source node and port to a
/// destination node and port.
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
pub struct Edge {
    pub id: EdgeID,
    /// The ID of the source node used by this edge.
    pub src_node: NodeID,
    /// The ID of the source port used by this edge.
    pub src_port: OutPortIdx,
    /// The ID of the destination node used by this edge.
    pub dst_node: NodeID,
    /// The ID of the destination port used by this edge.
    pub dst_port: InPortIdx,
}

/// A reference to an abstract buffer during buffer allocation.
#[derive(Debug, Clone, Copy)]
struct BufferRef {
    /// The index of the buffer
    idx: usize,
    /// The generation, or the nth time this buffer has
    /// been assigned to a different edge in the graph.
    generation: usize,
}

/// An allocator for managing and reusing [BufferRef]s.
#[derive(Debug, Clone)]
struct BufferAllocator {
    /// A list of free buffers that may be reallocated
    free_list: Vec<BufferRef>,
    /// The maximum number of buffers used
    count: usize,
}

impl BufferAllocator {
    /// Create a new allocator, `num_types` defines the number
    /// of buffer types we may allocate.
    fn new(initial_capacity: usize) -> Self {
        Self {
            free_list: Vec::with_capacity(initial_capacity),
            count: 0,
        }
    }

    /// Acquire a new buffer
    fn acquire(&mut self) -> Rc<BufferRef> {
        let entry = self.free_list.pop().unwrap_or_else(|| {
            let idx = self.count;
            self.count += 1;
            BufferRef { idx, generation: 0 }
        });
        Rc::new(BufferRef {
            idx: entry.idx,
            generation: entry.generation,
        })
    }

    /// Release a BufferRef
    fn release(&mut self, buffer_ref: Rc<BufferRef>) {
        if Rc::strong_count(&buffer_ref) == 1 {
            self.free_list.push(BufferRef {
                idx: buffer_ref.idx,
                generation: buffer_ref.generation + 1,
            });
        }
    }

    /// Consume the allocator to return the maximum number of buffers used
    fn num_buffers(self) -> usize {
        self.count
    }
}

/// Main compilation algorithm
pub fn compile<'a, N, const MBF: usize>(
    nodes: &mut Arena<NodeEntry<N>>,
    edges: &mut Arena<Edge>,
    graph_in_id: NodeID,
    graph_out_id: NodeID,
) -> Result<CompiledSchedule<MBF>, CompileGraphError> {
    Ok(GraphIR::preprocess(nodes, edges, graph_in_id, graph_out_id)
        .sort_topologically(true)?
        .solve_buffer_requirements()?
        .merge())
}

pub fn cycle_detected<'a, N, const MBF: usize>(
    nodes: &'a mut Arena<NodeEntry<N>>,
    edges: &'a mut Arena<Edge>,
    graph_in_id: NodeID,
    graph_out_id: NodeID,
) -> bool {
    if let Err(CompileGraphError::CycleDetected) =
        GraphIR::<N, MBF>::preprocess(nodes, edges, graph_in_id, graph_out_id)
            .sort_topologically(false)
    {
        true
    } else {
        false
    }
}

/// Internal IR used by the compiler algorithm. Built incrementally
/// via the compiler passes.
struct GraphIR<'a, N, const MBF: usize> {
    nodes: &'a mut Arena<NodeEntry<N>>,
    edges: &'a mut Arena<Edge>,

    /// The topologically sorted schedule of the graph. Built internally.
    schedule: Vec<ScheduledNode>,
    /// The maximum number of buffers used.
    max_num_buffers: usize,

    graph_in_id: NodeID,
    graph_out_id: NodeID,
    graph_in_idx: usize,
    graph_out_idx: usize,
    max_in_buffers: usize,
    max_out_buffers: usize,
}

impl<'a, N, const MBF: usize> GraphIR<'a, N, MBF> {
    /// Construct a [GraphIR] instance from lists of nodes and edges, building
    /// up the adjacency table and creating an empty schedule.
    fn preprocess(
        nodes: &'a mut Arena<NodeEntry<N>>,
        edges: &'a mut Arena<Edge>,
        graph_in_id: NodeID,
        graph_out_id: NodeID,
    ) -> Self {
        assert!(nodes.contains(graph_in_id.idx));
        assert!(nodes.contains(graph_out_id.idx));

        for (_, node) in nodes.iter_mut() {
            assert!(node.num_inputs <= 64);
            assert!(node.num_outputs <= 64);

            node.incoming.clear();
            node.outgoing.clear();
        }

        for (_, edge) in edges.iter() {
            nodes[edge.src_node.idx].outgoing.push(*edge);
            nodes[edge.dst_node.idx].incoming.push(*edge);

            debug_assert_ne!(edge.src_node, graph_out_id);
            debug_assert_ne!(edge.dst_node, graph_in_id);
        }

        Self {
            nodes,
            edges,
            schedule: vec![],
            max_num_buffers: 0,
            graph_in_id,
            graph_out_id,
            graph_in_idx: 0,
            graph_out_idx: 0,
            max_in_buffers: 0,
            max_out_buffers: 0,
        }
    }

    /// Sort the nodes topologically using Kahn's algorithm.
    /// https://www.geeksforgeeks.org/topological-sorting-indegree-based-solution/
    fn sort_topologically(mut self, build_schedule: bool) -> Result<Self, CompileGraphError> {
        let mut in_degree = vec![0i32; self.nodes.capacity()];
        let mut queue = VecDeque::with_capacity(self.nodes.len());

        if build_schedule {
            self.schedule.reserve(self.nodes.len());
        }

        let mut num_visited = 0;

        // Calculate in-degree of each vertex
        for (_, node_entry) in self.nodes.iter() {
            for edge in node_entry.outgoing.iter() {
                in_degree[edge.dst_node.idx.slot() as usize] += 1;
            }
        }

        // Enqueue vertices with 0 in-degree
        for (_, node_entry) in self.nodes.iter() {
            if node_entry.incoming.is_empty() {
                queue.push_back(node_entry.id);
            }
        }

        // BFS traversal
        while let Some(node_id) = queue.pop_front() {
            num_visited += 1;

            let node_entry = &self.nodes[node_id.idx];

            // Reduce in-degree of adjacent vertices
            for edge in node_entry.outgoing.iter() {
                in_degree[edge.dst_node.idx.slot() as usize] -= 1;

                // If in-degree becomes 0, enqueue it
                if in_degree[edge.dst_node.idx.slot() as usize] == 0 {
                    queue.push_back(edge.dst_node);
                }
            }

            if build_schedule {
                if node_id == self.graph_in_id {
                    self.graph_in_idx = self.schedule.len();
                } else if node_id == self.graph_out_id {
                    self.graph_out_idx = self.schedule.len();
                }

                self.schedule.push(ScheduledNode::new(node_id));
            }
        }

        // If not all vertices are visited, cycle
        if num_visited != self.nodes.len() {
            return Err(CompileGraphError::CycleDetected);
        }

        Ok(self)
    }

    fn solve_buffer_requirements(mut self) -> Result<Self, CompileGraphError> {
        let mut allocator = BufferAllocator::new(64);
        let mut assignment_table: Arena<Rc<BufferRef>> =
            Arena::with_capacity(self.edges.capacity());
        let mut buffers_to_release: Vec<Rc<BufferRef>> = Vec::with_capacity(64);

        for entry in &mut self.schedule {
            // Collect the inputs to the algorithm, the incoming/outgoing edges of this node.

            let node_entry = &self.nodes[entry.id.idx];

            buffers_to_release.clear();
            if buffers_to_release.capacity()
                < node_entry.num_inputs as usize + node_entry.num_outputs as usize
            {
                buffers_to_release.reserve(
                    node_entry.num_inputs as usize + node_entry.num_outputs as usize
                        - buffers_to_release.capacity(),
                );
            }

            entry
                .input_buffers
                .reserve_exact(node_entry.num_inputs as usize);
            entry
                .output_buffers
                .reserve_exact(node_entry.num_outputs as usize);

            for port_idx in 0..node_entry.num_inputs as u32 {
                let port_idx = InPortIdx(port_idx);

                let edges: SmallVec<[&Edge; 4]> = node_entry
                    .incoming
                    .iter()
                    .filter(|edge| edge.dst_port == port_idx)
                    .collect();

                if edges.is_empty() {
                    // Case 1: The port is an input and it is unconnected. Acquire a buffer, and
                    //         assign it. The buffer must be cleared. Release the buffer once the
                    //         node assignments are done.
                    let buffer = allocator.acquire();
                    entry.input_buffers.push(InBufferAssignment {
                        buffer_index: buffer.idx,
                        generation: buffer.generation,
                        should_clear: true,
                    });
                    buffers_to_release.push(buffer);
                } else if edges.len() == 1 {
                    // Case 2: The port is an input, and has exactly one incoming edge. Lookup the
                    //         corresponding buffer and assign it. Buffer should not be cleared.
                    //         Release the buffer once the node assignments are done.
                    let buffer = assignment_table
                        .remove(edges[0].id.0)
                        .expect("No buffer assigned to edge!");
                    entry.input_buffers.push(InBufferAssignment {
                        buffer_index: buffer.idx,
                        generation: buffer.generation,
                        should_clear: false,
                    });
                    buffers_to_release.push(buffer);
                } else {
                    return Err(CompileGraphError::ManyToOneError(entry.id, port_idx));
                }
            }

            for port_idx in 0..node_entry.num_outputs as u32 {
                let port_idx = OutPortIdx(port_idx);

                let edges: SmallVec<[&Edge; 4]> = node_entry
                    .outgoing
                    .iter()
                    .filter(|edge| edge.src_port == port_idx)
                    .collect();

                if edges.is_empty() {
                    // Case 1: The port is an output and it is unconnected. Acquire a buffer and
                    //         assign it. The buffer does not need to be cleared. Release the
                    //         buffer once the node assignments are done.
                    let buffer = allocator.acquire();
                    entry.output_buffers.push(OutBufferAssignment {
                        buffer_index: buffer.idx,
                        generation: buffer.generation,
                    });
                    buffers_to_release.push(buffer);
                } else {
                    // Case 2: The port is an output. Acquire a buffer, and add to the assignment
                    //         table with any corresponding edge IDs. For each edge, update the
                    //         assigned buffer table. Buffer should not be cleared or released.
                    let buffer = allocator.acquire();
                    for edge in &edges {
                        assignment_table.insert_at(edge.id.0, Rc::clone(&buffer));
                    }
                    entry.output_buffers.push(OutBufferAssignment {
                        buffer_index: buffer.idx,
                        generation: buffer.generation,
                    });
                }
            }

            for buffer in buffers_to_release.drain(..) {
                allocator.release(buffer);
            }

            self.max_in_buffers = self.max_in_buffers.max(node_entry.num_inputs as usize);
            self.max_out_buffers = self.max_out_buffers.max(node_entry.num_outputs as usize);
        }

        self.max_num_buffers = allocator.num_buffers() as usize;
        Ok(self)
    }

    /// Merge the GraphIR into a [CompiledSchedule].
    fn merge(self) -> CompiledSchedule<MBF> {
        CompiledSchedule::new(
            self.schedule,
            self.graph_in_idx,
            self.graph_out_idx,
            self.max_num_buffers,
        )
    }
}
