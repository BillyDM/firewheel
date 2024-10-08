//! General conversion functions and utilities.

use crate::SilenceMask;

/// Returns the raw amplitude from the given decibel value.
#[inline]
pub fn db_to_amp(db: f32) -> f32 {
    10.0f32.powf(0.05 * db)
}

/// Returns the decibel value from the raw amplitude.
#[inline]
pub fn amp_to_db(amp: f32) -> f32 {
    20.0 * amp.log(10.0)
}

/// Returns the raw amplitude from the given decibel value.
///
/// If `db <= -100.0`, then 0.0 will be returned instead (negative infinity gain).
#[inline]
pub fn db_to_amp_clamped_neg_100_db(db: f32) -> f32 {
    if db <= -100.0 {
        0.0
    } else {
        db_to_amp(db)
    }
}

/// Returns the decibel value from the raw amplitude value.
///
/// If `amp <= 0.00001`, then the minimum of `-100.0` dB will be
/// returned instead (representing negative infinity gain when paired with
/// [`db_to_amp_clamped_neg_100_db`]).
#[inline]
pub fn amp_to_db_clamped_neg_100_db(amp: f32) -> f32 {
    if amp <= 0.00001 {
        -100.0
    } else {
        amp_to_db(amp)
    }
}

/// De-interleave audio channels
pub fn deinterleave<'a>(
    mut channels: impl Iterator<Item = &'a mut [f32]>,
    interleaved: &[f32],
    num_interleaved_channels: usize,
    calculate_silence_mask: bool,
) -> SilenceMask {
    let mut silence_mask = SilenceMask::NONE_SILENT;
    let mut i = 0;

    for _ in 0..num_interleaved_channels {
        let Some(ch) = channels.next() else {
            return silence_mask;
        };

        if calculate_silence_mask && i < 64 {
            if ch.iter().find(|&&s| s != 0.0).is_none() {
                silence_mask.set_channel(i, true);
            }
        }

        for (input, output) in interleaved
            .iter()
            .skip(i)
            .step_by(num_interleaved_channels)
            .zip(ch.iter_mut())
        {
            *output = *input;
        }

        i += 1;
    }

    while let Some(ch) = channels.next() {
        ch.fill(0.0);

        if calculate_silence_mask && i < 64 {
            silence_mask.set_channel(i, true);
        }

        i += 1;
    }

    silence_mask
}

/// Interleave audio channels
pub fn interleave<'a>(
    mut channels: impl Iterator<Item = &'a [f32]>,
    interleaved: &mut [f32],
    num_interleaved_channels: usize,
    silence_mask: Option<SilenceMask>,
) {
    interleaved.fill(0.0);

    for ch_i in 0..num_interleaved_channels {
        let Some(ch) = channels.next() else {
            return;
        };

        if let Some(silence_mask) = silence_mask {
            if ch_i < 64 {
                if silence_mask.is_channel_silent(ch_i) {
                    continue;
                }
            }
        }

        for (output, input) in interleaved
            .iter_mut()
            .skip(ch_i)
            .step_by(num_interleaved_channels)
            .zip(ch.iter())
        {
            *output = *input;
        }
    }
}

/// Optimized interleaving for stereo audio channels
pub fn interleave_stereo(
    in_l: &[f32],
    in_r: &[f32],
    interleaved: &mut [f32],
    silence_mask: Option<SilenceMask>,
) {
    if let Some(silence_mask) = silence_mask {
        if silence_mask.all_channels_silent(2) {
            interleaved.fill(0.0);
            return;
        }
    }

    let frames = interleaved.len() / 2;
    let in_l = &in_l[0..frames];
    let in_r = &in_r[0..frames];

    for (out, (in_l, in_r)) in interleaved
        .chunks_exact_mut(2)
        .zip(in_l.iter().zip(in_r.iter()))
    {
        out[0] = *in_l;
        out[1] = *in_r;
    }
}

/// Optimized de-interleaving for stereo audio channels
pub fn deinterleave_stereo(out_l: &mut [f32], out_r: &mut [f32], interleaved: &[f32]) {
    let frames = interleaved.len() / 2;
    let out_l = &mut out_l[0..frames];
    let out_r = &mut out_r[0..frames];

    for (input, (out_l, out_r)) in interleaved
        .chunks_exact(2)
        .zip(out_l.iter_mut().zip(out_r.iter_mut()))
    {
        *out_l = input[0];
        *out_r = input[1];
    }
}

/// Recycle the allocation of one Vec for another Vec.
///
/// Note, this only works if the types `A` and `B` have the
/// same size.
///
/// This can be useful for realtime code which needs a Vec
/// of references without allocating. For example:
/// ```rust
/// # use firewheel_core::util::recycle_vec;
/// #
/// struct Foo {
///     buffer_list: Option<Vec<&'static Vec<f32>>>,
/// }
///
/// impl Foo {
///     pub fn new() -> Self {
///         Self { buffer_list: Some(Vec::with_capacity(100)) }
///     }
///
///     pub fn realtime_function(&mut self) {
///         // No allocations or deallocations are made here!
///         let mut buffer_list: Vec<&Vec<f32>> =
///             recycle_vec(self.buffer_list.take().unwrap());
///
///         // ... use buffer_list ...
///         assert!(buffer_list.capacity() >= 100);
///
///         // Put the buffer back so the allocation can be used again
///         // for the next call to `realtime_function()`.
///         self.buffer_list = Some(recycle_vec(buffer_list));
///     }
/// }
///
/// let mut foo = Foo::new();
/// foo.realtime_function();
/// foo.realtime_function();
/// ```
pub fn recycle_vec<A, B>(mut v: Vec<A>) -> Vec<B> {
    debug_assert_eq!(std::mem::size_of::<A>(), std::mem::size_of::<B>());

    v.clear();
    v.into_iter().map(|_| unreachable!()).collect()
}

/// A convenience method to clear all output channels to `0.0` (silence)
pub fn clear_all_outputs(outputs: &mut [&mut [f32]], out_silence_mask: &mut SilenceMask) {
    for out in outputs.iter_mut() {
        out.fill(0.0);
    }

    *out_silence_mask = SilenceMask::new_all_silent(outputs.len());
}
