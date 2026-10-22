//! Microphone capture on a dedicated thread.
//!
//! The cpal callback only downmixes into a pre-allocated ring buffer (no
//! allocation, no locks). The audio thread drains it every 20 ms while
//! recording, emits levels for the waveform and detects silence. When idle,
//! the thread blocks on its command channel: no polling.

pub mod level;
pub mod resample;

use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use serde::Serialize;
use tokio::sync::{mpsc as tmpsc, oneshot};

use level::SilenceDetector;

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("no microphone found")]
    NoDevice,
    #[error("cannot open microphone: {0}")]
    Open(String),
    #[error("unsupported sample format {0}")]
    Format(String),
    #[error("resampling failed: {0}")]
    Resample(String),
    #[error("not recording")]
    NotRecording,
    #[error("audio thread stopped")]
    ThreadGone,
}

#[derive(Debug, Clone, Serialize)]
pub struct MicInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AudioEvent {
    /// 0.0 – 1.0, about 30 per second while recording.
    Level(f32),
    DeviceLost,
    Silence,
}

#[derive(Debug)]
pub struct StartInfo {
    pub device_name: String,
    /// The configured microphone was missing; the default one is used.
    pub fell_back: bool,
}

enum Cmd {
    Start {
        device_id: Option<String>,
        silence_timeout_ms: Option<u64>,
        reply: oneshot::Sender<Result<StartInfo, AudioError>>,
    },
    Stop {
        reply: oneshot::Sender<Result<Vec<f32>, AudioError>>,
    },
    Discard,
}

/// Cheap handle to the audio thread.
#[derive(Clone)]
pub struct Audio {
    tx: mpsc::Sender<Cmd>,
}

impl Audio {
    pub fn spawn(events: tmpsc::UnboundedSender<AudioEvent>) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("audio".into())
            .spawn(move || run(rx, events))?;
        Ok(Self { tx })
    }

    pub async fn start(
        &self,
        device_id: Option<String>,
        silence_timeout_ms: Option<u64>,
    ) -> Result<StartInfo, AudioError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Cmd::Start {
                device_id,
                silence_timeout_ms,
                reply,
            })
            .map_err(|_| AudioError::ThreadGone)?;
        rx.await.map_err(|_| AudioError::ThreadGone)?
    }

    /// Stops and returns the recording as 16 kHz mono.
    pub async fn stop(&self) -> Result<Vec<f32>, AudioError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Cmd::Stop { reply })
            .map_err(|_| AudioError::ThreadGone)?;
        rx.await.map_err(|_| AudioError::ThreadGone)?
    }

    pub fn discard(&self) {
        // A dead thread has nothing to discard.
        let _ = self.tx.send(Cmd::Discard);
    }
}

pub fn list_inputs() -> Vec<MicInfo> {
    let host = cpal::default_host();
    let default_id = host.default_input_device().and_then(|d| d.id().ok());
    let Ok(devices) = host.input_devices() else {
        return Vec::new();
    };
    devices
        .filter_map(|d| {
            let id = d.id().ok()?;
            Some(MicInfo {
                id: id.to_string(),
                name: d.to_string(),
                is_default: Some(&id) == default_id.as_ref(),
            })
        })
        .collect()
}

const DRAIN_EVERY: Duration = Duration::from_millis(20);
const LEVEL_BLOCK_MS: u64 = 33;

struct Recording {
    // Dropping the stream stops capture.
    _stream: cpal::Stream,
    ring: rtrb::Consumer<f32>,
    rate: u32,
    samples: Vec<f32>,
    block: Vec<f32>,
    block_len: usize,
    silence: Option<SilenceDetector>,
    lost: Arc<AtomicBool>,
    lost_reported: bool,
    silence_reported: bool,
}

impl Recording {
    fn drain(&mut self, events: &tmpsc::UnboundedSender<AudioEvent>) {
        while let Ok(s) = self.ring.pop() {
            self.samples.push(s);
            self.block.push(s);
            if self.block.len() >= self.block_len {
                let l = level::level(&self.block);
                self.block.clear();
                // The actor may be shutting down; nothing to do then.
                let _ = events.send(AudioEvent::Level(l));
                if let Some(d) = &mut self.silence {
                    if d.feed(l, LEVEL_BLOCK_MS) && !self.silence_reported {
                        self.silence_reported = true;
                        let _ = events.send(AudioEvent::Silence);
                    }
                }
            }
        }
        if self.lost.load(Ordering::Relaxed) && !self.lost_reported {
            self.lost_reported = true;
            let _ = events.send(AudioEvent::DeviceLost);
        }
    }
}

fn run(rx: mpsc::Receiver<Cmd>, events: tmpsc::UnboundedSender<AudioEvent>) {
    let mut recording: Option<Recording> = None;
    loop {
        let cmd = if recording.is_some() {
            match rx.recv_timeout(DRAIN_EVERY) {
                Ok(c) => Some(c),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        } else {
            match rx.recv() {
                Ok(c) => Some(c),
                Err(_) => return,
            }
        };
        if let Some(rec) = &mut recording {
            rec.drain(&events);
        }
        match cmd {
            None => {}
            Some(Cmd::Start {
                device_id,
                silence_timeout_ms,
                reply,
            }) => {
                recording = None;
                let result = open(device_id.as_deref(), silence_timeout_ms).map(|(rec, info)| {
                    recording = Some(rec);
                    info
                });
                if let Err(e) = &result {
                    tracing::error!("microphone: {e}");
                }
                let _ = reply.send(result);
            }
            Some(Cmd::Stop { reply }) => {
                let result = match recording.take() {
                    Some(mut rec) => {
                        rec.drain(&events);
                        let rate = rec.rate;
                        drop(rec._stream);
                        resample::to_16k(&rec.samples, rate)
                    }
                    None => Err(AudioError::NotRecording),
                };
                let _ = reply.send(result);
            }
            Some(Cmd::Discard) => recording = None,
        }
    }
}

fn open(
    device_id: Option<&str>,
    silence_timeout_ms: Option<u64>,
) -> Result<(Recording, StartInfo), AudioError> {
    let host = cpal::default_host();
    let wanted = device_id
        .and_then(|id| cpal::DeviceId::from_str(id).ok())
        .and_then(|id| host.device_by_id(&id));
    let fell_back = device_id.is_some() && wanted.is_none();
    let device = wanted
        .or_else(|| host.default_input_device())
        .ok_or(AudioError::NoDevice)?;
    let supported = device
        .default_input_config()
        .map_err(|e| AudioError::Open(e.to_string()))?;
    let rate = supported.sample_rate();
    let channels = usize::from(supported.channels());
    let config = supported.config();

    // Two seconds of headroom: the drain runs every 20 ms.
    let (producer, consumer) = rtrb::RingBuffer::new(rate as usize * 2);
    let lost = Arc::new(AtomicBool::new(false));
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, &config, channels, producer, lost.clone()),
        SampleFormat::I16 => build::<i16>(&device, &config, channels, producer, lost.clone()),
        SampleFormat::U16 => build::<u16>(&device, &config, channels, producer, lost.clone()),
        SampleFormat::I32 => build::<i32>(&device, &config, channels, producer, lost.clone()),
        SampleFormat::U8 => build::<u8>(&device, &config, channels, producer, lost.clone()),
        other => return Err(AudioError::Format(other.to_string())),
    }?;
    stream.play().map_err(|e| AudioError::Open(e.to_string()))?;

    let block_len = (rate as u64 * LEVEL_BLOCK_MS / 1000) as usize;
    let info = StartInfo {
        device_name: device.to_string(),
        fell_back,
    };
    tracing::info!(device = %info.device_name, rate, channels, "recording started");
    Ok((
        Recording {
            _stream: stream,
            ring: consumer,
            rate,
            samples: Vec::with_capacity(rate as usize * 30),
            block: Vec::with_capacity(block_len),
            block_len,
            silence: silence_timeout_ms.map(SilenceDetector::new),
            lost,
            lost_reported: false,
            silence_reported: false,
        },
        info,
    ))
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    mut producer: rtrb::Producer<f32>,
    lost: Arc<AtomicBool>,
) -> Result<cpal::Stream, AudioError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = channels.max(1);
    device
        .build_input_stream(
            *config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                // Real-time context: downmix and push, nothing else.
                for frame in data.chunks_exact(channels) {
                    let sum: f32 = frame.iter().map(|&s| s.to_sample::<f32>()).sum();
                    // Ring full means the drain thread stalled: drop samples
                    // rather than block the audio callback.
                    let _ = producer.push(sum / channels as f32);
                }
            },
            move |e| {
                tracing::warn!("audio stream error: {e}");
                lost.store(true, Ordering::Relaxed);
            },
            Some(Duration::from_secs(3)),
        )
        .map_err(|e| AudioError::Open(e.to_string()))
}
