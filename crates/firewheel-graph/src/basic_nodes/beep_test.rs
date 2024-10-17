use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use firewheel_core::node::{AudioNode, AudioNodeInfo, AudioNodeProcessor, ProcInfo};

pub struct BeepTestNode {
    enabled: Arc<AtomicBool>,
    freq_hz: f32,
    gain: f32,
}

impl BeepTestNode {
    pub fn new(freq_hz: f32, gain_db: f32, enabled: bool) -> Self {
        let freq_hz = freq_hz.clamp(20.0, 20_000.0);
        let gain = firewheel_core::util::db_to_gain_clamped_neg_100_db(gain_db).clamp(0.0, 1.0);

        Self {
            freq_hz,
            gain,
            enabled: Arc::new(AtomicBool::new(enabled)),
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }
}

impl AudioNode for BeepTestNode {
    fn debug_name(&self) -> &'static str {
        "beep_test"
    }

    fn info(&self) -> AudioNodeInfo {
        AudioNodeInfo {
            num_min_supported_outputs: 1,
            num_max_supported_outputs: 64,
            ..Default::default()
        }
    }

    fn activate(
        &mut self,
        sample_rate: u32,
        _max_block_frames: usize,
        _num_inputs: usize,
        _num_outputs: usize,
    ) -> Result<Box<dyn AudioNodeProcessor>, Box<dyn std::error::Error>> {
        Ok(Box::new(BeepTestProcessor {
            enabled: Arc::clone(&self.enabled),
            phasor: 0.0,
            phasor_inc: self.freq_hz / sample_rate as f32,
            gain: self.gain,
        }))
    }
}

struct BeepTestProcessor {
    enabled: Arc<AtomicBool>,
    phasor: f32,
    phasor_inc: f32,
    gain: f32,
}

impl AudioNodeProcessor for BeepTestProcessor {
    fn process(
        &mut self,
        frames: usize,
        _inputs: &[&[f32]],
        outputs: &mut [&mut [f32]],
        proc_info: ProcInfo,
    ) {
        let Some((out1, outputs)) = outputs.split_first_mut() else {
            return;
        };

        if !self.enabled.load(Ordering::Relaxed) {
            firewheel_core::util::clear_all_outputs(frames, outputs, proc_info.out_silence_mask);
            return;
        }

        for s in out1[..frames].iter_mut() {
            *s = (self.phasor * std::f32::consts::TAU).sin() * self.gain;
            self.phasor = (self.phasor + self.phasor_inc).fract();
        }

        for out2 in outputs.iter_mut() {
            out2[..frames].copy_from_slice(&out1[..frames]);
        }
    }
}

impl Into<Box<dyn AudioNode>> for BeepTestNode {
    fn into(self) -> Box<dyn AudioNode> {
        Box::new(self)
    }
}
