//! A machine's name (Arugula #500): the display name its owner (or its
//! team's owners) gave it, and the word `@machine` mentions it by.
//!
//! The name is for people. A machine is `machine:<id>`; nothing authorizes
//! by name.

use serde::{Deserialize, Serialize};

/// The longest a machine's name is, in characters.
pub const NAME_MAX: usize = 40;

/// `POST /api/daemons/{id}/name`: a new name for a machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct MachineRename {
    pub name: String,
}

/// The word an `@machine` mention names a machine by: its name lowercased,
/// every run of anything but letters, digits, `_`, `-` and `.` one dash,
/// and no dash or dot at either end (the way a mention's word ends). The
/// hostname `Aarons-Mac-mini.local` is `aarons-mac-mini.local`, as it was
/// before machines could be renamed; "My Mini" is `my-mini`.
pub fn handle(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() || matches!(c, '_' | '.') {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches(['.', '-']).to_owned()
}

/// A name as it is kept, or why it can't be: trimmed, 1 to [`NAME_MAX`]
/// characters, no control characters, and a [`handle`] that isn't empty (a
/// name of symbols alone couldn't be mentioned).
pub fn clean_name(name: &str) -> Result<String, &'static str> {
    let name = name.trim();
    if name.is_empty() {
        return Err("a machine's name can't be empty");
    }
    if name.chars().count() > NAME_MAX {
        return Err("a machine's name is at most 40 characters");
    }
    if name.chars().any(char::is_control) {
        return Err("a machine's name is one line of text");
    }
    if handle(name).is_empty() {
        return Err("a machine's name needs a letter or a digit, to be mentioned by");
    }
    Ok(name.to_owned())
}

/// Whether `word` (an `@word`, lowercased) is the handle of `name`.
pub fn is_current(name: &str, word: &str) -> bool {
    handle(name) == word
}

/// The machines `word` names: those it is the current handle of, else those
/// that went by it once. A machine renamed to a word another still has as an
/// old one takes the word from it.
pub fn named_by<'a, T>(
    items: impl IntoIterator<Item = &'a T> + Clone,
    word: &str,
    name: impl Fn(&T) -> &str,
    aliases: impl Fn(&T) -> &[String],
) -> Vec<&'a T> {
    let now: Vec<&T> = items.clone().into_iter().filter(|i| is_current(name(i), word)).collect();
    if !now.is_empty() {
        return now;
    }
    items.into_iter().filter(|i| aliases(i).iter().any(|a| a == word)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hostname_keeps_its_old_mention_word() {
        assert_eq!(handle("Aarons-Mac-mini.local"), "aarons-mac-mini.local");
        assert_eq!(handle("My Mini"), "my-mini");
        assert_eq!(handle("  mini. "), "mini");
        assert_eq!(handle("***"), "");
    }

    #[test]
    fn a_name_is_trimmed_short_and_mentionable() {
        assert_eq!(clean_name("  mini  "), Ok("mini".into()));
        assert!(clean_name("   ").is_err());
        assert!(clean_name(&"x".repeat(41)).is_err());
        assert!(clean_name(&"x".repeat(40)).is_ok());
        assert!(clean_name("!!!").is_err());
        assert!(clean_name("a\nb").is_err());
    }

    #[test]
    fn the_current_name_beats_an_old_one() {
        let items = [("mini", vec!["box".to_owned()]), ("box", vec![])];
        let got =
            |w: &str| named_by(items.iter(), w, |i| i.0, |i| i.1.as_slice()).iter().map(|i| i.0).collect::<Vec<_>>();
        assert_eq!(got("box"), ["box"], "the machine called box now, not the one that was");
        assert_eq!(got("mini"), ["mini"]);
        assert!(got("nope").is_empty());
        let only = [("mini", vec!["box".to_owned()])];
        assert_eq!(named_by(only.iter(), "box", |i| i.0, |i| i.1.as_slice()).len(), 1);
    }
}
