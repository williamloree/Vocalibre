//! Puts the transcription where the cursor is: paste through the clipboard
//! (saved and restored around it) or type it key by key.

use std::thread::sleep;
use std::time::Duration;

use arboard::{Clipboard, ImageData};
use enigo::{Direction, Enigo, Key, Keyboard};

use crate::settings::InjectionMode;

/// A dictation right after the previous one continues the same text.
pub const AUTO_SPACE_WINDOW_MS: u64 = 60_000;
/// Lets clipboard managers and the target app see the new content before
/// the paste keystroke.
const CLIPBOARD_SETTLE: Duration = Duration::from_millis(40);

#[derive(Debug, thiserror::Error)]
pub enum InjectError {
    #[error("clipboard unavailable: {0}")]
    Clipboard(#[from] arboard::Error),
    #[error("keyboard simulation unavailable: {0}")]
    Keyboard(String),
}

/// What was injected last, to decide on a leading space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LastInjection {
    pub at_ms: u64,
    pub ended_with_space: bool,
}

/// Adds a space before `text` when it continues a recent dictation that did
/// not end with whitespace. The cursor position is unknown to us, so this
/// only looks at our own previous injection.
pub fn with_leading_space(text: &str, last: Option<LastInjection>, now_ms: u64) -> String {
    let continues = last.is_some_and(|l| {
        !l.ended_with_space && now_ms.saturating_sub(l.at_ms) <= AUTO_SPACE_WINDOW_MS
    });
    let starts_attached = text
        .chars()
        .next()
        .is_none_or(|c| c.is_whitespace() || ",.;:!?)]}…".contains(c));
    if continues && !starts_attached {
        format!(" {text}")
    } else {
        text.to_string()
    }
}

pub fn ends_with_space(text: &str) -> bool {
    text.chars().next_back().is_some_and(char::is_whitespace)
}

enum Saved {
    Text(String),
    Image(ImageData<'static>),
    Empty,
}

/// Long-lived: on Linux the clipboard content is served by this process,
/// so the `Clipboard` must outlive the paste.
#[derive(Default)]
pub struct Injector {
    clipboard: Option<Clipboard>,
}

impl Injector {
    pub fn inject(
        &mut self,
        text: &str,
        mode: InjectionMode,
        restore_delay_ms: u32,
        keep_in_clipboard: bool,
    ) -> Result<(), InjectError> {
        match mode {
            InjectionMode::Paste => self.paste(text, restore_delay_ms, keep_in_clipboard),
            InjectionMode::Type => {
                enigo()?
                    .text(text)
                    .map_err(|e| InjectError::Keyboard(e.to_string()))?;
                if keep_in_clipboard {
                    self.clipboard()?.set_text(text)?;
                }
                Ok(())
            }
        }
    }

    /// Puts `text` in the clipboard (history "copy" button).
    pub fn copy(&mut self, text: &str) -> Result<(), InjectError> {
        self.clipboard()?.set_text(text)?;
        Ok(())
    }

    fn paste(
        &mut self,
        text: &str,
        restore_delay_ms: u32,
        keep_in_clipboard: bool,
    ) -> Result<(), InjectError> {
        let cb = self.clipboard()?;
        let saved = if keep_in_clipboard {
            None
        } else {
            Some(save(cb))
        };
        cb.set_text(text)?;
        sleep(CLIPBOARD_SETTLE);
        let pasted = paste_shortcut();
        if let Some(saved) = saved {
            // Restore even if the keystroke failed: the user's clipboard
            // matters more than our text.
            sleep(Duration::from_millis(restore_delay_ms.into()));
            let cb = self.clipboard()?;
            match saved {
                Saved::Text(t) => cb.set_text(t)?,
                Saved::Image(i) => cb.set_image(i)?,
                Saved::Empty => cb.clear()?,
            }
        }
        pasted
    }

    fn clipboard(&mut self) -> Result<&mut Clipboard, InjectError> {
        match &mut self.clipboard {
            Some(cb) => Ok(cb),
            slot => Ok(slot.insert(Clipboard::new()?)),
        }
    }
}

fn save(cb: &mut Clipboard) -> Saved {
    if let Ok(t) = cb.get_text() {
        return Saved::Text(t);
    }
    match cb.get_image() {
        Ok(i) => Saved::Image(i),
        Err(_) => Saved::Empty,
    }
}

fn enigo() -> Result<Enigo, InjectError> {
    Enigo::new(&enigo::Settings::default()).map_err(|e| InjectError::Keyboard(e.to_string()))
}

fn paste_shortcut() -> Result<(), InjectError> {
    let modifier = if cfg!(target_os = "macos") {
        Key::Meta
    } else {
        Key::Control
    };
    let mut k = enigo()?;
    let err = |e: enigo::InputError| InjectError::Keyboard(e.to_string());
    k.key(modifier, Direction::Press).map_err(err)?;
    let clicked = k.key(Key::Unicode('v'), Direction::Click).map_err(err);
    // Always release the modifier, or it stays stuck for the user.
    k.key(modifier, Direction::Release).map_err(err)?;
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECENT: Option<LastInjection> = Some(LastInjection {
        at_ms: 1_000,
        ended_with_space: false,
    });

    #[test]
    fn first_dictation_gets_no_space() {
        assert_eq!(with_leading_space("Bonjour", None, 0), "Bonjour");
    }

    #[test]
    fn recent_continuation_gets_a_space() {
        assert_eq!(with_leading_space("Suite.", RECENT, 5_000), " Suite.");
    }

    #[test]
    fn old_previous_dictation_gets_no_space() {
        let now = 1_000 + AUTO_SPACE_WINDOW_MS + 1;
        assert_eq!(with_leading_space("Suite.", RECENT, now), "Suite.");
    }

    #[test]
    fn previous_ending_with_space_or_newline_gets_no_space() {
        let last = Some(LastInjection {
            at_ms: 1_000,
            ended_with_space: true,
        });
        assert_eq!(with_leading_space("Suite", last, 2_000), "Suite");
        assert!(ends_with_space("Fin\n"));
        assert!(!ends_with_space("Fin."));
        assert!(!ends_with_space(""));
    }

    #[test]
    fn punctuation_and_empty_text_stay_attached() {
        assert_eq!(with_leading_space(", et puis", RECENT, 2_000), ", et puis");
        assert_eq!(with_leading_space("\nOui", RECENT, 2_000), "\nOui");
        assert_eq!(with_leading_space("", RECENT, 2_000), "");
    }
}
