use std::error::Error;

use crate::server::AudioGraphExecutor;

pub trait AudioBackend: Default {
    type StreamHandle;
    type Config;
    type StartStreamError: Error + 'static;
    type StreamError: Error + 'static;

    fn start_stream(&mut self, sample_rate: f64, config: Self::Config, executor: AudioGraphExecutor) -> Result<StartStreamResult<Self::StreamHandle>, Self::StartStreamError>;

    fn poll_for_errors(&mut self, stream_handle: &Self::StreamHandle) -> PollStatus<Self::StreamError>;
}

pub enum PollStatus<E: Error + 'static> {
    Ok,
    Err {
        msg: E,
        /// If the audio stream is in a state where it can be closed gracefully,
        /// set this to `true`. Otherwise, set this to `false`.
        can_close_gracefully: bool,
    }
}

pub struct StartStreamResult<S> {
    pub stream_handle: S,
    pub num_input_channels: usize,
    pub num_output_channels: usize,
}

// TODO: Disable dummy module on WASM
pub mod dummy {
    use std::{sync::{atomic::{AtomicBool, Ordering}, Arc}, time::{Duration, Instant}};

    use crate::server::AudioGraphExecutor;

    use super::{AudioBackend, PollStatus, StartStreamResult};

    #[derive(Default)]
    pub struct DummyAudioBackend;

    impl AudioBackend for DummyAudioBackend {
        type StreamHandle = DummyStreamHandle;
        type Config = ();
        type StartStreamError = DummyStreamError;
        type StreamError = DummyStreamError;

        fn start_stream(&mut self, sample_rate: f64, _config: Self::Config, mut executor: AudioGraphExecutor) -> Result<StartStreamResult<Self::StreamHandle>, Self::StartStreamError> {
            let run = Arc::new(AtomicBool::new(true));
            let stream_handle = DummyStreamHandle { run: Arc::clone(&run) };

            let mut last_instant: Instant = Instant::now();

            std::thread::spawn(move || {
                while run.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(1));

                    let frames = (last_instant.elapsed().as_secs_f64() * sample_rate).round() as usize;
                    last_instant = Instant::now();

                    executor.process_interleaved(&[], &mut [], 0, 0, frames);
                }
            });

            Ok(StartStreamResult {
                stream_handle,
                num_input_channels: 0,
                num_output_channels: 0,
            })
        }

        fn poll_for_errors(&mut self, stream_handle: &Self::StreamHandle) -> PollStatus<Self::StreamError> {
            if Arc::strong_count(&stream_handle.run) == 1 {
                PollStatus::Err { msg: DummyStreamError::Unknown, can_close_gracefully: false }
            } else {
                PollStatus::Ok
            }
        }
    }

    pub struct DummyStreamHandle {
        run: Arc<AtomicBool>,
    }

    impl Drop for DummyStreamHandle {
        fn drop(&mut self) {
            self.run.store(false, Ordering::Relaxed);
        }
    }

    #[derive(Debug, thiserror::Error)]
    pub enum DummyStreamError {
        #[error("Unkown error")]
        Unknown,
    }
}