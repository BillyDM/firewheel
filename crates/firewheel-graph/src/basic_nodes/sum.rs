use firewheel_core::node::{AudioNode, AudioNodeInfo, AudioNodeProcessor, ProcInfo};

pub struct SumNode;

impl<C> AudioNode<C> for SumNode {
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
        _sample_rate: u32,
        _max_block_frames: usize,
        num_inputs: usize,
        num_outputs: usize,
    ) -> Result<Box<dyn AudioNodeProcessor<C>>, Box<dyn std::error::Error>> {
        if num_inputs % num_outputs != 0 {
            return Err(format!("The number of inputs on a SumNode must be a multiple of the number of outputs. Got num_inputs: {}, num_outputs: {}", num_inputs, num_outputs).into());
        }

        Ok(Box::new(SumNodeProcessor {
            num_in_ports: num_inputs / num_outputs,
        }))
    }
}

struct SumNodeProcessor {
    num_in_ports: usize,
}

impl<C> AudioNodeProcessor<C> for SumNodeProcessor {
    fn process(
        &mut self,
        _frames: usize,
        proc_info: ProcInfo<C>,
        inputs: &[&[f32]],
        outputs: &mut [&mut [f32]],
    ) {
        let num_inputs = inputs.len();
        let num_outputs = outputs.len();

        if proc_info.in_silence_mask.all_channels_silent(inputs.len()) {
            // All inputs are silent. Just clear outputs and return.
            firewheel_core::util::clear_all_outputs(outputs, proc_info.out_silence_mask);
            return;
        }

        if num_inputs == num_outputs {
            // No need to sum, just copy.
            for (out, input) in outputs.iter_mut().zip(inputs.iter()) {
                out.copy_from_slice(input);
            }
            *proc_info.out_silence_mask = proc_info.in_silence_mask;
            return;
        }

        match self.num_in_ports {
            // Provide a few optimized loops for common number of input ports.
            2 => {
                assert!(num_inputs >= (num_outputs * 2));

                for (ch_i, out) in outputs.iter_mut().enumerate() {
                    let in1 = &inputs[ch_i][0..out.len()];
                    let in2 = &inputs[(num_outputs * 1) + ch_i][0..out.len()];

                    for i in 0..out.len() {
                        out[i] = in1[i] + in2[i];
                    }
                }
            }
            3 => {
                assert!(num_inputs >= (num_outputs * 3));

                for (ch_i, out) in outputs.iter_mut().enumerate() {
                    let in1 = &inputs[ch_i][0..out.len()];
                    let in2 = &inputs[(num_outputs * 1) + ch_i][0..out.len()];
                    let in3 = &inputs[(num_outputs * 2) + ch_i][0..out.len()];

                    for i in 0..out.len() {
                        out[i] = in1[i] + in2[i] + in3[i];
                    }
                }
            }
            4 => {
                assert!(num_inputs >= (num_outputs * 4));

                for (ch_i, out) in outputs.iter_mut().enumerate() {
                    let in1 = &inputs[ch_i][0..out.len()];
                    let in2 = &inputs[(num_outputs * 1) + ch_i][0..out.len()];
                    let in3 = &inputs[(num_outputs * 2) + ch_i][0..out.len()];
                    let in4 = &inputs[(num_outputs * 3) + ch_i][0..out.len()];

                    for i in 0..out.len() {
                        out[i] = in1[i] + in2[i] + in3[i] + in4[i];
                    }
                }
            }
            n => {
                assert!(num_inputs >= (num_outputs * n));

                for (ch_i, out) in outputs.iter_mut().enumerate() {
                    out.copy_from_slice(inputs[ch_i]);

                    for in_port_i in 1..n {
                        let input = &inputs[(num_outputs * in_port_i) + ch_i][0..out.len()];
                        for (out_s, in_s) in out.iter_mut().zip(input.iter()) {
                            *out_s += *in_s;
                        }
                    }
                }
            }
        }
    }
}
