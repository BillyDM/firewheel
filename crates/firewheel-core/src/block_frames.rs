use std::hint::unreachable_unchecked;

/// A `usize` value which is gauranteed to be less than `MBF`
/// (max block frames).
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockFrames<const MBF: usize>(usize);

impl<const MBF: usize> BlockFrames<MBF> {
    pub fn new(frames: usize) -> Self {
        Self(frames.min(MBF - 1))
    }

    #[inline(always)]
    pub fn get(&self) -> usize {
        if self.0 < MBF {
            self.0
        } else {
            // SAFETY:
            // The constructor ensures that `self.0 < MBF`.
            unsafe { unreachable_unchecked() }
        }
    }
}

impl<const MBF: usize> From<usize> for BlockFrames<MBF> {
    fn from(frames: usize) -> Self {
        Self::new(frames)
    }
}

impl<const MBF: usize> Into<usize> for BlockFrames<MBF> {
    #[inline(always)]
    fn into(self) -> usize {
        self.get()
    }
}
