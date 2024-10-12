use firewheel::{
    basic_nodes::{
        beep_test::BeepTestNode, HardClipNode, MonoToStereoNode, StereoToMonoNode, SumNode,
        VolumeNode,
    },
    graph::{AddEdgeError, NodeID},
    node::AudioNode,
    UpdateStatus, DEFAULT_MAX_BLOCK_FRAMES,
};

use crate::ui::DynGuiAudioNode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    BeepTest,
    HardClip,
    MonoToStereo,
    StereoToMono,
    Sum,
    Volume,
}

pub struct AudioSystem {
    cx: Option<firewheel::ActiveCtx>,
}

impl AudioSystem {
    pub fn new() -> Self {
        let cx = firewheel::InactiveCtx::new(Default::default());

        Self {
            cx: Some(cx.activate(None, true, ()).unwrap()),
        }
    }

    pub fn remove_node(&mut self, node_id: NodeID) {
        if let Err(_) = self.cx.as_mut().unwrap().graph_mut().remove_node(node_id) {
            log::error!("Node already removed!");
        }
    }

    pub fn add_node(&mut self, node_type: NodeType) -> DynGuiAudioNode {
        let (node, num_inputs, num_outputs): (
            Box<dyn AudioNode<(), DEFAULT_MAX_BLOCK_FRAMES>>,
            usize,
            usize,
        ) = match node_type {
            NodeType::BeepTest => (Box::new(BeepTestNode::new(440.0, -12.0, true)), 0, 1),
            NodeType::HardClip => (Box::new(HardClipNode::new(0.0)), 2, 2),
            NodeType::MonoToStereo => (Box::new(MonoToStereoNode), 1, 2),
            NodeType::StereoToMono => (Box::new(StereoToMonoNode), 2, 1),
            NodeType::Sum => (Box::new(SumNode), 8, 2),
            NodeType::Volume => (Box::new(VolumeNode::new(100.0)), 2, 2),
        };

        let id = self
            .cx
            .as_mut()
            .unwrap()
            .graph_mut()
            .add_node(num_inputs, num_outputs, node);

        DynGuiAudioNode {
            id,
            num_inputs,
            num_outputs,
            node_type,
        }
    }

    pub fn connect(
        &mut self,
        src_node: NodeID,
        dst_node: NodeID,
        src_port: usize,
        dst_port: usize,
    ) -> Result<(), AddEdgeError> {
        self.cx
            .as_mut()
            .unwrap()
            .graph_mut()
            .add_edge(src_node, src_port, dst_node, dst_port, true)?;

        Ok(())
    }

    pub fn graph_in_node(&self) -> NodeID {
        self.cx.as_ref().unwrap().graph().graph_in_node()
    }

    pub fn graph_out_node(&self) -> NodeID {
        self.cx.as_ref().unwrap().graph().graph_out_node()
    }

    pub fn update(&mut self) -> bool {
        match self.cx.take().unwrap().update() {
            UpdateStatus::Ok { cx, graph_error } => {
                self.cx = Some(cx);

                if let Some(e) = graph_error {
                    log::error!("{}", e);
                }

                false
            }
            UpdateStatus::Deactivated {
                cx: _,
                user_cx: _,
                error_msg,
            } => {
                // TODO: Attempt to reconnect.
                log::error!("Stream disconnected: {:?}", error_msg);
                true
            }
        }
    }
}
