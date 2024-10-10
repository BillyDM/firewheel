use firewheel_core::{
    node::{AudioNode, AudioNodeInfo, AudioNodeProcessor, ProcInfo},
    BlockFrames,
};

pub struct BeepTestNode {
    freq_hz: f32,
    gain: f32,
}

impl BeepTestNode {
    pub fn new(freq_hz: f32, gain_db: f32) -> Self {
        let freq_hz = freq_hz.clamp(20.0, 20_000.0);
        let gain = firewheel_core::util::db_to_gain_clamped_neg_100_db(gain_db).clamp(0.0, 1.0);

        Self { freq_hz, gain }
    }
}

impl<C, const MBF: usize> AudioNode<C, MBF> for BeepTestNode {
    fn info(&self) -> AudioNodeInfo {
        AudioNodeInfo {
            num_min_supported_inputs: 0,
            num_max_supported_inputs: 0,
            num_min_supported_outputs: 1,
            num_max_supported_outputs: 64,
        }
    }

    fn activate(
        &mut self,
        sample_rate: u32,
        _num_inputs: usize,
        _num_outputs: usize,
    ) -> Result<Box<dyn AudioNodeProcessor<C, MBF>>, Box<dyn std::error::Error>> {
        Ok(Box::new(BeepTestProcessor {
            phasor: 0.0,
            phasor_inc: self.freq_hz / sample_rate as f32,
            gain: self.gain,
        }))
    }
}

struct BeepTestProcessor {
    phasor: f32,
    phasor_inc: f32,
    gain: f32,
}

impl<C, const MBF: usize> AudioNodeProcessor<C, MBF> for BeepTestProcessor {
    fn process(
        &mut self,
        frames: BlockFrames<MBF>,
        _proc_info: ProcInfo<C>,
        _inputs: &[&[f32; MBF]],
        outputs: &mut [&mut [f32; MBF]],
    ) {
        let Some((out1, outputs)) = outputs.split_first_mut() else {
            return;
        };

        let frames = frames.get();

        for s in out1[..frames].iter_mut() {
            *s = (self.phasor * std::f32::consts::TAU).sin() * self.gain;
            self.phasor = (self.phasor + self.phasor_inc).fract();
        }

        for out2 in outputs.iter_mut() {
            out2[..frames].copy_from_slice(&out1[..frames]);
        }
    }
}
