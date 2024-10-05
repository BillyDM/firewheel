pub struct CpalBackend {}

impl firewheel_core::AudioBackend for CpalBackend {
    type StreamHandle = cpal::Stream;
}
