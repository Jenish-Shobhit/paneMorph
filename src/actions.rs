//! The commands herdr runs: plugin actions, popup entrypoints and helpers.

use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::api::{self, Herdr, HerdrError, SocketClient};
use crate::exec::{Exec, ExecResult, Outcome};
use crate::journal::Journal;
use crate::plan::{self, SendTarget, Step};
use crate::sim::{Sim, SAMPLE_FIXTURE};
use crate::state::{append_log, Context, QueueLock};
use crate::ui::app::Mode;
use crate::ui::run::{self, Exit, Options};

/// The oldest herdr paneMorph runs on. 0.9.1 is recommended (see README).
pub const MIN_HERDR: &str = "0.9.0";
pub const RECOMMENDED_HERDR: &str = "0.9.1";

pub const USAGE: &str = "usage: panemorph <command>

Plugin actions (bound in herdr's config):
  send               open the Send window for the focused pane
  fetch              open the Fetch window
  move-tab-new       move the focused pane to a new tab next to this one
  move-space-new     move the focused pane to a new space
  move-tab-prev      move the focused pane one tab left
  move-tab-next      move the focused pane one tab right
  undo               put the last moved pane back
  extract-pane, send-pane, bring-pane
                     deprecated aliases for move-tab-new, send, fetch

Other commands:
  doctor             check the herdr connection and versions
  preview send|fetch [--fixture FILE] [--result FILE]
                     open a window over a simulated session
  version            print the version";

fn version_tuple(text: &str) -> Vec<u32> {
    text.trim_start_matches('v')
        .split(|c: char| !c.is_ascii_digit())
        .take(3)
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

/// Is `running` at least `min`? Unknown versions pass.
pub fn version_at_least(running: &str, min: &str) -> bool {
    if running.trim().is_empty() {
        return true;
    }
    version_tuple(running) >= version_tuple(min)
}

/// Edge case 6.7: refuse to act on a server older than paneMorph needs.
fn check_version(herdr: &dyn Herdr) -> Result<(), String> {
    let pong = herdr
        .request("ping", json!({}))
        .map_err(|e| e.to_string())?;
    let running = pong
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if version_at_least(running, MIN_HERDR) {
        Ok(())
    } else {
        Err(format!(
            "paneMorph requires Herdr {MIN_HERDR} or newer; current Herdr is {running}"
        ))
    }
}

/// Start `panemorph <args>` in its own session so it outlives the popup
/// that spawned it.
pub fn spawn_detached_child(args: &[&str]) -> std::io::Result<std::process::Child> {
    let exe = std::env::current_exe()?;
    let mut command = std::process::Command::new(exe);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: setsid is async-signal-safe and touches no Rust state.
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    command.spawn()
}

pub fn spawn_detached(args: &[&str]) {
    let _ = spawn_detached_child(args);
}

fn popup_size(entrypoint: &str) -> (Value, Value) {
    match entrypoint {
        "send" => (json!("60%"), json!("50%")),
        "fetch" => (json!("64%"), json!("60%")),
        _ => (json!(60), json!(7)),
    }
}

fn open_popup(
    herdr: &dyn Herdr,
    plugin_id: &str,
    entrypoint: &str,
    env: Value,
) -> Result<(), HerdrError> {
    let (width, height) = popup_size(entrypoint);
    herdr
        .request(
            "plugin.pane.open",
            json!({
                "plugin_id": plugin_id,
                "entrypoint": entrypoint,
                "placement": "popup",
                "width": width,
                "height": height,
                "env": env,
                "focus": true,
            }),
        )
        .map(|_| ())
}

/// Tell the user about a real failure: a toast, or a notice popup when
/// toasts are off (edge cases 7.6, 7.7). Retries a busy popup for `wait`.
pub fn notify_failure(herdr: &dyn Herdr, ctx: &Context, message: &str, wait: Duration) {
    // herdr's default is `delivery = "off"`; 0.9.0 still answers
    // `shown: true` then, so only trust the toast when toasts are on.
    if crate::state::herdr_toasts_on() {
        if let Ok((true, _)) = api::notify(herdr, "paneMorph", message) {
            return;
        }
    }
    let deadline = Instant::now() + wait;
    loop {
        match open_popup(
            herdr,
            &ctx.plugin_id,
            "notice",
            json!({"PANEMORPH_NOTICE": message}),
        ) {
            Ok(()) => return,
            Err(error) if error.is_ui_busy() && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(40));
            }
            Err(error) => {
                append_log(
                    &ctx.state_dir,
                    &format!("notice not shown ({error}): {message}"),
                );
                return;
            }
        }
    }
}

/// Edge press: try the toast only (edge case 7.6).
fn toast_only(herdr: &dyn Herdr, message: &str) {
    let _ = api::notify(herdr, "paneMorph", message);
}

fn say(ctx: &Context, line: &str) {
    println!("{line}");
    let _ = std::io::stdout().flush();
    append_log(&ctx.state_dir, line);
}

fn deprecated(old: &str, new: &str) {
    eprintln!(
        "paneMorph: `{old}` is deprecated; bind dev.panemorph.{new} instead (the alias goes away in v1.0)"
    );
}

fn connect() -> Result<(Context, SocketClient), (Context, String)> {
    let ctx = Context::from_env();
    match SocketClient::from_env() {
        Ok(client) => Ok((ctx, client)),
        Err(error) => Err((ctx, error.to_string())),
    }
}

/// `send` and `fetch`: open the window as a herdr popup (edge case 6.1).
fn open_window(mode: Mode) -> i32 {
    let (ctx, client) = match connect() {
        Ok(pair) => pair,
        Err((_, error)) => {
            eprintln!("paneMorph: {error}");
            return 1;
        }
    };
    if let Err(error) = check_version(&client) {
        notify_failure(&client, &ctx, &error, Duration::ZERO);
        eprintln!("paneMorph: {error}");
        return 1;
    }
    let Some(source) = ctx.focused_pane_id.clone() else {
        eprintln!("paneMorph: no focused pane in the invocation context");
        return 1;
    };
    match open_popup(
        &client,
        &ctx.plugin_id,
        mode.name(),
        json!({"PANEMORPH_SOURCE_PANE_ID": source}),
    ) {
        Ok(()) => {
            say(&ctx, &format!("{} window opened for {source}", mode.name()));
            0
        }
        Err(error) if error.is_ui_busy() => {
            // Edge cases 6.3 and 6.4: another popup or modal is open.
            say(
                &ctx,
                &format!("{}: window already open ({error})", mode.name()),
            );
            0
        }
        Err(error) => {
            let text = if error.to_string().contains("too small") {
                "Terminal too small for the paneMorph window".to_string()
            } else {
                format!("Couldn't open the {} window: {error}", mode.name())
            };
            notify_failure(&client, &ctx, &text, Duration::ZERO);
            eprintln!("paneMorph: {text}");
            1
        }
    }
}

/// Report a move's outcome for a key with no window (7.6, 7.8, 7.10).
fn finish(herdr: &dyn Herdr, ctx: &Context, action: &str, result: ExecResult) -> i32 {
    let mut journal = Journal::load(&ctx.state_dir, &herdr.session_key());
    match result {
        Ok(Outcome::Done(done)) => {
            say(ctx, &done.log);
            if let Some(entry) = done.entry {
                journal.push(*entry);
            }
            let _ = journal.save();
            for warning in &done.warnings {
                say(ctx, &format!("{action} warning: {warning}"));
                notify_failure(herdr, ctx, warning, Duration::ZERO);
            }
            0
        }
        Ok(Outcome::NoOp(no_op)) => {
            let _ = journal.save();
            say(ctx, &format!("{action} no-op: {}", no_op.0));
            toast_only(herdr, &no_op.0);
            0
        }
        Ok(Outcome::Notice(text)) => {
            let _ = journal.save();
            say(ctx, &format!("{action}: {text}"));
            notify_failure(herdr, ctx, &text, Duration::ZERO);
            0
        }
        Err(failure) => {
            if let Some(entry) = failure.entry {
                journal.push(*entry);
                let _ = journal.save();
            }
            eprintln!("paneMorph: {action} failed: {}", failure.message);
            append_log(
                &ctx.state_dir,
                &format!("{action} error={}", failure.message),
            );
            notify_failure(herdr, ctx, &failure.message, Duration::ZERO);
            1
        }
    }
}

enum Quick {
    TabNew,
    SpaceNew,
    Tab(Step),
    Undo,
}

fn quick(kind: Quick, action: &str) -> i32 {
    let (ctx, client) = match connect() {
        Ok(pair) => pair,
        Err((_, error)) => {
            eprintln!("paneMorph: {error}");
            return 1;
        }
    };
    if let Err(error) = check_version(&client) {
        notify_failure(&client, &ctx, &error, Duration::ZERO);
        eprintln!("paneMorph: {error}");
        return 1;
    }
    // Edge case 4.8: presses queue; each acts on the state the last left.
    let _lock = match QueueLock::acquire(&ctx.state_dir) {
        Ok(lock) => Some(lock),
        Err(error) => {
            append_log(&ctx.state_dir, &format!("queue lock unavailable: {error}"));
            None
        }
    };
    let exec = Exec::new(&client);
    let source = ctx.focused_pane_id.clone();
    let result = match (&kind, source.as_deref()) {
        (Quick::Undo, focused) => {
            let mut journal = Journal::load(&ctx.state_dir, &client.session_key());
            let result = exec.undo(&mut journal, focused);
            let _ = journal.save();
            // `finish` reloads the journal; undo already saved it.
            return finish_undo(&client, &ctx, action, result);
        }
        (_, None) => Err(crate::exec::Failure::new(
            "paneMorph was not invoked from a herdr pane",
        )),
        (Quick::TabNew, Some(id)) => {
            exec.send(id, &SendTarget::NewTabHere, Default::default(), action)
        }
        (Quick::SpaceNew, Some(id)) => {
            exec.send(id, &SendTarget::NewSpace, Default::default(), action)
        }
        (Quick::Tab(step), Some(id)) => match api::snapshot(&client) {
            Err(error) => Err(error.into()),
            Ok(snapshot) => match exec.resolve(&snapshot, id) {
                None => Err(crate::exec::Failure::new(plan::PANE_CLOSED)),
                Some(pane) => match plan::neighbour_tab(&snapshot, &pane, *step) {
                    Err(no_op) => Ok(Outcome::NoOp(no_op)),
                    Ok(tab) => exec.send(
                        &pane.pane_id,
                        &SendTarget::Tab(tab),
                        Default::default(),
                        action,
                    ),
                },
            },
        },
    };
    finish(&client, &ctx, action, result)
}

fn finish_undo(herdr: &dyn Herdr, ctx: &Context, action: &str, result: ExecResult) -> i32 {
    match result {
        Ok(Outcome::Done(done)) => {
            say(ctx, &done.log);
            for warning in &done.warnings {
                notify_failure(herdr, ctx, warning, Duration::ZERO);
            }
            0
        }
        Ok(Outcome::NoOp(no_op)) => {
            say(ctx, &format!("{action} no-op: {}", no_op.0));
            toast_only(herdr, &no_op.0);
            0
        }
        Ok(Outcome::Notice(text)) => {
            say(ctx, &format!("{action}: {text}"));
            notify_failure(herdr, ctx, &text, Duration::ZERO);
            0
        }
        Err(failure) => {
            eprintln!("paneMorph: {action} failed: {}", failure.message);
            append_log(
                &ctx.state_dir,
                &format!("{action} error={}", failure.message),
            );
            notify_failure(herdr, ctx, &failure.message, Duration::ZERO);
            1
        }
    }
}

/// The popup entrypoint: `panemorph window send|fetch`.
fn window(mode: Mode) -> i32 {
    let ctx = Context::from_env();
    let client = match SocketClient::from_env() {
        Ok(client) => Arc::new(client) as Arc<dyn Herdr>,
        Err(error) => {
            eprintln!("paneMorph: {error}");
            return 1;
        }
    };
    let options = Options {
        mode,
        source_id: ctx.focused_pane_id.clone(),
        state_dir: ctx.state_dir.clone(),
        in_process: false,
    };
    match run::run(client, options) {
        Ok(Exit::Moved { summary, .. }) => {
            // The worker already logged the move and reports late problems
            // itself once this popup is gone (edge case 6.20).
            append_log(&ctx.state_dir, &format!("{}: {summary}", mode.name()));
            0
        }
        Ok(Exit::Closed) | Ok(Exit::Switched(_)) => 0,
        Err(error) => {
            append_log(
                &ctx.state_dir,
                &format!("{} window error: {error}", mode.name()),
            );
            1
        }
    }
}

/// The detached worker behind a window's ⏎ (see `ui::apply`).
fn apply_worker(json: &str) -> i32 {
    let Ok(args) = serde_json::from_str::<crate::ui::apply::WorkerArgs>(json) else {
        return 2;
    };
    let (ctx, client) = match connect() {
        Ok(pair) => pair,
        Err((ctx, error)) => {
            let reply = crate::ui::apply::Reply::Error(error);
            let _ = crate::ui::apply::write_reply(&args.result, &reply);
            append_log(&ctx.state_dir, "apply: herdr unreachable");
            return 1;
        }
    };
    let (reply, warnings) = crate::ui::apply::apply(&client, &ctx.state_dir, &args.request);
    let failed = matches!(reply, crate::ui::apply::Reply::Error(_));
    let _ = crate::ui::apply::write_reply(&args.result, &reply);
    // The window closes after reading the reply; then these can show.
    for warning in warnings {
        notify_failure(&client, &ctx, &warning, Duration::from_secs(3));
    }
    // If the popup was torn down before reading its reply, tidy up.
    std::thread::sleep(Duration::from_secs(2));
    let _ = std::fs::remove_file(&args.result);
    i32::from(failed)
}

/// Reopen the other window once this popup has closed (⌃⌥S / ⌃⌥F).
fn reopen(mode: Mode, source: &str) -> i32 {
    let (ctx, client) = match connect() {
        Ok(pair) => pair,
        Err(_) => return 1,
    };
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match open_popup(
            &client,
            &ctx.plugin_id,
            mode.name(),
            json!({"PANEMORPH_SOURCE_PANE_ID": source}),
        ) {
            Ok(()) => return 0,
            Err(error) if error.is_ui_busy() && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => {
                append_log(
                    &ctx.state_dir,
                    &format!("reopen {} failed: {error}", mode.name()),
                );
                return 1;
            }
        }
    }
}

fn doctor() -> i32 {
    println!("paneMorph {}", crate::VERSION);
    let ctx = Context::from_env();
    println!("state dir: {}", ctx.state_dir.display());
    let client = match SocketClient::from_env() {
        Ok(client) => client,
        Err(error) => {
            println!("paneMorph doctor: failed: {error}");
            return 1;
        }
    };
    println!("socket: {}", client.path().display());
    let snapshot = match api::snapshot(&client) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            println!("paneMorph doctor: failed: {error}");
            return 1;
        }
    };
    println!(
        "herdr: {} (protocol {})",
        snapshot.version, snapshot.protocol
    );
    if !version_at_least(&snapshot.version, MIN_HERDR) {
        println!("paneMorph doctor: failed: needs Herdr {MIN_HERDR} or newer");
        return 1;
    }
    if !version_at_least(&snapshot.version, RECOMMENDED_HERDR) {
        println!(
            "note: Herdr {RECOMMENDED_HERDR} is recommended. On {} paneMorph moves your view with an extra pane.focus after each move (herdr issue #4153).",
            snapshot.version
        );
    }
    println!(
        "spaces: {}, tabs: {}, panes: {}",
        snapshot.workspaces.len(),
        snapshot.tabs.len(),
        snapshot.panes.len()
    );
    let journal = Journal::load(&ctx.state_dir, &client.session_key());
    println!("undo journal: {} moves", journal.entries.len());
    println!("paneMorph doctor: ok");
    0
}

/// `panemorph preview send|fetch`: a window over the simulated session.
fn preview(args: &[String]) -> i32 {
    let Some(mode) = args.first().and_then(|m| Mode::parse(m)) else {
        eprintln!("{USAGE}");
        return 2;
    };
    let mut fixture = SAMPLE_FIXTURE.to_string();
    let mut result_path: Option<PathBuf> = None;
    let mut iter = args[1..].iter();
    while let Some(flag) = iter.next() {
        match (flag.as_str(), iter.next()) {
            ("--fixture", Some(path)) => match std::fs::read_to_string(path) {
                Ok(text) => fixture = text,
                Err(error) => {
                    eprintln!("paneMorph: {path}: {error}");
                    return 2;
                }
            },
            ("--result", Some(path)) => result_path = Some(PathBuf::from(path)),
            _ => {
                eprintln!("{USAGE}");
                return 2;
            }
        }
    }
    let sim = match Sim::from_json(&fixture) {
        Ok(sim) => Arc::new(sim),
        Err(error) => {
            eprintln!("paneMorph: bad fixture: {error}");
            return 2;
        }
    };
    let source = api::snapshot(&*sim).ok().and_then(|s| s.focused_pane_id);
    let state_dir = std::env::temp_dir().join(format!("panemorph-preview-{}", std::process::id()));
    let options = Options {
        mode,
        source_id: source,
        state_dir: state_dir.clone(),
        in_process: true,
    };
    let exit = run::run(sim.clone() as Arc<dyn Herdr>, options);
    let _ = std::fs::remove_dir_all(&state_dir);
    let exit = match exit {
        Ok(exit) => exit,
        Err(error) => {
            eprintln!("paneMorph: {error}");
            return 1;
        }
    };
    if let Some(path) = result_path {
        let (kind, summary) = match &exit {
            Exit::Closed => ("closed", String::new()),
            Exit::Moved { summary, .. } => ("moved", summary.clone()),
            Exit::Switched(mode) => ("switched", mode.name().to_string()),
        };
        let snapshot = sim
            .request("session.snapshot", json!({}))
            .unwrap_or(Value::Null);
        let report = json!({"exit": kind, "summary": summary, "calls": sim.all_calls(), "snapshot": snapshot["snapshot"]});
        let _ = std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap_or_default());
    }
    0
}

/// Entry point for `main`.
pub fn main_with(args: &[String]) -> i32 {
    let Some(command) = args.first() else {
        eprintln!("{USAGE}");
        return 2;
    };
    match command.as_str() {
        "send" => open_window(Mode::Send),
        "fetch" => open_window(Mode::Fetch),
        "send-pane" => {
            deprecated("send-pane", "send");
            open_window(Mode::Send)
        }
        "bring-pane" => {
            deprecated("bring-pane", "fetch");
            open_window(Mode::Fetch)
        }
        "move-tab-new" => quick(Quick::TabNew, "move-tab-new"),
        "extract-pane" => {
            deprecated("extract-pane", "move-tab-new");
            quick(Quick::TabNew, "move-tab-new")
        }
        "move-space-new" => quick(Quick::SpaceNew, "move-space-new"),
        "move-tab-prev" => quick(Quick::Tab(Step::Prev), "move-tab-prev"),
        "move-tab-next" => quick(Quick::Tab(Step::Next), "move-tab-next"),
        "undo" => quick(Quick::Undo, "undo"),
        "window" => match args.get(1).and_then(|m| Mode::parse(m)) {
            Some(mode) => window(mode),
            None => 2,
        },
        "notice" => crate::ui::notice::run(),
        "reopen" => match (args.get(1).and_then(|m| Mode::parse(m)), args.get(2)) {
            (Some(mode), Some(source)) => reopen(mode, source),
            _ => 2,
        },
        "apply" => args.get(1).map(|json| apply_worker(json)).unwrap_or(2),
        "doctor" => doctor(),
        "preview" => preview(&args[1..]),
        "version" | "--version" | "-V" => {
            println!("panemorph {}", crate::VERSION);
            0
        }
        "help" | "--help" | "-h" => {
            println!("{USAGE}");
            0
        }
        other => {
            eprintln!("paneMorph: unknown command `{other}`\n{USAGE}");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Edge case 6.7: version gate; the manifest keeps 0.9.0.
    #[test]
    fn edge_6_7_version_gate() {
        assert!(version_at_least("0.9.0", MIN_HERDR));
        assert!(version_at_least("0.9.1", MIN_HERDR));
        assert!(version_at_least("0.10.0", MIN_HERDR));
        assert!(!version_at_least("0.8.9", MIN_HERDR));
        assert!(!version_at_least("0.9.0", RECOMMENDED_HERDR));
        assert!(version_at_least("", MIN_HERDR));
    }

    #[test]
    fn manifest_keeps_min_version_and_every_action() {
        let manifest = include_str!("../herdr-plugin.toml");
        assert!(manifest.contains(&format!("min_herdr_version = \"{MIN_HERDR}\"")));
        for id in [
            "send",
            "fetch",
            "move-tab-new",
            "move-space-new",
            "move-tab-prev",
            "move-tab-next",
            "undo",
            // Edge case 4.15: the old ids stay as aliases in v0.2.
            "extract-pane",
            "send-pane",
            "bring-pane",
        ] {
            assert!(manifest.contains(&format!("id = \"{id}\"")), "{id}");
        }
        // Popup commands must start with ./ so portable-pty resolves them
        // against the plugin root (herdr vendor/portable-pty cmdbuilder.rs).
        assert!(!manifest.contains("command = [\"bin/"));
        assert!(manifest.contains("placement = \"popup\""));
    }

    #[test]
    fn popup_sizes_follow_the_spec() {
        assert_eq!(popup_size("send"), (json!("60%"), json!("50%")));
        assert_eq!(popup_size("fetch"), (json!("64%"), json!("60%")));
    }
}
