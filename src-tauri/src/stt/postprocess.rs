//! Text clean-up applied to every transcription, whatever the engine.

use serde::{Deserialize, Serialize};

/// Phrases Whisper is known to hallucinate on near-silence (learned from
/// subtitled training data). Compared after [`normalize_for_match`].
const HALLUCINATIONS: &[&str] = &[
    "sous-titres réalisés par la communauté d'amara.org",
    "sous-titres réalisés para la communauté d'amara.org",
    "sous-titrage st' 501",
    "sous-titrage st'501",
    "sous-titrage société radio-canada",
    "merci d'avoir regardé",
    "merci d'avoir regardé cette vidéo",
    "n'oubliez pas de vous abonner",
    "abonnez-vous",
    "subtitles by the amara.org community",
    "thank you for watching",
    "thanks for watching",
    "please subscribe",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Replacement {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    pub capitalize_first: bool,
    pub strip_final_period: bool,
    pub replacements: Vec<Replacement>,
}

pub fn process(raw: &str, opts: &Options) -> String {
    let mut text = collapse_whitespace(raw);
    text = strip_hallucinations(&text);
    for r in &opts.replacements {
        text = replace_phrase(&text, &r.from, &r.to);
    }
    text = tidy_punctuation(&text);
    if opts.strip_final_period && text.ends_with('.') && !text.ends_with("..") {
        text.pop();
    }
    if opts.capitalize_first {
        text = capitalize_first(&text);
    }
    text
}

fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Lowercase, unify apostrophes, drop surrounding punctuation and collapse
/// spaces, so "  Merci d’avoir regardé ! " matches "merci d'avoir regardé".
fn normalize_for_match(s: &str) -> String {
    let s = s.to_lowercase().replace('’', "'");
    collapse_whitespace(&s)
        .trim_matches(|c: char| c.is_ascii_punctuation() && c != '\'' || c == '…' || c == ' ')
        .to_string()
}

/// Removes a known hallucination when it is the whole text or its last
/// sentence. Never touches a phrase in the middle of real dictation.
fn strip_hallucinations(text: &str) -> String {
    let mut text = text.to_string();
    loop {
        let (head, last) = split_last_sentence(&text);
        if !HALLUCINATIONS.contains(&normalize_for_match(last).as_str()) {
            return text;
        }
        text = head.trim_end().to_string();
        if text.is_empty() {
            return text;
        }
    }
}

/// Splits after the last sentence terminator followed by a space (so the dot
/// in "Amara.org" is not a sentence end).
fn split_last_sentence(text: &str) -> (&str, &str) {
    let trimmed = text.trim_end_matches(|c: char| ".!?… ".contains(c));
    let mut split = 0;
    let mut prev_terminator = false;
    for (i, c) in trimmed.char_indices() {
        if c == ' ' && prev_terminator {
            split = i;
        }
        prev_terminator = ".!?…".contains(c);
    }
    text.split_at(split)
}

/// Case-insensitive, whole-word replacement.
fn replace_phrase(text: &str, from: &str, to: &str) -> String {
    let from = from.trim();
    if from.is_empty() {
        return text.to_string();
    }
    let lower = text.to_lowercase();
    let needle = from.to_lowercase();
    // Lowercasing can change byte lengths (rare Unicode cases): fall back to
    // no replacement rather than slicing at wrong offsets.
    if lower.len() != text.len() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while let Some(pos) = lower[i..].find(&needle) {
        let start = i + pos;
        let end = start + needle.len();
        let before_ok = text[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric());
        let after_ok = text[end..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric());
        out.push_str(&text[i..start]);
        if before_ok && after_ok {
            out.push_str(to);
        } else {
            out.push_str(&text[start..end]);
        }
        i = end;
    }
    out.push_str(&text[i..]);
    out
}

/// Cleans spacing left by replacements: no space before `,` `.` or around
/// newlines, and a period right after a spoken punctuation is dropped.
fn tidy_punctuation(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            ',' | '.' | '\n' => {
                while out.ends_with(' ') {
                    out.pop();
                }
                // "virgule." → ",." → ","; "point à la ligne." → "\n."
                if c == '.' && (out.ends_with(',') || out.ends_with('\n')) {
                    continue;
                }
                if c == ',' && out.ends_with('\n') {
                    continue;
                }
                out.push(c);
            }
            ' ' if out.ends_with('\n') || out.ends_with(' ') => {}
            _ => out.push(c),
        }
    }
    out.trim_matches(' ').to_string()
}

fn capitalize_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(s: &str) -> String {
        process(s, &Options::default())
    }

    #[test]
    fn trims_and_collapses_whitespace() {
        assert_eq!(plain("  Bonjour   le\tmonde  "), "Bonjour le monde");
    }

    #[test]
    fn whole_text_hallucination_is_removed() {
        assert_eq!(
            plain("Sous-titres réalisés par la communauté d'Amara.org"),
            ""
        );
        assert_eq!(plain(" Merci d’avoir regardé ! "), "");
        assert_eq!(plain("Thanks for watching."), "");
    }

    #[test]
    fn trailing_hallucination_is_removed() {
        assert_eq!(
            plain("Le rendez-vous est à midi. Sous-titrage ST' 501"),
            "Le rendez-vous est à midi."
        );
        assert_eq!(
            plain("C'est noté. Merci d'avoir regardé. Abonnez-vous !"),
            "C'est noté."
        );
    }

    #[test]
    fn phrase_in_the_middle_is_kept() {
        let s = "Il m'a dit merci d'avoir regardé le dossier hier.";
        assert_eq!(plain(s), s);
    }

    #[test]
    fn legitimate_short_text_is_kept() {
        assert_eq!(plain("Thank you."), "Thank you.");
    }

    #[test]
    fn capitalize_and_strip_period() {
        let opts = Options {
            capitalize_first: true,
            strip_final_period: true,
            ..Options::default()
        };
        assert_eq!(process("élément suivant.", &opts), "Élément suivant");
        assert_eq!(process("et puis...", &opts), "Et puis...");
    }

    #[test]
    fn replacements_are_whole_word_and_case_insensitive() {
        let opts = Options {
            replacements: vec![
                Replacement {
                    from: "virgule".into(),
                    to: ",".into(),
                },
                Replacement {
                    from: "point à la ligne".into(),
                    to: "\n".into(),
                },
            ],
            ..Options::default()
        };
        assert_eq!(
            process("Bonjour virgule ça va. Point à la ligne. Oui", &opts),
            "Bonjour, ça va.\nOui"
        );
        assert_eq!(
            process("Les virgules restent.", &opts),
            "Les virgules restent."
        );
    }

    #[test]
    fn empty_replacement_source_is_ignored() {
        let opts = Options {
            replacements: vec![Replacement {
                from: " ".into(),
                to: "x".into(),
            }],
            ..Options::default()
        };
        assert_eq!(process("a b", &opts), "a b");
    }
}
