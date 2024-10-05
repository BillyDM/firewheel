/// An optional optimization hint on which channels contain all
/// zeros (silence). The first bit (`0x1`) is the first channel,
/// the second bit is the second channel, and so on.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SilenceMask(pub u64);

impl SilenceMask {
    /// A mask with no channels marked as silent
    pub const NONE_SILENT: Self = Self(0);

    /// A mask with only the first channel marked as silent
    pub const MONO_SILENT: Self = Self(0b1);

    /// A mask with only the first two channels marked as silent
    pub const STEREO_SILENT: Self = Self(0b11);

    /// Returns `true` if the channel is marked as silent, `false`
    /// otherwise.
    ///
    /// `i` must be less than `64`.
    pub const fn is_channel_silent(&self, i: usize) -> bool {
        self.0 & (0b1 << i) != 0
    }

    /// Returns `true` if any channel is marked as silent, `false`
    /// otherwise.
    ///
    /// `num_channels` must be less than `64`.
    pub const fn any_channel_silent(&self, num_channels: usize) -> bool {
        self.0 & ((0b1 << num_channels) - 1) != 0
    }

    /// Returns `true` if all channels are marked as silent, `false`
    /// otherwise.
    ///
    /// `num_channels` must be less than `64`.
    pub const fn all_channels_silent(&self, num_channels: usize) -> bool {
        let mask = (0b1 << num_channels) - 1;
        self.0 & mask == mask
    }

    /// Mark/un-mark the given channel as silent.
    ///
    /// `num_channels` must be less than `64`.
    pub fn set_channel(&mut self, i: usize, silent: bool) {
        if silent {
            self.0 |= 0b1 << i;
        } else {
            self.0 &= !(0b1 << i);
        }
    }
}
