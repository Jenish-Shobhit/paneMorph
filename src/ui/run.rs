//! The window's terminal loop.

use std::collections::HashSet;
use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::api::{self, Herdr};
use crate::exec::{Exec, Outcome};
use crate::journal::Journal;
use crate::model::Command;
use crate::plan::{self, FetchTarget};
use crate::state::{append_log, QueueLock};
use crate::ui::app::{map_key, App, Effect, Mode};
use crate::ui::rows::RowAction;
use crate::ui::view;

/// Events that can change what a window lists (edge case 6.15).
pub const EVENT_TYPES: &[&str] = &[
    "workspace.created",
    "workspace.closed",
    "workspace.moved",
    "workspace.renamed",
    "tab.created",
    "tab.closed",
    "tab.moved",
    "tab.renamed",
    "pane.created",
    "pane.closed",
    "pane.moved",
    "pane.updated",
    "pane.agent_status_changed",
    "layout.updated",
];

pub struct Options {
    pub mode: Mode,
    pub source_id: Option<String>,
    pub state_dir: PathBuf,
    /// Preview mode switches windows in place instead of reopening a popup.
    pub switch_in_place: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Exit {
    Closed,
    Moved {
        summary: String,
        warnings: Vec<String>,
    },
    Switched(Mode),
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(
        io::stdout(),
        DisableMouseCapture,
        LeaveAlternateScreen,
        crossterm::cursor::Show
    );
}

/// Fetch running commands in the background, in the order given (visible
/// rows first), without blocking the first draw (edge case 2.12).
fn fetch_commands(
    herdr: Arc<dyn Herdr>,
    panes: Vec<(String, String)>,
) -> Receiver<(String, Command)> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        for (pane_id, terminal) in panes {
            if let Ok(info) = api::process_info(&*herdr, &pane_id) {
                if let Some(command) = info.command() {
                    if sender.send((terminal, command)).is_err() {
                        return;
                    }
                }
            }
        }
    });
    receiver
}

/// Panes whose command is still unknown, visible rows first.
fn wanted(app: &App, requested: &HashSet<String>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut push = |pane: &crate::model::PaneInfo| {
        if crate::names::agent(pane).is_none()
            && !requested.contains(&pane.terminal_id)
            && !app.commands.contains_key(&pane.terminal_id)
            && !out
                .iter()
                .any(|(_, t): &(String, String)| t == &pane.terminal_id)
        {
            out.push((pane.pane_id.clone(), pane.terminal_id.clone()));
        }
    };
    if let Some(source) = &app.source {
        push(source);
    }
    if app.mode == Mode::Fetch {
        let rows = app.rows();
        let order = (app.scroll..rows.len()).chain(0..app.scroll);
        for index in order {
            if let Some(terminal) = rows[index].key.strip_prefix("pane:") {
                if let Some(pane) = app.snapshot.pane_by_terminal(terminal) {
                    push(pane);
                }
            }
        }
    }
    out
}

/// Run a window until it closes, moves something, or switches.
pub fn run(herdr: Arc<dyn Herdr>, options: Options) -> io::Result<Exit> {
    // Subscribe before the first snapshot so no change is missed.
    let events = herdr.subscribe(EVENT_TYPES);
    let mut app = match api::snapshot(&*herdr) {
        Ok(snapshot) => {
            let source = options
                .source_id
                .as_deref()
                .and_then(|id| Exec::new(&*herdr).resolve(&snapshot, id));
            let mut app = App::new(options.mode, snapshot, source);
            if app.source.is_none() {
                app.show_error(plan::PANE_CLOSED);
            }
            app
        }
        Err(error) => {
            // Edge case 6.5: say so and wait for ⎋.
            let mut app = App::new(options.mode, Default::default(), None);
            app.show_error(error.to_string());
            app
        }
    };

    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    // Edge case 6.18: mouse support, but only when herdr captures the
    // mouse too, so a keyboard-only herdr keeps its fast Esc.
    if crate::state::herdr_mouse_capture() {
        execute!(io::stdout(), EnableMouseCapture)?;
    }
    let _guard = TerminalGuard;
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        default_hook(info);
    }));
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;

    let mut requested: HashSet<String> = HashSet::new();
    let mut command_feeds: Vec<Receiver<(String, Command)>> = Vec::new();
    let mut refresh_at: Option<Instant> = None;
    let mut dirty = true;
    let mut started_commands = false;

    loop {
        if dirty {
            terminal.draw(|frame| view::render(frame, &mut app))?;
            dirty = false;
            if !started_commands {
                // First draw is on screen; now fill in commands.
                started_commands = true;
                let panes = wanted(&app, &requested);
                requested.extend(panes.iter().map(|(_, t)| t.clone()));
                command_feeds.push(fetch_commands(herdr.clone(), panes));
            }
        }
        if event::poll(Duration::from_millis(25))? {
            let input = match event::read()? {
                Event::Key(key) => map_key(key),
                Event::Mouse(mouse) => app.mouse(mouse),
                Event::Resize(_, _) => {
                    dirty = true;
                    continue;
                }
                _ => continue,
            };
            dirty = true;
            match app.handle(input) {
                Effect::None => {}
                Effect::Close => return Ok(Exit::Closed),
                Effect::Switch(mode) => {
                    if options.switch_in_place {
                        app.switch(mode);
                        started_commands = false;
                    } else {
                        let source = app
                            .source
                            .as_ref()
                            .map(|p| p.pane_id.clone())
                            .unwrap_or_default();
                        crate::actions::spawn_detached(&["reopen", mode.name(), &source]);
                        return Ok(Exit::Switched(mode));
                    }
                }
                Effect::Execute(action) => {
                    terminal.draw(|frame| view::render(frame, &mut app))?;
                    if let Some(exit) = execute(&*herdr, &mut app, &options, action) {
                        return Ok(exit);
                    }
                    refresh_at = Some(Instant::now());
                }
            }
        }
        for feed in &command_feeds {
            while let Ok((terminal_id, command)) = feed.try_recv() {
                app.commands.insert(terminal_id, command);
                dirty = true;
            }
        }
        if let Some(events) = &events {
            while events.try_recv().is_ok() {
                refresh_at.get_or_insert_with(|| Instant::now() + Duration::from_millis(80));
            }
        }
        if refresh_at.is_some_and(|at| Instant::now() >= at) {
            refresh_at = None;
            if let Ok(snapshot) = api::snapshot(&*herdr) {
                app.set_snapshot(snapshot);
                let panes = wanted(&app, &requested);
                if !panes.is_empty() {
                    requested.extend(panes.iter().map(|(_, t)| t.clone()));
                    command_feeds.push(fetch_commands(herdr.clone(), panes));
                }
                dirty = true;
            }
        }
    }
}

/// Carry out the chosen row. Returns an exit when the window should close.
fn execute(herdr: &dyn Herdr, app: &mut App, options: &Options, action: RowAction) -> Option<Exit> {
    let Some(source) = app.source.clone() else {
        app.show_error(plan::PANE_CLOSED);
        return None;
    };
    let _lock = QueueLock::acquire(&options.state_dir).ok();
    let exec = Exec::new(herdr);
    let result = match action {
        RowAction::Send(target) => exec.send(&source.pane_id, &target, app.split, "send"),
        RowAction::FetchPane(pane_id) => exec.fetch(
            &source.pane_id,
            &FetchTarget::Pane(pane_id),
            app.split,
            "fetch",
        ),
        RowAction::FetchTab(tab_id) => exec.fetch(
            &source.pane_id,
            &FetchTarget::Tab(tab_id),
            app.split,
            "fetch-tab",
        ),
        RowAction::OpenSpace(_) | RowAction::Back | RowAction::None => return None,
    };
    let mut journal = Journal::load(&options.state_dir, &herdr.session_key());
    match result {
        Ok(Outcome::Done(done)) => {
            append_log(&options.state_dir, &done.log);
            if let Some(entry) = done.entry {
                journal.push(*entry);
                let _ = journal.save();
            }
            Some(Exit::Moved {
                summary: done.summary,
                warnings: done.warnings,
            })
        }
        Ok(Outcome::NoOp(no_op)) => {
            app.show_info(no_op.0);
            None
        }
        Ok(Outcome::Notice(text)) => {
            app.show_info(text);
            None
        }
        Err(failure) => {
            append_log(
                &options.state_dir,
                &format!("{} error={}", app.mode.name(), failure.message),
            );
            if let Some(entry) = failure.entry {
                journal.push(*entry);
                let _ = journal.save();
            }
            // Edge case 7.5: inline, and the window stays open.
            app.show_error(failure.message);
            None
        }
    }
}
