use firewheel_core::node::{AudioNode, AudioNodeInfo, AudioNodeProcessor, ProcInfo};

pub struct BeepTestNode {
    freq_hz: f32,
    gain_amp: f32,
}

impl BeepTestNode {
    pub fn new(freq_hz: f32, gain_db: f32) -> Self {
        let freq_hz = freq_hz.clamp(20.0, 20_000.0);
        let gain_amp = firewheel_core::util::db_to_amp_clamped_neg_100_db(gain_db).clamp(0.0, 1.0);

        Self { freq_hz, gain_amp }
    }
}

impl<C> AudioNode<C> for BeepTestNode {
    fn info(&self) -> AudioNodeInfo {
        AudioNodeInfo {
            num_min_supported_inputs: 0,
            num_max_supported_inputs: 0,
            num_min_supported_outputs: 1,
            num_max_supported_outputs: u32::MAX,
        }
    }

    fn activate(
        &mut self,
        sample_rate: u32,
        _max_block_frames: usize,
        _num_inputs: usize,
        _num_outputs: usize,
    ) -> Result<Box<dyn AudioNodeProcessor<C>>, Box<dyn std::error::Error>> {
        Ok(Box::new(BeepTestProcessor {
            phasor: 0.0,
            phasor_inc: self.freq_hz / sample_rate as f32,
            gain_amp: self.gain_amp,
        }))
    }
}

struct BeepTestProcessor {
    phasor: f32,
    phasor_inc: f32,
    gain_amp: f32,
}

impl<C> AudioNodeProcessor<C> for BeepTestProcessor {
    fn process(
        &mut self,
        _frames: usize,
        _proc_info: ProcInfo<C>,
        _inputs: &[&[f32]],
        outputs: &mut [&mut [f32]],
    ) {
        let Some((out1, outputs)) = outputs.split_first_mut() else {
            return;
        };

        for s in out1.iter_mut() {
            *s = (self.phasor * std::f32::consts::TAU).sin() * self.gain_amp;
            self.phasor = (self.phasor + self.phasor_inc).fract();
        }

        for out2 in outputs.iter_mut() {
            out2.copy_from_slice(out1);
        }
    }
}
