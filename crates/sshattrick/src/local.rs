//! Playing at this computer, an Asciiverse addition: the person in this
//! terminal joins the same lobby as anyone connecting over ssh, without
//! connecting to anything. Their screen is this terminal (out::LocalOut)
//! and their keys come from it, read on a thread of their own.

use crate::ai::Skill;
use crate::out::{LocalOut, Out};
use crate::tui::{Start, Tui};
use crate::SshattrickGame;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::style::Print;
use crossterm::terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{cursor, execute, queue};
use frittura_ssh_core::TerminalEvent;
use sshattrick_core::constants::UI_SCREEN_SIZE;
use sshattrick_core::AppResult;
use std::io::Write;
use std::sync::Arc;
use tokio::sync::{mpsc, Notify};

pub struct LocalOptions {
    pub username: String,
    pub start: Option<Start>,
    pub repeat_watch: bool,
    pub skill: Skill,
    pub lan_hint: Option<String>,
}

/// Put the terminal back the way it was.
pub fn restore_terminal() {
    let _ = execute!(
        std::io::stdout(),
        crossterm::event::DisableMouseCapture,
        LeaveAlternateScreen,
        cursor::Show
    );
    let _ = terminal::disable_raw_mode();
}

/// Wait until the terminal is big enough for the rink (160x50), showing
/// how big it is now. Returns false if the player leaves instead.
fn wait_for_room() -> AppResult<bool> {
    let (w, h) = UI_SCREEN_SIZE;
    loop {
        let (cols, rows) = terminal::size()?;
        if cols >= w && rows >= h {
            return Ok(true);
        }
        let lines = [
            "ssHattrick needs a bigger window".to_string(),
            String::new(),
            format!("at least {w}x{h}; this one is {cols}x{rows}"),
            String::new(),
            "make the window bigger, or the font smaller".to_string(),
            "Esc or q to leave".to_string(),
        ];
        let mut out = std::io::stdout();
        queue!(out, Clear(ClearType::All))?;
        let top = rows.saturating_sub(lines.len() as u16) / 2;
        for (i, l) in lines.iter().enumerate() {
            let x = cols.saturating_sub(l.chars().count() as u16) / 2;
            queue!(out, cursor::MoveTo(x, top + i as u16), Print(l))?;
        }
        out.flush()?;
        match event::read()? {
            Event::Key(k) if k.kind != KeyEventKind::Release => {
                log::debug!("key {:?} {:?} (waiting for room)", k.code, k.modifiers);
                let ctrl_c = k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c');
                if ctrl_c || matches!(k.code, KeyCode::Esc | KeyCode::Char('q')) {
                    return Ok(false);
                }
            }
            Event::Resize(c, r) => log::info!("resize {c}x{r} (waiting for room)"),
            _ => {}
        }
    }
}

/// Keys and window sizes from this terminal, as the lobby and the game
/// expect them from an ssh session. Ctrl+C is leaving.
fn spawn_input(tx: mpsc::Sender<TerminalEvent>) {
    std::thread::spawn(move || loop {
        let ev = match event::read() {
            Ok(ev) => ev,
            Err(e) => {
                log::warn!("reading the terminal: {e}");
                let _ = tx.blocking_send(TerminalEvent::Quit);
                return;
            }
        };
        let out = match ev {
            Event::Key(k) => {
                log::debug!("key {:?} {:?} {:?}", k.code, k.kind, k.modifiers);
                if k.kind == KeyEventKind::Release {
                    continue;
                }
                if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
                    TerminalEvent::Quit
                } else {
                    TerminalEvent::Key(k)
                }
            }
            Event::Resize(c, r) => {
                log::info!("resize {c}x{r}");
                TerminalEvent::Resize(c, r)
            }
            _ => continue,
        };
        let quit = matches!(out, TerminalEvent::Quit);
        if tx.blocking_send(out).is_err() || quit {
            return;
        }
    });
}

/// Play in this terminal until the player leaves the lobby.
pub async fn play_here(game: &Arc<SshattrickGame>, opts: LocalOptions) -> AppResult<()> {
    terminal::enable_raw_mode()?;
    execute!(std::io::stdout(), EnterAlternateScreen, cursor::Hide)?;
    if !wait_for_room()? {
        return Ok(());
    }
    let left = Arc::new(Notify::new());
    let (tx, rx) = mpsc::channel(64);
    spawn_input(tx);
    let mut tui = Tui::new(opts.username, Out::Local(LocalOut::new(left.clone())), rx)?;
    tui.start = opts.start;
    tui.repeat_watch = opts.repeat_watch;
    tui.skill = opts.skill;
    tui.lan_hint = opts.lan_hint;
    log::info!("playing here as {}, start {:?}, computer {}", tui.username(), tui.start, tui.skill.name());
    if game.tui_sender.send(tui).await.is_err() {
        anyhow::bail!("the lobby is gone");
    }
    left.notified().await;
    log::info!("left the lobby");
    Ok(())
}
