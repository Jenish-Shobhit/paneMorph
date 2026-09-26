//! Carrying out a window's choice.
//!
//! herdr closes a popup when its owner tab closes (open question 7), which
//! is exactly what happens when Send moves the last pane of that tab. The
//! window therefore hands the move to a detached `panemorph apply` worker in
//! its own session: the worker finishes the move, the focus call, the tab
//! placement, the journal and the log even if the popup is torn down, and
//! reports back through a result file (edge cases 1.6, 6.2, 6.20).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::api::Herdr;
use crate::exec::{Exec, Outcome};
use crate::journal::Journal;
use crate::model::SplitDir;
use crate::plan::FetchTarget;
use crate::state::{append_log, now_ms, QueueLock};
use crate::ui::rows::RowAction;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    /// The moving pane (Send) or your pane (Fetch).
    pub source: String,
    pub action: RowAction,
    pub split: SplitDir,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "message", rename_all = "snake_case")]
pub enum Reply {
    /// The move happened; the window closes.
    Moved(String),
    /// Nothing moved, and the window says why.
    Info(String),
    /// It failed; the window shows the error inline and stays open.
    Error(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkerArgs {
    pub request: Request,
    pub result: PathBuf,
}

/// Do the move, journal it and log it. Returns the reply for the window and
/// problems to report once the window has gone.
pub fn apply(herdr: &dyn Herdr, state_dir: &Path, request: &Request) -> (Reply, Vec<String>) {
    let _lock = QueueLock::acquire(state_dir).ok();
    let exec = Exec::new(herdr);
    let (result, action) = match &request.action {
        RowAction::Send(target) => (
            exec.send(&request.source, target, request.split, "send"),
            "send",
        ),
        RowAction::FetchPane(pane_id) => (
            exec.fetch(
                &request.source,
                &FetchTarget::Pane(pane_id.clone()),
                request.split,
                "fetch",
            ),
            "fetch",
        ),
        RowAction::FetchTab(tab_id) => (
            exec.fetch(
                &request.source,
                &FetchTarget::Tab(tab_id.clone()),
                request.split,
                "fetch-tab",
            ),
            "fetch-tab",
        ),
        RowAction::OpenSpace(_) | RowAction::Back | RowAction::None => {
            return (Reply::Info(String::new()), Vec::new())
        }
    };
    let mut journal = Journal::load(state_dir, &herdr.session_key());
    match result {
        Ok(Outcome::Done(done)) => {
            append_log(state_dir, &done.log);
            if let Some(entry) = done.entry {
                journal.push(*entry);
                let _ = journal.save();
            }
            for warning in &done.warnings {
                append_log(state_dir, &format!("{action} warning: {warning}"));
            }
            (Reply::Moved(done.summary), done.warnings)
        }
        Ok(Outcome::NoOp(no_op)) => (Reply::Info(no_op.0), Vec::new()),
        Ok(Outcome::Notice(text)) => (Reply::Info(text), Vec::new()),
        Err(failure) => {
            append_log(state_dir, &format!("{action} error={}", failure.message));
            if let Some(entry) = failure.entry {
                journal.push(*entry);
                let _ = journal.save();
            }
            (Reply::Error(failure.message), Vec::new())
        }
    }
}

/// Write the reply atomically so the window never reads half a file.
pub fn write_reply(path: &Path, reply: &Reply) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(
        &tmp,
        serde_json::to_vec(reply).map_err(std::io::Error::other)?,
    )?;
    std::fs::rename(tmp, path)
}

/// Run the move in a detached worker and wait for its reply.
pub fn run_worker(state_dir: &Path, request: &Request) -> Result<Reply, String> {
    std::fs::create_dir_all(state_dir).map_err(|e| e.to_string())?;
    let result = state_dir.join(format!("apply-{}-{}.json", std::process::id(), now_ms()));
    let args = serde_json::to_string(&WorkerArgs {
        request: request.clone(),
        result: result.clone(),
    })
    .map_err(|e| e.to_string())?;
    let mut child = crate::actions::spawn_detached_child(&["apply", &args])
        .map_err(|e| format!("couldn't start the move: {e}"))?;
    let read = |path: &Path| -> Option<Reply> {
        let text = std::fs::read_to_string(path).ok()?;
        let _ = std::fs::remove_file(path);
        serde_json::from_str(&text).ok()
    };
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(reply) = read(&result) {
            return Ok(reply);
        }
        if let Ok(Some(status)) = child.try_wait() {
            return read(&result).ok_or_else(|| {
                format!("the move stopped unexpectedly ({status}); see panemorph.log")
            });
        }
        if Instant::now() > deadline {
            return Err("the move is taking too long; see panemorph.log".into());
        }
        std::thread::sleep(Duration::from_millis(3));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::SendTarget;
    use crate::sim::{Sim, SAMPLE_FIXTURE};

    #[test]
    fn request_and_reply_round_trip_as_json() {
        let request = Request {
            source: "w1:p2".into(),
            action: RowAction::Send(SendTarget::NewTabIn("w2".into())),
            split: SplitDir::Down,
        };
        let text = serde_json::to_string(&request).unwrap();
        assert_eq!(serde_json::from_str::<Request>(&text).unwrap(), request);
        let reply = Reply::Error("That pane closed".into());
        let text = serde_json::to_string(&reply).unwrap();
        assert_eq!(text, r#"{"kind":"error","message":"That pane closed"}"#);
    }

    /// Edge cases 1.6 and 5.7 through the window path: the move of a tab's
    /// last pane is journaled so ⌃⌥Z can recreate the tab.
    #[test]
    fn apply_journals_and_logs_the_move() {
        let sim = Sim::from_json(SAMPLE_FIXTURE).unwrap();
        let dir = std::env::temp_dir().join(format!("pm-apply-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let request = Request {
            source: sim.pane_id("zsh"),
            action: RowAction::Send(SendTarget::Tab(sim.tab_id_of("scraper"))),
            split: SplitDir::Right,
        };
        let (reply, warnings) = apply(&sim, &dir, &request);
        assert_eq!(reply, Reply::Moved("Sent zsh to Load tests".into()));
        assert!(warnings.is_empty());
        let journal = Journal::load(&dir, "sim");
        assert!(journal.last().unwrap().source_tab_closed);
        assert!(std::fs::read_to_string(dir.join("panemorph.log"))
            .unwrap()
            .contains("send terminal="));
        let path = dir.join("reply.json");
        write_reply(&path, &reply).unwrap();
        assert_eq!(
            serde_json::from_str::<Reply>(&std::fs::read_to_string(&path).unwrap()).unwrap(),
            reply
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Edge case 6.20 (with 3.7): a problem after the pane moved goes back to
    /// the worker as a warning, which reports it once the popup has gone.
    #[test]
    fn edge_6_20_late_problems_are_returned_to_the_worker() {
        let sim = Sim::from_json(SAMPLE_FIXTURE).unwrap();
        sim.inject(crate::sim::Fault::FailTabMove);
        let dir = std::env::temp_dir().join(format!("pm-apply-late-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let request = Request {
            source: sim.pane_id("portfolio"),
            action: RowAction::Send(SendTarget::NewTabHere),
            split: SplitDir::Right,
        };
        let (reply, warnings) = apply(&sim, &dir, &request);
        assert!(matches!(reply, Reply::Moved(_)));
        assert_eq!(
            warnings,
            ["Moved, but couldn't place the new tab next to this one."]
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
