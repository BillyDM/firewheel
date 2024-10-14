use eframe::{App, CreationContext};
use egui::{Color32, Id, Ui};
use egui_snarl::{
    ui::{AnyPins, PinInfo, SnarlStyle, SnarlViewer},
    InPin, InPinId, OutPin, OutPinId, Snarl,
};

use crate::system::{AudioSystem, NodeType};

const CABLE_COLOR: Color32 = Color32::from_rgb(0xb0, 0x00, 0xb0);

pub enum GuiAudioNode {
    #[allow(unused)]
    SystemIn,
    SystemOut,
    BeepTest {
        id: firewheel::graph::NodeID,
    },
    HardClip {
        id: firewheel::graph::NodeID,
    },
    MonoToStereo {
        id: firewheel::graph::NodeID,
    },
    StereoToMono {
        id: firewheel::graph::NodeID,
    },
    SumMono4Ins {
        id: firewheel::graph::NodeID,
    },
    SumStereo2Ins {
        id: firewheel::graph::NodeID,
    },
    SumStereo4Ins {
        id: firewheel::graph::NodeID,
    },
    VolumeMono {
        id: firewheel::graph::NodeID,
        percent: f32,
    },
    VolumeStereo {
        id: firewheel::graph::NodeID,
        percent: f32,
    },
}

impl GuiAudioNode {
    fn node_id(&self, audio_system: &AudioSystem) -> firewheel::graph::NodeID {
        match self {
            &Self::SystemIn => audio_system.graph_in_node(),
            &Self::SystemOut => audio_system.graph_out_node(),
            &Self::BeepTest { id } => id,
            &Self::HardClip { id } => id,
            &Self::MonoToStereo { id } => id,
            &Self::StereoToMono { id } => id,
            &Self::SumMono4Ins { id } => id,
            &Self::SumStereo2Ins { id } => id,
            &Self::SumStereo4Ins { id } => id,
            &Self::VolumeMono { id, .. } => id,
            &Self::VolumeStereo { id, .. } => id,
        }
    }

    fn title(&self) -> String {
        match self {
            &Self::SystemIn => "System In",
            &Self::SystemOut => "System Out",
            &Self::BeepTest { .. } => "Beep Test",
            &Self::HardClip { .. } => "Hard Clip",
            &Self::MonoToStereo { .. } => "Mono To Stereo",
            &Self::StereoToMono { .. } => "Stereo To Mono",
            &Self::SumMono4Ins { .. } => "Sum (Mono, 4 Ins)",
            &Self::SumStereo2Ins { .. } => "Sum (Stereo, 2 Ins)",
            &Self::SumStereo4Ins { .. } => "Sum (Stereo, 4 Ins)",
            &Self::VolumeMono { .. } => "Volume (Mono)",
            &Self::VolumeStereo { .. } => "Volume (Stereo)",
        }
        .into()
    }

    fn num_inputs(&self) -> usize {
        match self {
            &Self::SystemIn => 0,
            &Self::SystemOut => 2,
            &Self::BeepTest { .. } => 0,
            &Self::HardClip { .. } => 2,
            &Self::MonoToStereo { .. } => 1,
            &Self::StereoToMono { .. } => 2,
            &Self::SumMono4Ins { .. } => 4,
            &Self::SumStereo2Ins { .. } => 4,
            &Self::SumStereo4Ins { .. } => 8,
            &Self::VolumeMono { .. } => 1,
            &Self::VolumeStereo { .. } => 2,
        }
    }

    fn num_outputs(&self) -> usize {
        match self {
            &Self::SystemIn => 1,
            &Self::SystemOut => 0,
            &Self::BeepTest { .. } => 1,
            &Self::HardClip { .. } => 2,
            &Self::MonoToStereo { .. } => 2,
            &Self::StereoToMono { .. } => 1,
            &Self::SumMono4Ins { .. } => 1,
            &Self::SumStereo2Ins { .. } => 2,
            &Self::SumStereo4Ins { .. } => 2,
            &Self::VolumeMono { .. } => 1,
            &Self::VolumeStereo { .. } => 2,
        }
    }
}

struct DemoViewer<'a> {
    audio_system: &'a mut AudioSystem,
}

impl<'a> DemoViewer<'a> {
    fn remove_edge(&mut self, from: OutPinId, to: InPinId, snarl: &mut Snarl<GuiAudioNode>) {
        let Some(src_node) = snarl.get_node(from.node) else {
            return;
        };
        let Some(dst_node) = snarl.get_node(to.node) else {
            return;
        };
        let src_node = src_node.node_id(&self.audio_system);
        let dst_node = dst_node.node_id(&self.audio_system);

        self.audio_system
            .disconnect(src_node, dst_node, from.output, to.input);

        snarl.disconnect(from, to);
    }
}

impl<'a> SnarlViewer<GuiAudioNode> for DemoViewer<'a> {
    fn drop_inputs(&mut self, pin: &InPin, snarl: &mut Snarl<GuiAudioNode>) {
        for from in pin.remotes.iter() {
            self.remove_edge(*from, pin.id, snarl);
        }
    }

    fn drop_outputs(&mut self, pin: &OutPin, snarl: &mut Snarl<GuiAudioNode>) {
        for to in pin.remotes.iter() {
            self.remove_edge(pin.id, *to, snarl);
        }
    }

    fn disconnect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<GuiAudioNode>) {
        self.remove_edge(from.id, to.id, snarl);
    }

    fn connect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<GuiAudioNode>) {
        let src_node = snarl
            .get_node(from.id.node)
            .unwrap()
            .node_id(&self.audio_system);
        let dst_node = snarl
            .get_node(to.id.node)
            .unwrap()
            .node_id(&self.audio_system);

        if let Err(e) = self
            .audio_system
            .connect(src_node, dst_node, from.id.output, to.id.input)
        {
            log::error!("{}", e);
            return;
        }

        snarl.connect(from.id, to.id);
    }

    fn title(&mut self, node: &GuiAudioNode) -> String {
        node.title()
    }

    fn inputs(&mut self, node: &GuiAudioNode) -> usize {
        node.num_inputs()
    }

    fn outputs(&mut self, node: &GuiAudioNode) -> usize {
        node.num_outputs()
    }

    fn show_input(
        &mut self,
        _pin: &InPin,
        _ui: &mut Ui,
        _scale: f32,
        _snarl: &mut Snarl<GuiAudioNode>,
    ) -> PinInfo {
        PinInfo::square().with_fill(CABLE_COLOR)
    }

    fn show_output(
        &mut self,
        _pin: &OutPin,
        _ui: &mut Ui,
        _scale: f32,
        _snarl: &mut Snarl<GuiAudioNode>,
    ) -> PinInfo {
        PinInfo::square().with_fill(CABLE_COLOR)
    }

    fn has_graph_menu(&mut self, _pos: egui::Pos2, _snarl: &mut Snarl<GuiAudioNode>) -> bool {
        true
    }

    fn show_graph_menu(
        &mut self,
        pos: egui::Pos2,
        ui: &mut Ui,
        _scale: f32,
        snarl: &mut Snarl<GuiAudioNode>,
    ) {
        ui.label("Add node");
        if ui.button("Beep Test").clicked() {
            let node = self.audio_system.add_node(NodeType::BeepTest);
            snarl.insert_node(pos, node);
            ui.close_menu();
        }
        if ui.button("Hard Clip").clicked() {
            let node = self.audio_system.add_node(NodeType::HardClip);
            snarl.insert_node(pos, node);
            ui.close_menu();
        }
        if ui.button("Mono To Stereo").clicked() {
            let node = self.audio_system.add_node(NodeType::MonoToStereo);
            snarl.insert_node(pos, node);
            ui.close_menu();
        }
        if ui.button("Stereo To Mono").clicked() {
            let node = self.audio_system.add_node(NodeType::StereoToMono);
            snarl.insert_node(pos, node);
            ui.close_menu();
        }
        if ui.button("Sum (mono, 4 ins)").clicked() {
            let node = self.audio_system.add_node(NodeType::SumMono4Ins);
            snarl.insert_node(pos, node);
            ui.close_menu();
        }
        if ui.button("Sum (stereo, 2 ins)").clicked() {
            let node = self.audio_system.add_node(NodeType::SumStereo2Ins);
            snarl.insert_node(pos, node);
            ui.close_menu();
        }
        if ui.button("Sum (stereo, 4 ins)").clicked() {
            let node = self.audio_system.add_node(NodeType::SumStereo4Ins);
            snarl.insert_node(pos, node);
            ui.close_menu();
        }
        if ui.button("Volume (mono)").clicked() {
            let node = self.audio_system.add_node(NodeType::VolumeMono);
            snarl.insert_node(pos, node);
            ui.close_menu();
        }
        if ui.button("Volume (stereo)").clicked() {
            let node = self.audio_system.add_node(NodeType::VolumeStereo);
            snarl.insert_node(pos, node);
            ui.close_menu();
        }
    }

    fn has_dropped_wire_menu(
        &mut self,
        _src_pins: AnyPins,
        _snarl: &mut Snarl<GuiAudioNode>,
    ) -> bool {
        false
    }

    fn has_node_menu(&mut self, _node: &GuiAudioNode) -> bool {
        true
    }

    fn show_node_menu(
        &mut self,
        node: egui_snarl::NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        _scale: f32,
        snarl: &mut Snarl<GuiAudioNode>,
    ) {
        let n = snarl.get_node(node).unwrap();

        match &n {
            GuiAudioNode::SystemIn | GuiAudioNode::SystemOut => {}
            _ => {
                ui.label("Node menu");
                if ui.button("Remove").clicked() {
                    self.audio_system.remove_node(n.node_id(&self.audio_system));
                    snarl.remove_node(node);
                    ui.close_menu();
                }
            }
        }
    }

    fn has_on_hover_popup(&mut self, _: &GuiAudioNode) -> bool {
        false
    }

    fn has_body(&mut self, node: &GuiAudioNode) -> bool {
        match node {
            GuiAudioNode::VolumeMono { .. } | GuiAudioNode::VolumeStereo { .. } => true,
            _ => false,
        }
    }

    fn show_body(
        &mut self,
        node: egui_snarl::NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        _scale: f32,
        snarl: &mut Snarl<GuiAudioNode>,
    ) {
        match snarl.get_node_mut(node).unwrap() {
            GuiAudioNode::VolumeMono { id, percent, .. } => {
                if ui
                    .add(egui::Slider::new(percent, 0.0..=200.0).text("volume"))
                    .changed()
                {
                    self.audio_system.set_volume(*id, *percent);
                }
            }
            GuiAudioNode::VolumeStereo { id, percent, .. } => {
                if ui
                    .add(egui::Slider::new(percent, 0.0..=200.0).text("volume"))
                    .changed()
                {
                    self.audio_system.set_volume(*id, *percent);
                }
            }
            _ => {}
        }
    }
}

pub struct DemoApp {
    snarl: Snarl<GuiAudioNode>,
    style: SnarlStyle,
    snarl_ui_id: Option<Id>,
    audio_system: AudioSystem,
}

impl DemoApp {
    pub fn new(cx: &CreationContext) -> Self {
        cx.egui_ctx.style_mut(|style| style.animation_time *= 10.0);

        let mut snarl = Snarl::new();
        let style = SnarlStyle::new();

        snarl.insert_node(egui::Pos2 { x: 0.0, y: 0.0 }, GuiAudioNode::SystemOut);

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
                    self.audio_system.reset();

                    self.snarl = Default::default();
                    self.snarl
                        .insert_node(egui::Pos2 { x: 0.0, y: 0.0 }, GuiAudioNode::SystemOut);
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
