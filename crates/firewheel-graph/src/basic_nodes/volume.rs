use atomic_float::AtomicF32;
use firewheel_core::{
    node::{AudioNode, AudioNodeInfo, AudioNodeProcessor, ProcInfo},
    param::{range::percent_volume_to_raw_gain, smoother::ParamSmoother},
    BlockFrames,
};
use std::sync::{atomic::Ordering, Arc};

pub struct VolumeNode {
    // TODO: Find a good solution for webassembly.
    raw_gain: Arc<AtomicF32>,
    percent_volume: f32,
}

impl VolumeNode {
    pub fn new(percent_volume: f32) -> Self {
        let percent_volume = percent_volume.max(0.0);

        Self {
            raw_gain: Arc::new(AtomicF32::new(percent_volume_to_raw_gain(percent_volume))),
            percent_volume,
        }
    }

    pub fn percent_volume(&self) -> f32 {
        self.percent_volume
    }

    pub fn set_percent_volume(&mut self, percent_volume: f32) {
        self.raw_gain.store(
            percent_volume_to_raw_gain(percent_volume),
            Ordering::Relaxed,
        );
        self.percent_volume = percent_volume.max(0.0);
    }

    pub fn raw_gain(&self) -> f32 {
        self.raw_gain.load(Ordering::Relaxed)
    }
}

impl<C, const MBF: usize> AudioNode<C, MBF> for VolumeNode {
    fn info(&self) -> AudioNodeInfo {
        AudioNodeInfo {
            num_min_supported_inputs: 1,
            num_max_supported_inputs: 64,
            num_min_supported_outputs: 1,
            num_max_supported_outputs: 64,
        }
    }

    fn activate(
        &mut self,
        sample_rate: u32,
        num_inputs: usize,
        num_outputs: usize,
    ) -> Result<Box<dyn AudioNodeProcessor<C, MBF>>, Box<dyn std::error::Error>> {
        if num_inputs != num_outputs {
            return Err(format!("The number of inputs on a VolumeNode node must equal the number of outputs. Got num_inputs: {}, num_outputs: {}", num_inputs, num_outputs).into());
        }

        Ok(Box::new(VolumeProcessor {
            raw_gain: Arc::clone(&self.raw_gain),
            gain_smoother: ParamSmoother::new(self.raw_gain(), sample_rate, Default::default()),
        }))
    }
}

struct VolumeProcessor<const MBF: usize> {
    raw_gain: Arc<AtomicF32>,
    gain_smoother: ParamSmoother<MBF>,
}

impl<C, const MBF: usize> AudioNodeProcessor<C, MBF> for VolumeProcessor<MBF> {
    fn process(
        &mut self,
        frames: BlockFrames<MBF>,
        proc_info: ProcInfo<C>,
        inputs: &[&[f32; MBF]],
        outputs: &mut [&mut [f32; MBF]],
    ) {
        let raw_gain = self.raw_gain.load(Ordering::Relaxed);

        if proc_info.in_silence_mask.all_channels_silent(inputs.len()) {
            // All channels are silent, so there is no need to process. Also reset
            // the filter since it doesn't need to smooth anything.
            self.gain_smoother.reset(raw_gain);
            firewheel_core::util::clear_all_outputs(frames, outputs, proc_info.out_silence_mask);
            return;
        }

        let gain = self.gain_smoother.set_and_process(raw_gain, frames);

        if !gain.is_smoothing() && gain.values[0] < 0.00001 {
            // Muted, so there is no need to process.
            firewheel_core::util::clear_all_outputs(frames, outputs, proc_info.out_silence_mask);
            return;
        }

        *proc_info.out_silence_mask = proc_info.in_silence_mask;

        let frames = frames.get();

        // Provide an optimized loop for stereo.
        if inputs.len() == 2 && outputs.len() == 2 {
            for i in 0..frames {
                outputs[0][i] = inputs[0][i] * gain[i];
                outputs[1][i] = inputs[1][i] * gain[i];
            }

            return;
        }

        for (i, (output, input)) in outputs.iter_mut().zip(inputs.iter()).enumerate() {
            if proc_info.in_silence_mask.is_channel_silent(i) {
                output[..frames].fill(0.0);
                continue;
            }

            for i in 0..frames {
                output[i] = input[i] * gain[i];
            }
        }
    }
}
