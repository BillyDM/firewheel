/// A parameter range with a linear mapping
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinearRange {
    pub min: f32,
    pub max: f32,
}

impl LinearRange {
    pub fn new(min: f32, max: f32) -> Self {
        Self { min, max }
    }

    /// Map a value to its corresponding raw value for use in DSP
    pub fn clamp(&self, val: f32) -> f32 {
        if self.min > self.max {
            val.min(self.min).max(self.max)
        } else {
            val.min(self.max).max(self.min)
        }
    }
}

impl Default for LinearRange {
    fn default() -> Self {
        Self { min: 0.0, max: 1.0 }
    }
}

/// Map a percent value (where `0.0` means mute and `100.0` means unity
/// gain) to the corresponding raw gain value (not decibels) for use in
/// DSP. Values above `100.0` are allowed.
pub fn percent_volume_to_raw_gain(percent_volume: f32) -> f32 {
    let n = percent_volume.max(0.0) * (1.0 / 100.0);
    n * n
}

/// A parameter range that takes a normalized value in the range `[0.0, 1.0]`
/// as input and outputs a frequency value in Hz.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NormToFreqRange {
    min_hz: f32,
    max_hz: f32,

    min_log2: f32,
    range: f32,
}

impl NormToFreqRange {
    pub fn new(min_hz: f32, max_hz: f32) -> Self {
        assert!(min_hz < max_hz);
        assert_ne!(min_hz, 0.0);
        assert_ne!(max_hz, 0.0);

        let min_log2 = min_hz.log2();
        let range = max_hz.log2() - min_log2;

        Self {
            min_hz,
            max_hz,
            min_log2,
            range,
        }
    }

    pub fn min_hz(&self) -> f32 {
        self.min_hz
    }

    pub fn max_hz(&self) -> f32 {
        self.max_hz
    }

    /// Convert the normalized value in the range `[0.0, 1.0]` to the
    /// corresponding frequency value in hz.
    pub fn to_hz(&self, normalized: f32) -> f32 {
        if normalized <= 0.0 {
            return self.min_hz;
        }

        if normalized >= 1.0 {
            return self.max_hz;
        }

        2.0f32.powf((normalized * self.range) + self.min_log2)
    }
}

/// A parameter range that takes a normalized value in the range `[0.0, 1.0]`
/// as input and outputs a corresponding value using a power curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NormToPowRange {
    pub exponent: f32,
    min: f32,
    max: f32,
}

impl NormToPowRange {
    pub fn new(min: f32, max: f32, exponent: f32) -> Self {
        assert!(min <= max);

        Self { exponent, min, max }
    }

    pub fn min(&self) -> f32 {
        self.min
    }

    pub fn max(&self) -> f32 {
        self.max
    }

    /// Convert the normalized value in the range `[0.0, 1.0]` to the
    /// corresponding value for use in DSP.
    pub fn to_dsp(&self, normalized: f32) -> f32 {
        if normalized <= 0.0 {
            return self.min;
        }

        if normalized >= 1.0 {
            return self.max;
        }

        normalized.powf(self.exponent) * (self.max - self.min) + self.min
    }
}
