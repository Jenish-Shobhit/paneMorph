//! The undo journal: the last 20 moves of one herdr session, tracked by
//! terminal id (edge cases 5.1, 5.2, 5.15).

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::topology::Tree;

pub const CAPACITY: usize = 20;

/// Where a pane (or tab) came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Place {
    pub workspace_id: String,
    pub workspace_label: String,
    pub workspace_index: usize,
    pub tab_id: String,
    /// Custom tab label; `None` for an untitled tab (edge case 5.7).
    pub tab_label: Option<String>,
    pub tab_index: usize,
}

/// One move paneMorph made (edge case 5.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Entry {
    pub action: String,
    pub at_ms: u64,
    /// Moved terminals, in move order.
    pub terminals: Vec<String>,
    /// Their pane ids right after the move.
    pub pane_ids_after: Vec<String>,
    /// The tab the panes landed in.
    pub dest_tab_id: String,
    pub source: Place,
    /// The source tab's split tree before the move, leaves are terminal ids.
    pub source_tree: Option<Tree>,
    pub source_tab_closed: bool,
    pub source_space_closed: bool,
    /// The moved pane was zoomed and paneMorph unzoomed it (1.11, 5.16).
    pub rezoom_terminal: Option<String>,
    /// Focus went with the pane.
    pub followed: bool,
    /// A whole-tab fetch (edge case 5.10).
    pub whole_tab: bool,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct File {
    session: String,
    entries: Vec<Entry>,
}

/// The journal file for one session, keyed by its socket path.
pub struct Journal {
    path: PathBuf,
    session: String,
    pub entries: Vec<Entry>,
}

/// FNV-1a, stable across builds and platforms.
fn fnv64(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

pub fn journal_path(state_dir: &Path, session: &str) -> PathBuf {
    state_dir.join(format!("journal-{:016x}.json", fnv64(session)))
}

impl Journal {
    pub fn load(state_dir: &Path, session: &str) -> Self {
        let path = journal_path(state_dir, session);
        let entries = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<File>(&text).ok())
            .filter(|file| file.session == session)
            .map(|file| file.entries)
            .unwrap_or_default();
        Self {
            path,
            session: session.to_string(),
            entries,
        }
    }

    pub fn push(&mut self, entry: Entry) {
        self.entries.push(entry);
        if self.entries.len() > CAPACITY {
            let excess = self.entries.len() - CAPACITY;
            self.entries.drain(..excess);
        }
    }

    pub fn last(&self) -> Option<&Entry> {
        self.entries.last()
    }

    pub fn pop(&mut self) -> Option<Entry> {
        self.entries.pop()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Undo recreated a closed tab under a new id; older entries that name
    /// the old id now mean the new tab (edge case 5.7).
    pub fn rename_tab(&mut self, old: &str, new: &str) {
        for entry in &mut self.entries {
            if entry.source.tab_id == old {
                entry.source.tab_id = new.to_string();
            }
            if entry.dest_tab_id == old {
                entry.dest_tab_id = new.to_string();
            }
        }
    }

    /// Undo recreated a closed space under a new id (edge case 5.8).
    pub fn rename_space(&mut self, old: &str, new: &str) {
        for entry in &mut self.entries {
            if entry.source.workspace_id == old {
                entry.source.workspace_id = new.to_string();
            }
        }
    }

    /// Write atomically: temp file, then rename.
    pub fn save(&self) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let file = File {
            session: self.session.clone(),
            entries: self.entries.clone(),
        };
        let tmp = self
            .path
            .with_extension(format!("tmp{}", std::process::id()));
        {
            let mut out = std::fs::File::create(&tmp)?;
            out.write_all(&serde_json::to_vec_pretty(&file).map_err(std::io::Error::other)?)?;
            out.sync_all()?;
        }
        std::fs::rename(&tmp, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pm-journal-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn entry(n: usize) -> Entry {
        Entry {
            action: "send".into(),
            terminals: vec![format!("term-{n}")],
            ..Default::default()
        }
    }

    /// Edge case 5.2: the journal keeps the last 20 moves.
    #[test]
    fn edge_5_2_keeps_last_twenty() {
        let dir = temp_dir("cap");
        let mut journal = Journal::load(&dir, "/s/herdr.sock");
        for n in 0..25 {
            journal.push(entry(n));
        }
        journal.save().unwrap();
        let journal = Journal::load(&dir, "/s/herdr.sock");
        assert_eq!(journal.entries.len(), CAPACITY);
        assert_eq!(journal.entries[0].terminals[0], "term-5");
        assert_eq!(journal.last().unwrap().terminals[0], "term-24");
    }

    /// Edge case 5.15: each session has its own journal file.
    #[test]
    fn edge_5_15_sessions_do_not_share_a_journal() {
        let dir = temp_dir("sessions");
        let mut a = Journal::load(&dir, "/a/herdr.sock");
        a.push(entry(1));
        a.save().unwrap();
        let b = Journal::load(&dir, "/b/herdr.sock");
        assert!(b.entries.is_empty());
        assert_ne!(
            journal_path(&dir, "/a/herdr.sock"),
            journal_path(&dir, "/b/herdr.sock")
        );
    }

    /// Edge case 5.3: popping walks back one move at a time.
    #[test]
    fn edge_5_3_each_undo_takes_the_next_older_move() {
        let dir = temp_dir("pop");
        let mut journal = Journal::load(&dir, "s");
        journal.push(entry(1));
        journal.push(entry(2));
        assert_eq!(journal.pop().unwrap().terminals[0], "term-2");
        assert_eq!(journal.pop().unwrap().terminals[0], "term-1");
        assert!(journal.pop().is_none());
    }

    #[test]
    fn corrupt_journal_reads_as_empty() {
        let dir = temp_dir("corrupt");
        std::fs::write(journal_path(&dir, "s"), "{not json").unwrap();
        assert!(Journal::load(&dir, "s").entries.is_empty());
    }
}
