use eframe::{App, CreationContext};
use egui::{Color32, Id, Ui};
use egui_snarl::{
    ui::{AnyPins, PinInfo, SnarlStyle, SnarlViewer},
    InPin, OutPin, Snarl,
};

use crate::system::{AudioSystem, NodeType};

const CABLE_COLOR: Color32 = Color32::from_rgb(0xb0, 0x00, 0xb0);

enum AudioNode {
    #[allow(unused)]
    SystemIn,
    SystemOut,
    Dynamic(DynAudioNode),
}

pub struct DynAudioNode {
    pub id: firewheel::graph::NodeID,
    pub name: String,
    pub num_inputs: usize,
    pub num_outputs: usize,
}

struct DemoViewer<'a> {
    audio_system: &'a mut AudioSystem,
}

impl<'a> SnarlViewer<AudioNode> for DemoViewer<'a> {
    fn disconnect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<AudioNode>) {
        // TODO
    }

    fn connect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<AudioNode>) {
        let src_node = snarl.get_node(from.id.node).unwrap();
        let dst_node = snarl.get_node(to.id.node).unwrap();

        let src_node = match src_node {
            AudioNode::SystemIn => self.audio_system.graph_in_node(),
            AudioNode::SystemOut => self.audio_system.graph_out_node(),
            AudioNode::Dynamic(n) => n.id,
        };
        let dst_node = match dst_node {
            AudioNode::SystemIn => self.audio_system.graph_in_node(),
            AudioNode::SystemOut => self.audio_system.graph_out_node(),
            AudioNode::Dynamic(n) => n.id,
        };

        if let Err(e) = self
            .audio_system
            .connect(src_node, dst_node, from.id.output, to.id.input)
        {
            log::error!("{}", e);
            return;
        }

        snarl.connect(from.id, to.id);
    }

    fn title(&mut self, node: &AudioNode) -> String {
        match node {
            AudioNode::SystemIn => "System In".into(),
            AudioNode::SystemOut => "System Out".into(),
            AudioNode::Dynamic(n) => n.name.clone(),
        }
    }

    fn inputs(&mut self, node: &AudioNode) -> usize {
        match node {
            AudioNode::SystemIn => 0,
            AudioNode::SystemOut => 2,
            AudioNode::Dynamic(n) => n.num_inputs,
        }
    }

    fn outputs(&mut self, node: &AudioNode) -> usize {
        match node {
            AudioNode::SystemIn => 2,
            AudioNode::SystemOut => 0,
            AudioNode::Dynamic(n) => n.num_outputs,
        }
    }

    fn show_input(
        &mut self,
        _pin: &InPin,
        _ui: &mut Ui,
        _scale: f32,
        _snarl: &mut Snarl<AudioNode>,
    ) -> PinInfo {
        PinInfo::square().with_fill(CABLE_COLOR)
    }

    fn show_output(
        &mut self,
        _pin: &OutPin,
        _ui: &mut Ui,
        _scale: f32,
        _snarl: &mut Snarl<AudioNode>,
    ) -> PinInfo {
        PinInfo::square().with_fill(CABLE_COLOR)
    }

    fn has_graph_menu(&mut self, _pos: egui::Pos2, _snarl: &mut Snarl<AudioNode>) -> bool {
        true
    }

    fn show_graph_menu(
        &mut self,
        pos: egui::Pos2,
        ui: &mut Ui,
        _scale: f32,
        snarl: &mut Snarl<AudioNode>,
    ) {
        ui.label("Add node");
        if ui.button("Beep Test").clicked() {
            let node = self.audio_system.add_node(NodeType::BeepTest);
            snarl.insert_node(pos, AudioNode::Dynamic(node));
            ui.close_menu();
        }
        if ui.button("Hard Clip").clicked() {
            let node = self.audio_system.add_node(NodeType::HardClip);
            snarl.insert_node(pos, AudioNode::Dynamic(node));
            ui.close_menu();
        }
        if ui.button("Mono To Stereo").clicked() {
            let node = self.audio_system.add_node(NodeType::MonoToStereo);
            snarl.insert_node(pos, AudioNode::Dynamic(node));
            ui.close_menu();
        }
        if ui.button("Stereo To Mono").clicked() {
            let node = self.audio_system.add_node(NodeType::StereoToMono);
            snarl.insert_node(pos, AudioNode::Dynamic(node));
            ui.close_menu();
        }
        if ui.button("Sum").clicked() {
            let node = self.audio_system.add_node(NodeType::Sum);
            snarl.insert_node(pos, AudioNode::Dynamic(node));
            ui.close_menu();
        }
        if ui.button("Volume").clicked() {
            let node = self.audio_system.add_node(NodeType::Volume);
            snarl.insert_node(pos, AudioNode::Dynamic(node));
            ui.close_menu();
        }
    }

    fn has_dropped_wire_menu(&mut self, _src_pins: AnyPins, _snarl: &mut Snarl<AudioNode>) -> bool {
        false
    }

    fn has_node_menu(&mut self, _node: &AudioNode) -> bool {
        true
    }

    fn show_node_menu(
        &mut self,
        node: egui_snarl::NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        _scale: f32,
        snarl: &mut Snarl<AudioNode>,
    ) {
        match snarl.get_node(node).unwrap() {
            AudioNode::Dynamic(n) => {
                ui.label("Node menu");
                if ui.button("Remove").clicked() {
                    self.audio_system.remove_node(n.id);
                    snarl.remove_node(node);
                    ui.close_menu();
                }
            }
            _ => {}
        }
    }

    fn has_on_hover_popup(&mut self, _: &AudioNode) -> bool {
        false
    }
}

pub struct DemoApp {
    snarl: Snarl<AudioNode>,
    style: SnarlStyle,
    snarl_ui_id: Option<Id>,
    audio_system: AudioSystem,
}

impl DemoApp {
    pub fn new(cx: &CreationContext) -> Self {
        cx.egui_ctx.style_mut(|style| style.animation_time *= 10.0);

        let mut snarl = Snarl::new();
        let style = SnarlStyle::new();

        snarl.insert_node(egui::Pos2 { x: 0.0, y: 0.0 }, AudioNode::SystemOut);

        DemoApp {
            snarl,
            style,
            snarl_ui_id: None,
            audio_system: AudioSystem::new(),
        }
    }
}

impl App for DemoApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    ui.menu_button("File", |ui| {
                        if ui.button("Quit").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close)
                        }
                    });
                    ui.add_space(16.0);
                }

                egui::widgets::global_dark_light_mode_switch(ui);

                if ui.button("Clear All").clicked() {
                    self.snarl = Default::default();
                    self.snarl
                        .insert_node(egui::Pos2 { x: 0.0, y: 0.0 }, AudioNode::SystemOut);
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.snarl_ui_id = Some(ui.id());

            self.snarl.show(
                &mut DemoViewer {
                    audio_system: &mut self.audio_system,
                },
                &self.style,
                "snarl",
                ui,
            );
        });

        if self.audio_system.update() {
            // TODO: Don't panic.
            panic!("Audio system disconnected");
        }
    }
}
