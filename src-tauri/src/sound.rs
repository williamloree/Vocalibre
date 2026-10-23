//! Feedback sounds, played on a dedicated thread. The output device is
//! opened on first use and released after a few idle seconds, so it is not
//! held open (and kept awake) all day.

use std::io::Cursor;
use std::sync::mpsc;
use std::time::Duration;

use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Source};

use crate::app::state::Sound;

const ON: &[u8] = include_bytes!("../../assets/on.wav");
const OFF: &[u8] = include_bytes!("../../assets/off.wav");
const FINISH: &[u8] = include_bytes!("../../assets/finish.wav");

/// Longer than any of the sounds, so none is cut short.
const RELEASE_AFTER: Duration = Duration::from_secs(5);

fn bytes(sound: Sound) -> &'static [u8] {
    match sound {
        Sound::Start => ON,
        Sound::Stop | Sound::Cancel => OFF,
        Sound::Done => FINISH,
    }
}

#[derive(Clone)]
pub struct Player {
    tx: mpsc::Sender<(Sound, f32)>,
}

impl Player {
    pub fn spawn() -> std::io::Result<Self> {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("sound".into())
            .spawn(move || run(rx))?;
        Ok(Self { tx })
    }

    /// `volume` 0.0 – 1.0. Never blocks; a failure is only logged.
    pub fn play(&self, sound: Sound, volume: f32) {
        let _ = self.tx.send((sound, volume));
    }
}

fn run(rx: mpsc::Receiver<(Sound, f32)>) {
    let mut sink: Option<MixerDeviceSink> = None;
    loop {
        let msg = if sink.is_some() {
            match rx.recv_timeout(RELEASE_AFTER) {
                Ok(m) => m,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    sink = None;
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        } else {
            match rx.recv() {
                Ok(m) => m,
                Err(_) => return,
            }
        };
        let (sound, volume) = msg;
        if sink.is_none() {
            match DeviceSinkBuilder::open_default_sink() {
                Ok(mut s) => {
                    s.log_on_drop(false);
                    sink = Some(s);
                }
                Err(e) => {
                    tracing::warn!("no audio output for sounds: {e}");
                    continue;
                }
            }
        }
        match Decoder::try_from(Cursor::new(bytes(sound))) {
            Ok(source) => {
                if let Some(s) = &sink {
                    s.mixer().add(source.amplify(volume.clamp(0.0, 1.0)));
                }
            }
            Err(e) => tracing::warn!("cannot decode {sound:?} sound: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_sounds_decode() {
        for s in [Sound::Start, Sound::Stop, Sound::Cancel, Sound::Done] {
            let d = Decoder::try_from(Cursor::new(bytes(s))).unwrap();
            assert!(d.total_duration().is_some_and(|t| t > Duration::ZERO));
        }
    }
}
