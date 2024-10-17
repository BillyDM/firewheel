use firewheel::{
    basic_nodes::{
        beep_test::BeepTestNode, HardClipNode, MonoToStereoNode, StereoToMonoNode, SumNode,
        VolumeNode,
    },
    graph::{AddEdgeError, AudioGraph, NodeID},
    node::AudioNode,
    FirewheelCtx, UpdateStatus,
};

use crate::ui::GuiAudioNode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    BeepTest,
    HardClip,
    MonoToStereo,
    StereoToMono,
    SumMono4Ins,
    SumStereo2Ins,
    SumStereo4Ins,
    VolumeMono,
    VolumeStereo,
}

pub struct AudioSystem {
    cx: FirewheelCtx,
}

impl AudioSystem {
    pub fn new() -> Self {
        let mut cx = FirewheelCtx::new(Default::default());
        cx.activate(None, true, None).unwrap();

        Self { cx }
    }

    fn graph(&self) -> &AudioGraph {
        self.cx.graph()
    }

    fn graph_mut(&mut self) -> &mut AudioGraph {
        self.cx.graph_mut()
    }

    pub fn remove_node(&mut self, node_id: NodeID) {
        if let Err(_) = self.cx.graph_mut().remove_node(node_id) {
            log::error!("Node already removed!");
        }
    }

    pub fn add_node(&mut self, node_type: NodeType) -> GuiAudioNode {
        let (node, num_inputs, num_outputs): (Box<dyn AudioNode>, usize, usize) = match node_type {
            NodeType::BeepTest => (Box::new(BeepTestNode::new(440.0, -12.0, true)), 0, 1),
            NodeType::HardClip => (Box::new(HardClipNode::new(0.0)), 2, 2),
            NodeType::MonoToStereo => (Box::new(MonoToStereoNode), 1, 2),
            NodeType::StereoToMono => (Box::new(StereoToMonoNode), 2, 1),
            NodeType::SumMono4Ins => (Box::new(SumNode), 4, 1),
            NodeType::SumStereo2Ins => (Box::new(SumNode), 4, 2),
            NodeType::SumStereo4Ins => (Box::new(SumNode), 8, 2),
            NodeType::VolumeMono => (Box::new(VolumeNode::new(100.0)), 1, 1),
            NodeType::VolumeStereo => (Box::new(VolumeNode::new(100.0)), 2, 2),
        };

        let id = self.graph_mut().add_node(num_inputs, num_outputs, node);

        match node_type {
            NodeType::BeepTest => GuiAudioNode::BeepTest { id },
            NodeType::HardClip => GuiAudioNode::HardClip { id },
            NodeType::MonoToStereo => GuiAudioNode::MonoToStereo { id },
            NodeType::StereoToMono => GuiAudioNode::StereoToMono { id },
            NodeType::SumMono4Ins => GuiAudioNode::SumMono4Ins { id },
            NodeType::SumStereo2Ins => GuiAudioNode::SumStereo2Ins { id },
            NodeType::SumStereo4Ins => GuiAudioNode::SumStereo4Ins { id },
            NodeType::VolumeMono => GuiAudioNode::VolumeMono { id, percent: 100.0 },
            NodeType::VolumeStereo => GuiAudioNode::VolumeStereo { id, percent: 100.0 },
        }
    }

    pub fn connect(
        &mut self,
        src_node: NodeID,
        dst_node: NodeID,
        src_port: usize,
        dst_port: usize,
    ) -> Result<(), AddEdgeError> {
        self.graph_mut()
            .connect(src_node, src_port, dst_node, dst_port, true)?;

        Ok(())
    }

    pub fn disconnect(
        &mut self,
        src_node: NodeID,
        dst_node: NodeID,
        src_port: usize,
        dst_port: usize,
    ) {
        self.graph_mut()
            .disconnect(src_node, src_port, dst_node, dst_port);
    }

    pub fn graph_in_node(&self) -> NodeID {
        self.graph().graph_in_node()
    }

    pub fn graph_out_node(&self) -> NodeID {
        self.graph().graph_out_node()
    }

    pub fn is_activated(&self) -> bool {
        self.cx.is_activated()
    }

    pub fn update(&mut self) {
        match self.cx.update() {
            UpdateStatus::Inactive => {}
            UpdateStatus::Active { graph_error } => {
                if let Some(e) = graph_error {
                    log::error!("audio graph error: {}", e);
                }
            }
            UpdateStatus::Deactivated { error, .. } => {
                if let Some(e) = error {
                    log::error!("Stream disconnected: {}", e);
                } else {
                    log::error!("Stream disconnected");
                }
            }
        }
    }

    pub fn reset(&mut self) {
        self.graph_mut().reset();
    }

    pub fn set_volume(&mut self, node_id: NodeID, percent_volume: f32) {
        let volume_node = self
            .graph_mut()
            .node_mut(node_id)
            .unwrap()
            .downcast_mut::<VolumeNode>()
            .unwrap();

        volume_node.set_percent_volume(percent_volume);
    }
}
