//! Sync between PCs: "my words" and "my translations" kept in a folder that a service the person already has (OneDrive, Dropbox, Google Drive,
//! a network drive) copies from one PC to the others. Verba talks to no server: it reads and writes one text file in that folder.
//!
//! The file is the backup format (`backup.rs`) without settings. Each PC also remembers what the file said the last time it was in
//! agreement (`sync-base.txt` next to the other files of Verba). With that, a change is told from a stale copy: this is a three-way merge.
//! A word or a translation that one side added, changed or removed since then is taken; when both sides changed the same one, the side whose file
//! is newer wins. So a word removed on one PC is removed on the others, instead of coming back from a PC that has not heard of it.
//! Pure functions, no files: `main.rs` reads and writes them.

use std::collections::BTreeMap;

use verba_local::{UserEntry, UserTranslations};

use crate::backup;

/// The words and the translations of one PC, or of the file, as maps from a key (the same word spelled alike is one) to the thing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    /// Lower-case word to the word as written.
    pub words: BTreeMap<String, String>,
    /// `UserEntry::key` to the entry.
    pub entries: BTreeMap<String, UserEntry>,
}

impl State {
    pub fn from_parts(words: &[String], translations: &UserTranslations) -> Self {
        Self {
            words: words.iter().map(|w| (w.to_lowercase(), w.clone())).collect(),
            entries: translations.entries().iter().map(|e| (e.key(), e.clone())).collect(),
        }
    }

    /// The state a file says, `None` when it is not a Verba backup.
    pub fn from_text(text: &str) -> Option<Self> {
        let b = backup::parse(text)?;
        Some(Self::from_parts(&b.words, &b.translations))
    }

    /// The text of the sync file (no settings: the PCs keep their own).
    pub fn to_text(&self) -> String {
        let words: Vec<String> = self.words.values().cloned().collect();
        let mut tr = UserTranslations::new();
        for e in self.entries.values() {
            let _ = tr.add(e.from, e.to, &e.word, &e.translations.join("; "));
        }
        backup::render(&words, &tr, "")
    }

    pub fn word_list(&self) -> Vec<String> {
        self.words.values().cloned().collect()
    }

    pub fn translations(&self) -> UserTranslations {
        let mut tr = UserTranslations::new();
        for e in self.entries.values() {
            let _ = tr.add(e.from, e.to, &e.word, &e.translations.join("; "));
        }
        tr
    }
}

/// Merges one kind of thing. For each key: unchanged on a side means the other side's version is taken; changed on both: `remote_newer` decides.
fn merge_map<V: Clone + PartialEq>(base: &BTreeMap<String, V>, local: &BTreeMap<String, V>, remote: &BTreeMap<String, V>, remote_newer: bool) -> BTreeMap<String, V> {
    let mut keys: Vec<&String> = base.keys().chain(local.keys()).chain(remote.keys()).collect();
    keys.sort();
    keys.dedup();
    let mut out = BTreeMap::new();
    for k in keys {
        let (b, l, r) = (base.get(k), local.get(k), remote.get(k));
        let chosen = if l == r {
            l
        } else if l == b {
            r
        } else if r == b {
            l
        } else if remote_newer {
            r
        } else {
            l
        };
        if let Some(v) = chosen {
            out.insert(k.clone(), v.clone());
        }
    }
    out
}

/// The three-way merge of the two PCs' states over what they agreed on last (`base`, empty the first time).
pub fn merge(base: &State, local: &State, remote: &State, remote_newer: bool) -> State {
    State {
        words: merge_map(&base.words, &local.words, &remote.words, remote_newer),
        entries: merge_map(&base.entries, &local.entries, &remote.entries, remote_newer),
    }
}

/// What a sync changed on this PC, for the message to the person.
#[derive(Debug, Default, PartialEq)]
pub struct Report {
    pub words_added: usize,
    pub words_removed: usize,
    pub translations_changed: usize,
    /// The file in the folder was written (this PC had news for the others).
    pub file_written: bool,
}

/// What changed between this PC's state and the merged one.
pub fn report(local: &State, merged: &State, remote: &State) -> Report {
    Report {
        words_added: merged.words.keys().filter(|k| !local.words.contains_key(*k)).count(),
        words_removed: local.words.keys().filter(|k| !merged.words.contains_key(*k)).count(),
        translations_changed: merged.entries.iter().filter(|(k, v)| local.entries.get(*k) != Some(*v)).count()
            + local.entries.keys().filter(|k| !merged.entries.contains_key(*k)).count(),
        file_written: merged != remote,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verba_core::Lang;

    fn state(words: &[&str], tr: &[(&str, &str)]) -> State {
        let mut t = UserTranslations::new();
        for (w, x) in tr {
            t.add(Lang::Ar, Lang::En, w, x).unwrap();
        }
        State::from_parts(&words.iter().map(|w| w.to_string()).collect::<Vec<_>>(), &t)
    }

    #[test]
    fn the_first_sync_joins_what_both_have() {
        let m = merge(&State::default(), &state(&["Snow", "Verba"], &[("كتاب", "book")]), &state(&["Verba", "Positiv"], &[("بيت", "house")]), true);
        assert_eq!(m.word_list(), ["Positiv", "Snow", "Verba"]);
        assert_eq!(m.entries.len(), 2);
    }

    #[test]
    fn a_word_removed_on_one_pc_is_removed_on_the_other() {
        let base = state(&["Snow", "Verba"], &[]);
        // this PC removed Snow; the other did nothing since
        let m = merge(&base, &state(&["Verba"], &[]), &base, true);
        assert_eq!(m.word_list(), ["Verba"]);
        // the other PC removed Verba; this one added Positiv
        let m = merge(&base, &state(&["Snow", "Verba", "Positiv"], &[]), &state(&["Snow"], &[]), false);
        assert_eq!(m.word_list(), ["Positiv", "Snow"]);
    }

    #[test]
    fn a_change_is_taken_from_the_side_that_made_it_and_the_newer_wins_a_conflict() {
        let base = state(&[], &[("كتاب", "book")]);
        // only the other PC changed it
        let m = merge(&base, &base, &state(&[], &[("كتاب", "volume")]), false);
        assert_eq!(m.entries.values().next().unwrap().translations, ["volume"]);
        // only this PC changed it
        let m = merge(&base, &state(&[], &[("كتاب", "novel")]), &base, true);
        assert_eq!(m.entries.values().next().unwrap().translations, ["novel"]);
        // both changed it: the newer file wins
        let (mine, theirs) = (state(&[], &[("كتاب", "novel")]), state(&[], &[("كتاب", "volume")]));
        assert_eq!(merge(&base, &mine, &theirs, true).entries.values().next().unwrap().translations, ["volume"]);
        assert_eq!(merge(&base, &mine, &theirs, false).entries.values().next().unwrap().translations, ["novel"]);
    }

    #[test]
    fn nothing_changed_means_nothing_to_do() {
        let s = state(&["Snow"], &[("كتاب", "book")]);
        let m = merge(&s, &s, &s, true);
        assert_eq!(m, s);
        assert_eq!(report(&s, &m, &s), Report::default());
    }

    #[test]
    fn the_report_counts_what_changed_here_and_whether_the_file_needs_writing() {
        let local = state(&["Snow"], &[("كتاب", "book")]);
        let remote = state(&["Snow", "Verba"], &[("كتاب", "book"), ("بيت", "house")]);
        let m = merge(&State::default(), &local, &remote, true);
        let r = report(&local, &m, &remote);
        assert_eq!((r.words_added, r.words_removed, r.translations_changed, r.file_written), (1, 0, 1, false));
        let r = report(&remote, &m, &state(&[], &[]));
        assert!(r.file_written);
    }

    #[test]
    fn the_file_round_trips_and_has_no_settings() {
        let s = state(&["Snow"], &[("كتاب", "book; volume")]);
        let text = s.to_text();
        assert_eq!(State::from_text(&text), Some(s));
        assert!(State::from_text("theme=dark\n").is_none(), "a settings file is not a sync file");
    }
}
