//! Dedicated owner thread for microphone capture.
//!
//! `cpal::Stream` must be stopped by the same audio owner that created it.
//! Tauri callbacks and global shortcut callbacks can arrive on different
//! threads, so callers send commands here instead of storing AudioCapture in
//! thread-local UI callback storage.

use crate::audio::{AudioCapture, AudioError};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

const TARGET_SAMPLE_RATE: u32 = 16_000;

#[derive(Debug, Clone)]
pub struct ActiveRecording {
    pub buffer: Arc<Mutex<Vec<f32>>>,
    pub sample_rate: u32,
}

#[derive(Debug)]
pub struct RecordedAudio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

enum AudioSessionCommand {
    Start {
        device_name: Option<String>,
        reply: mpsc::Sender<Result<ActiveRecording, AudioError>>,
    },
    Stop {
        reply: mpsc::Sender<Result<Option<RecordedAudio>, AudioError>>,
    },
    IsRecording {
        reply: mpsc::Sender<bool>,
    },
}

trait CaptureDriver {
    fn start(&mut self, device_name: Option<&str>) -> Result<(), AudioError>;
    fn stop(&mut self) -> Vec<f32>;
    fn get_buffer_arc(&self) -> Arc<Mutex<Vec<f32>>>;
    fn get_sample_rate(&self) -> u32;
    fn is_recording(&self) -> bool;
}

impl CaptureDriver for AudioCapture {
    fn start(&mut self, device_name: Option<&str>) -> Result<(), AudioError> {
        AudioCapture::start(self, device_name)
    }

    fn stop(&mut self) -> Vec<f32> {
        AudioCapture::stop(self)
    }

    fn get_buffer_arc(&self) -> Arc<Mutex<Vec<f32>>> {
        AudioCapture::get_buffer_arc(self)
    }

    fn get_sample_rate(&self) -> u32 {
        AudioCapture::get_sample_rate(self)
    }

    fn is_recording(&self) -> bool {
        AudioCapture::is_recording(self)
    }
}

#[derive(Clone)]
pub struct AudioSessionController {
    tx: mpsc::Sender<AudioSessionCommand>,
}

impl AudioSessionController {
    pub fn spawn() -> Self {
        Self::spawn_with_factory::<AudioCapture, _>(AudioCapture::new)
    }

    fn spawn_with_factory<C, F>(factory: F) -> Self
    where
        C: CaptureDriver + 'static,
        F: FnMut() -> Result<C, AudioError> + Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || run_session_loop(rx, factory));
        Self { tx }
    }

    pub fn start(&self, device_name: Option<String>) -> Result<ActiveRecording, AudioError> {
        let (reply, response) = mpsc::channel();
        self.tx
            .send(AudioSessionCommand::Start { device_name, reply })
            .map_err(|e| {
                AudioError::Other(format!("Audio session thread is unavailable: {}", e))
            })?;

        response.recv().map_err(|e| {
            AudioError::Other(format!("Audio session thread did not respond: {}", e))
        })?
    }

    pub fn stop(&self) -> Result<Option<RecordedAudio>, AudioError> {
        let (reply, response) = mpsc::channel();
        self.tx
            .send(AudioSessionCommand::Stop { reply })
            .map_err(|e| {
                AudioError::Other(format!("Audio session thread is unavailable: {}", e))
            })?;

        response.recv().map_err(|e| {
            AudioError::Other(format!("Audio session thread did not respond: {}", e))
        })?
    }

    pub fn is_recording(&self) -> bool {
        let (reply, response) = mpsc::channel();
        if self
            .tx
            .send(AudioSessionCommand::IsRecording { reply })
            .is_err()
        {
            return false;
        }

        response.recv().unwrap_or(false)
    }
}

fn run_session_loop<C, F>(rx: mpsc::Receiver<AudioSessionCommand>, mut factory: F)
where
    C: CaptureDriver + 'static,
    F: FnMut() -> Result<C, AudioError>,
{
    let mut capture: Option<C> = None;

    for command in rx {
        match command {
            AudioSessionCommand::Start { device_name, reply } => {
                let already_recording = capture
                    .as_ref()
                    .map(|active| active.is_recording())
                    .unwrap_or(false);

                if already_recording {
                    let _ = reply.send(Err(AudioError::Other(
                        "Recording is already active".to_string(),
                    )));
                    continue;
                }

                let result = match factory() {
                    Ok(mut next_capture) => match next_capture.start(device_name.as_deref()) {
                        Ok(()) => {
                            let active = ActiveRecording {
                                buffer: next_capture.get_buffer_arc(),
                                sample_rate: next_capture.get_sample_rate(),
                            };
                            capture = Some(next_capture);
                            Ok(active)
                        }
                        Err(error) => Err(error),
                    },
                    Err(error) => Err(error),
                };

                let _ = reply.send(result);
            }
            AudioSessionCommand::Stop { reply } => {
                let result = if let Some(mut active_capture) = capture.take() {
                    let samples = active_capture.stop();
                    Ok(Some(RecordedAudio {
                        samples,
                        sample_rate: TARGET_SAMPLE_RATE,
                    }))
                } else {
                    Ok(None)
                };

                let _ = reply.send(result);
            }
            AudioSessionCommand::IsRecording { reply } => {
                let recording = capture
                    .as_ref()
                    .map(|active| active.is_recording())
                    .unwrap_or(false);
                let _ = reply.send(recording);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;

    struct FakeCapture {
        buffer: Arc<Mutex<Vec<f32>>>,
        sample_rate: u32,
        started: bool,
        stop_count: Arc<AtomicUsize>,
        start_thread: Arc<Mutex<Option<thread::ThreadId>>>,
        stop_thread: Arc<Mutex<Option<thread::ThreadId>>>,
    }

    impl CaptureDriver for FakeCapture {
        fn start(&mut self, _device_name: Option<&str>) -> Result<(), AudioError> {
            self.started = true;
            *self.start_thread.lock().unwrap() = Some(thread::current().id());
            Ok(())
        }

        fn stop(&mut self) -> Vec<f32> {
            self.stop_count.fetch_add(1, Ordering::SeqCst);
            *self.stop_thread.lock().unwrap() = Some(thread::current().id());
            self.started = false;
            let mut buffer = self.buffer.lock().unwrap();
            std::mem::take(&mut *buffer)
        }

        fn get_buffer_arc(&self) -> Arc<Mutex<Vec<f32>>> {
            Arc::clone(&self.buffer)
        }

        fn get_sample_rate(&self) -> u32 {
            self.sample_rate
        }

        fn is_recording(&self) -> bool {
            self.started
        }
    }

    #[test]
    fn stop_runs_on_capture_owner_thread_even_when_requested_from_another_thread() {
        let stop_count = Arc::new(AtomicUsize::new(0));
        let start_thread = Arc::new(Mutex::new(None));
        let stop_thread = Arc::new(Mutex::new(None));

        let controller = AudioSessionController::spawn_with_factory::<FakeCapture, _>({
            let stop_count = Arc::clone(&stop_count);
            let start_thread = Arc::clone(&start_thread);
            let stop_thread = Arc::clone(&stop_thread);

            move || {
                Ok(FakeCapture {
                    buffer: Arc::new(Mutex::new(vec![0.25, -0.25])),
                    sample_rate: TARGET_SAMPLE_RATE,
                    started: false,
                    stop_count: Arc::clone(&stop_count),
                    start_thread: Arc::clone(&start_thread),
                    stop_thread: Arc::clone(&stop_thread),
                })
            }
        });

        let active = controller
            .start(Some("Built-in Microphone".to_string()))
            .unwrap();
        assert_eq!(active.sample_rate, TARGET_SAMPLE_RATE);
        assert!(controller.is_recording());

        let controller_from_other_thread = controller.clone();
        let recorded = thread::spawn(move || controller_from_other_thread.stop().unwrap())
            .join()
            .unwrap()
            .unwrap();

        assert_eq!(recorded.samples, vec![0.25, -0.25]);
        assert_eq!(recorded.sample_rate, TARGET_SAMPLE_RATE);
        assert_eq!(stop_count.load(Ordering::SeqCst), 1);
        assert_eq!(*start_thread.lock().unwrap(), *stop_thread.lock().unwrap());
        assert!(!controller.is_recording());
    }

    #[test]
    fn stop_without_recording_is_a_noop() {
        let stop_count = Arc::new(AtomicUsize::new(0));

        let controller = AudioSessionController::spawn_with_factory::<FakeCapture, _>({
            let stop_count = Arc::clone(&stop_count);

            move || {
                Ok(FakeCapture {
                    buffer: Arc::new(Mutex::new(Vec::new())),
                    sample_rate: TARGET_SAMPLE_RATE,
                    started: false,
                    stop_count: Arc::clone(&stop_count),
                    start_thread: Arc::new(Mutex::new(None)),
                    stop_thread: Arc::new(Mutex::new(None)),
                })
            }
        });

        assert!(controller.stop().unwrap().is_none());
        assert_eq!(stop_count.load(Ordering::SeqCst), 0);
    }
}
