use std::fmt;
use std::ops;
use std::slice;

/// The configuration for a [`ParamSmoother`]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SmootherConfig {
    /// The amount of smoothing in seconds
    ///
    /// By default this is set to 5 milliseconds.
    pub smooth_secs: f32,
    /// The threshold at which the smoothing will complete
    ///
    /// By default this is set to `0.00001`.
    pub settle_epsilon: f32,
}

impl Default for SmootherConfig {
    fn default() -> Self {
        Self {
            smooth_secs: 5.0 / 1000.0,
            settle_epsilon: 0.00001f32,
        }
    }
}

/// The status of a [`ParamSmoother`]
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum SmootherStatus {
    /// Not currently smoothing. All values in [`ParamSmoother::output`]
    /// will contain the same value.
    Inactive,
    /// Currently smoothing. Values in [`ParamSmoother::output`] will NOT
    /// be all the same.
    Active,
    /// Currently smoothing but will become deactivated on the next process
    /// cycle. Values in [`ParamSmoother::output`] will NOT be all the same.
    Deactivating,
}

impl SmootherStatus {
    fn is_active(&self) -> bool {
        self != &SmootherStatus::Inactive
    }
}

/// The output of a [`ParamSmoother`]
pub struct SmootherOutput<'a> {
    pub values: &'a [f32],
    pub status: SmootherStatus,
}

impl<'a> SmootherOutput<'a> {
    pub fn is_smoothing(&self) -> bool {
        self.status.is_active()
    }
}

impl<'a, I> ops::Index<I> for SmootherOutput<'a>
where
    I: slice::SliceIndex<[f32]>,
{
    type Output = I::Output;

    #[inline]
    fn index(&self, idx: I) -> &I::Output {
        &self.values[idx]
    }
}

/// A simple filter used to smooth a parameter
pub struct ParamSmoother {
    output: Vec<f32>,
    input: f32,

    status: SmootherStatus,

    a: f32,
    b: f32,
    last_output: f32,

    settle_epsilon: f32,
}

impl ParamSmoother {
    /// Create a new parameter smoothing filter.
    ///
    /// * `val` - The initial starting value
    /// * `max_block_frames` - The maximum number of frames that can appear in a
    /// processing block.
    /// * `sample_rate` - The sampling rate
    /// * `config` - Additional options for a [`ParamSmoother`]
    pub fn new(
        val: f32,
        max_block_frames: usize,
        sample_rate: u32,
        config: SmootherConfig,
    ) -> Self {
        let b = (-1.0f32 / (config.smooth_secs as f32 * sample_rate as f32)).exp();
        let a = 1.0f32 - b;

        Self {
            status: SmootherStatus::Inactive,
            input: val,
            output: vec![val; max_block_frames],

            a,
            b,
            last_output: val,
            settle_epsilon: config.settle_epsilon,
        }
    }

    /// Reset the filter with the new given initial value.
    pub fn reset(&mut self, val: f32) {
        self.status = SmootherStatus::Inactive;
        self.input = val;
        self.last_output = val;

        let max_block_frames = self.output.len();

        self.output.clear();
        self.output.resize(max_block_frames, val);
    }

    /// Set the new target value. If the value is different from the previous process
    /// cycle, then smoothing will begin.
    pub fn set(&mut self, val: f32) {
        if self.input == val {
            return;
        }

        self.input = val;
        self.status = SmootherStatus::Active;
    }

    /// The current target value that is being smoothed to.
    pub fn dest(&self) -> f32 {
        self.input
    }

    /// Get the current value of the smoother, along with its status.
    ///
    /// Note, this will NOT update the filter. This only returns the most
    /// recently-processed sample.
    pub fn current_value(&self) -> (f32, SmootherStatus) {
        (self.last_output, self.status)
    }

    /// Process the filter and return the smoothed output.
    ///
    /// If the filter is not currently smoothing, then no processing will occur and
    /// the output (which will contain all the same value) will simply be returned.
    pub fn process(&mut self, frames: usize) -> SmootherOutput {
        let frames = frames.min(self.output.len());

        if self.status != SmootherStatus::Active || frames == 0 || self.output.is_empty() {
            return SmootherOutput {
                values: &self.output[0..frames],
                status: self.status,
            };
        }

        let input = self.input * self.a;

        self.output[0] = input + (self.last_output * self.b);

        for i in 1..frames {
            self.output[i] = input + (self.output[i - 1] * self.b);
        }

        self.last_output = self.output[frames - 1];

        match self.status {
            SmootherStatus::Active => {
                if (self.input - self.output[0]).abs() < self.settle_epsilon {
                    self.reset(self.input);
                    self.status = SmootherStatus::Deactivating;
                }
            }
            SmootherStatus::Deactivating => self.status = SmootherStatus::Inactive,
            _ => (),
        };

        SmootherOutput {
            values: &self.output[0..frames],
            status: self.status,
        }
    }

    /// Whether or not the filter is currently smoothing (`true`) or not (`false`)
    pub fn is_active(&self) -> bool {
        self.status.is_active()
    }

    /// The maximum number of frames that can be sent to [`ParamSmoother::process`]
    pub fn max_block_frames(&self) -> usize {
        self.output.len()
    }
}

impl fmt::Debug for ParamSmoother {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct(concat!("ParamSmoother"))
            .field("output[0]", &self.output[0])
            .field("max_block_frames", &self.output.len())
            .field("input", &self.input)
            .field("status", &self.status)
            .field("last_output", &self.last_output)
            .field("settle_epsilon", &self.settle_epsilon)
            .finish()
    }
}
