use std::{
    fmt::Debug,
    ops::Range,
    sync::{atomic::Ordering, Arc},
};

use atomic_float::AtomicF32;
use firewheel_core::{
    node::{AudioNode, AudioNodeInfo, AudioNodeProcessor, ProcInfo},
    param::{range::percent_volume_to_raw_gain, smoother::ParamSmoother},
    sample_resource::SampleResource,
};

const CHANNEL_CAPACITY: usize = 128;

pub enum LoopRange {
    Full,
    RangeSecs(Range<f64>),
}

enum NodeToProcessorMsg<S: SampleResource> {
    SetSample { sample: S, stop_playback: bool },
    Play,
    Pause,
    Stop,
    SetPlayheadSecs(f64),
    SetLoopRange(Option<LoopRange>),
}

impl<S: SampleResource> Debug for NodeToProcessorMsg<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "NodeToProcessorMsg")
    }
}

enum ProcessorToNodeMsg<S: SampleResource> {
    ReturnSample(S),
}

struct ActiveState<S: SampleResource> {
    // TODO: Find a good solution for webassembly.
    to_processor_tx: rtrb::Producer<NodeToProcessorMsg<S>>,
    from_processor_rx: rtrb::Consumer<ProcessorToNodeMsg<S>>,
}

pub struct SamplerNode<S: SampleResource> {
    active_state: Option<ActiveState<S>>,

    raw_gain: Arc<AtomicF32>,
    percent_volume: f32,
    playing: bool,
}

impl<S: SampleResource> SamplerNode<S> {
    pub fn new(percent_volume: f32) -> Self {
        let percent_volume = percent_volume.max(0.0);

        Self {
            raw_gain: Arc::new(AtomicF32::new(percent_volume_to_raw_gain(percent_volume))),
            percent_volume,
            active_state: None,
            playing: false,
        }
    }

    // TODO: Error type
    pub fn set_sample(&mut self, sample: S, stop_playback: bool) -> Result<(), ()> {
        if let Some(state) = &mut self.active_state {
            state
                .to_processor_tx
                .push(NodeToProcessorMsg::SetSample {
                    sample,
                    stop_playback,
                })
                .map_err(|_| ())
        } else {
            todo!()
        }
    }

    // TODO: Error type
    pub fn play(&mut self) -> Result<(), ()> {
        if !self.playing {
            if let Some(state) = &mut self.active_state {
                state
                    .to_processor_tx
                    .push(NodeToProcessorMsg::Play)
                    .map_err(|_| ())?;
            } else {
                todo!()
            }

            self.playing = true;
        }

        Ok(())
    }

    // TODO: Error type
    pub fn pause(&mut self) -> Result<(), ()> {
        if self.playing {
            if let Some(state) = &mut self.active_state {
                state
                    .to_processor_tx
                    .push(NodeToProcessorMsg::Pause)
                    .map_err(|_| ())?;
            } else {
                todo!()
            }

            self.playing = false;
        }

        Ok(())
    }

    // TODO: Error type
    pub fn stop(&mut self) -> Result<(), ()> {
        if self.playing {
            if let Some(state) = &mut self.active_state {
                state
                    .to_processor_tx
                    .push(NodeToProcessorMsg::Stop)
                    .map_err(|_| ())?;
            } else {
                todo!()
            }

            self.playing = false;
        }

        Ok(())
    }

    // TODO: Error type
    pub fn set_playhead(&mut self, playhead_secs: f64) -> Result<(), ()> {
        if let Some(state) = &mut self.active_state {
            state
                .to_processor_tx
                .push(NodeToProcessorMsg::SetPlayheadSecs(playhead_secs))
                .map_err(|_| ())?;
        } else {
            todo!()
        }

        Ok(())
    }

    // TODO: Error type
    pub fn set_loop_range(&mut self, loop_range: Option<LoopRange>) -> Result<(), ()> {
        if let Some(state) = &mut self.active_state {
            state
                .to_processor_tx
                .push(NodeToProcessorMsg::SetLoopRange(loop_range))
                .map_err(|_| ())?;
        } else {
            todo!()
        }

        Ok(())
    }

    pub fn is_playing(&self) -> bool {
        self.playing
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

impl<S: SampleResource> AudioNode for SamplerNode<S> {
    fn debug_name(&self) -> &'static str {
        "beep_test"
    }

    fn info(&self) -> AudioNodeInfo {
        AudioNodeInfo {
            num_min_supported_outputs: 1,
            num_max_supported_outputs: 64,
            updates: true,
            ..Default::default()
        }
    }

    fn activate(
        &mut self,
        sample_rate: u32,
        max_block_frames: usize,
        _num_inputs: usize,
        _num_outputs: usize,
    ) -> Result<Box<dyn AudioNodeProcessor>, Box<dyn std::error::Error>> {
        let (to_processor_tx, from_node_rx) =
            rtrb::RingBuffer::<NodeToProcessorMsg<S>>::new(CHANNEL_CAPACITY);
        let (to_node_tx, from_processor_rx) =
            rtrb::RingBuffer::<ProcessorToNodeMsg<S>>::new(CHANNEL_CAPACITY);

        self.active_state = Some(ActiveState {
            to_processor_tx,
            from_processor_rx,
        });

        Ok(Box::new(SamplerProcessor::new(
            Arc::clone(&self.raw_gain),
            sample_rate,
            max_block_frames,
            from_node_rx,
            to_node_tx,
        )))
    }

    fn update(&mut self) {
        if let Some(active_state) = &mut self.active_state {
            while let Ok(msg) = active_state.from_processor_rx.pop() {
                match msg {
                    ProcessorToNodeMsg::ReturnSample(_smp) => {}
                }
            }
        }
    }
}

struct ProcLoopRange {
    playhead_range: Range<u64>,
    full_range: bool,
}

impl ProcLoopRange {
    fn new<S: SampleResource>(loop_range: LoopRange, sample_rate: u32, sample: &Option<S>) -> Self {
        let (start_frame, end_frame, full_range) = match &loop_range {
            LoopRange::Full => {
                let end_frame = if let Some(sample) = sample {
                    sample.len_frames()
                } else {
                    0
                };

                (0, end_frame, true)
            }
            LoopRange::RangeSecs(range) => (
                (range.start * f64::from(sample_rate)).round() as u64,
                (range.end * f64::from(sample_rate)).round() as u64,
                false,
            ),
        };

        Self {
            playhead_range: start_frame..end_frame,
            full_range,
        }
    }

    fn update_sample<S: SampleResource>(&mut self, sample: &Option<S>) {
        let Some(sample) = sample else {
            return;
        };

        if !self.full_range {
            return;
        }

        let end_frame = sample.len_frames();

        self.playhead_range = 0..end_frame;
    }
}

struct SamplerProcessor<S: SampleResource> {
    raw_gain: Arc<AtomicF32>,
    gain_smoother: ParamSmoother,
    playing: bool,
    sample_rate: u32,
    playhead: u64,
    loop_range: Option<ProcLoopRange>,

    sample: Option<S>,

    from_node_rx: rtrb::Consumer<NodeToProcessorMsg<S>>,
    to_node_tx: rtrb::Producer<ProcessorToNodeMsg<S>>,
}

impl<S: SampleResource> SamplerProcessor<S> {
    fn new(
        raw_gain: Arc<AtomicF32>,
        sample_rate: u32,
        max_block_frames: usize,
        from_node_rx: rtrb::Consumer<NodeToProcessorMsg<S>>,
        to_node_tx: rtrb::Producer<ProcessorToNodeMsg<S>>,
    ) -> Self {
        let gain_val = raw_gain.load(Ordering::Relaxed);

        Self {
            raw_gain,
            gain_smoother: ParamSmoother::new(
                gain_val,
                sample_rate,
                max_block_frames,
                Default::default(),
            ),
            playing: false,
            sample_rate,
            playhead: 0,
            loop_range: None,
            sample: None,
            from_node_rx,
            to_node_tx,
        }
    }
}

impl<S: SampleResource> AudioNodeProcessor for SamplerProcessor<S> {
    fn process(
        &mut self,
        frames: usize,
        _inputs: &[&[f32]],
        outputs: &mut [&mut [f32]],
        proc_info: ProcInfo,
    ) {
        while let Ok(msg) = self.from_node_rx.pop() {
            match msg {
                NodeToProcessorMsg::SetSample {
                    sample,
                    stop_playback,
                } => {
                    if let Some(old_sample) = self.sample.take() {
                        let _ = self
                            .to_node_tx
                            .push(ProcessorToNodeMsg::ReturnSample(old_sample));
                    }

                    self.sample = Some(sample);

                    if let Some(loop_range) = &mut self.loop_range {
                        loop_range.update_sample(&self.sample);
                    }

                    if stop_playback {
                        self.playhead = self
                            .loop_range
                            .as_ref()
                            .map(|l| l.playhead_range.start)
                            .unwrap_or(0);

                        if self.playing {
                            self.playing = false;

                            // TODO
                        }
                    }

                    // TODO: Declick
                }
                NodeToProcessorMsg::Play => {
                    if !self.playing {
                        self.playing = true;

                        // TODO: Declick
                    }
                }
                NodeToProcessorMsg::Pause => {
                    if self.playing {
                        self.playing = false;

                        // TODO: Declick
                    }
                }
                NodeToProcessorMsg::Stop => {
                    self.playhead = self
                        .loop_range
                        .as_ref()
                        .map(|l| l.playhead_range.start)
                        .unwrap_or(0);

                    if self.playing {
                        self.playing = false;

                        // TODO: Declick
                    }
                }
                NodeToProcessorMsg::SetPlayheadSecs(playhead_secs) => {
                    let sample = (playhead_secs * f64::from(self.sample_rate)).round() as u64;

                    if sample != self.playhead {
                        self.playhead = sample;
                        // TODO: Declick
                    }
                }
                NodeToProcessorMsg::SetLoopRange(loop_range) => {
                    self.loop_range = loop_range.map(|loop_range| {
                        ProcLoopRange::new(loop_range, self.sample_rate, &self.sample)
                    });

                    if let Some(loop_range) = &self.loop_range {
                        if loop_range.playhead_range.contains(&self.playhead) {
                            self.playhead = loop_range.playhead_range.start;

                            // TODO: Declick
                        }
                    }
                }
            }
        }

        let Some(sample) = &self.sample else {
            // TODO: Declick

            // No sample data, output silence.
            firewheel_core::util::clear_all_outputs(frames, outputs, proc_info.out_silence_mask);
            return;
        };

        if !self.playing {
            // TODO: Declick

            // Not playing, output silence.
            firewheel_core::util::clear_all_outputs(frames, outputs, proc_info.out_silence_mask);
            return;
        }

        let raw_gain = self.raw_gain.load(Ordering::Relaxed);
        let gain = self.gain_smoother.set_and_process(raw_gain, frames);
        // Hint to the compiler to optimize loop.
        assert_eq!(gain.values.len(), frames);

        if !gain.is_smoothing() && gain.values[0] < 0.00001 {
            // TODO: Reset declick.

            // Muted, so there is no need to process.
            firewheel_core::util::clear_all_outputs(frames, outputs, proc_info.out_silence_mask);
            return;
        }

        if let Some(loop_range) = &self.loop_range {
            if self.playhead >= loop_range.playhead_range.end {
                // Playhead is out of range. Return to the start.
                self.playhead = self
                    .loop_range
                    .as_ref()
                    .map(|l| l.playhead_range.start)
                    .unwrap_or(0);
            }

            // Copy first block of samples.

            let frames_left = if loop_range.playhead_range.end - self.playhead <= usize::MAX as u64
            {
                (loop_range.playhead_range.end - self.playhead) as usize
            } else {
                usize::MAX
            };
            let first_copy_frames = frames.min(frames_left);

            sample.fill_buffers(outputs, 0..first_copy_frames, self.playhead);

            if first_copy_frames < frames {
                // Loop back to the start.
                self.playhead = self
                    .loop_range
                    .as_ref()
                    .map(|l| l.playhead_range.start)
                    .unwrap_or(0);

                // Copy second block of samples.

                let second_copy_frames = frames - first_copy_frames;

                sample.fill_buffers(outputs, first_copy_frames..frames, self.playhead);

                self.playhead += second_copy_frames as u64;
            } else {
                self.playhead += frames as u64;
            }
        } else {
            if self.playhead >= sample.len_frames() {
                // Playhead is out of range. Output silence.
                self.playing = false;
                firewheel_core::util::clear_all_outputs(
                    frames,
                    outputs,
                    proc_info.out_silence_mask,
                );
                return;

                // TODO: Notify node that sample has finished.
            }

            let copy_frames = frames.min((sample.len_frames() - self.playhead) as usize);

            sample.fill_buffers(outputs, 0..copy_frames, self.playhead);

            if copy_frames < frames {
                // Finished playing sample.
                self.playing = false;
                self.playhead = 0;

                // Fill any remaining frames with zeros
                for out_ch in outputs.iter_mut() {
                    out_ch[copy_frames..].fill(0.0);
                }

                // TODO: Notify node that sample has finished.
            } else {
                self.playhead += frames as u64;
            }
        }

        let sample_channels = sample.num_channels().get();

        // Apply gain and declick
        // TODO: Declick
        if outputs.len() >= 2 && sample_channels == 2 {
            // Provide an optimized stereo loop.

            // Hint to the compiler to optimize loop.
            assert_eq!(outputs[0].len(), frames);
            assert_eq!(outputs[1].len(), frames);

            for i in 0..frames {
                outputs[0][i] *= gain.values[i];
                outputs[1][i] *= gain.values[i];
            }
        } else {
            for (out_ch, _) in outputs.iter_mut().zip(0..sample_channels) {
                // Hint to the compiler to optimize loop.
                assert_eq!(out_ch.len(), frames);

                for i in 0..frames {
                    out_ch[i] *= gain.values[i];
                }
            }
        }

        if outputs.len() > sample_channels {
            if outputs.len() == 2 && sample_channels == 1 {
                // If the output of this node is stereo and the sample is mono,
                // assume that the user wants both channels filled with the
                // sample data.
                let (out_first, outs) = outputs.split_first_mut().unwrap();
                outs[0].copy_from_slice(out_first);
            } else {
                // Fill the rest of the channels with zeros.
                for (i, out_ch) in outputs.iter_mut().enumerate().skip(sample_channels) {
                    out_ch.fill(0.0);
                    proc_info.out_silence_mask.set_channel(i, true);
                }
            }
        }
    }
}

impl<S: SampleResource> Drop for SamplerProcessor<S> {
    fn drop(&mut self) {
        if let Some(sample) = self.sample.take() {
            let _ = self
                .to_node_tx
                .push(ProcessorToNodeMsg::ReturnSample(sample));
        }
    }
}

impl<S: SampleResource> Into<Box<dyn AudioNode>> for SamplerNode<S> {
    fn into(self) -> Box<dyn AudioNode> {
        Box::new(self)
    }
}
