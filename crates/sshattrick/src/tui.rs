use crate::ui;
use crossterm::cursor::Hide;
use crossterm::event::EnableMouseCapture;
use crossterm::terminal::{Clear, ClearType, EnterAlternateScreen, SetTitle};
use crate::ai::Skill;
use crate::out::Out;
use frittura_ssh_core::TerminalEvent;
use ratatui::layout::Rect;
use ratatui::prelude::CrosstermBackend;
use ratatui::text::Line;
use ratatui::{Terminal, TerminalOptions, Viewport};
use sshattrick_core::constants::UI_SCREEN_SIZE;
use sshattrick_core::{AppResult, Game, GameSide};
use tokio::sync::mpsc::Receiver;

/// Asciiverse: where a local player starts, skipping the lobby.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    Computer,
    Practice,
    Watch,
}

#[derive(Debug)]
pub struct Tui {
    /// Asciiverse: a mode to go straight into on arrival
    pub start: Option<Start>,
    /// Asciiverse: after a watched game, watch the next (`--watch`)
    pub repeat_watch: bool,
    /// Asciiverse: the computer's level when this player picks it
    pub skill: Skill,
    /// Asciiverse: how friends on the network join this game, shown in
    /// the lobby of the person playing at this computer
    pub lan_hint: Option<String>,
    username: String,
    games_played: usize,
    games_won: usize,
    terminal: Terminal<CrosstermBackend<Out>>,
    events: Receiver<TerminalEvent>,
}

impl Tui {
    pub fn new(
        username: String,
        writer: Out,
        events: Receiver<TerminalEvent>,
    ) -> AppResult<Self> {
        let backend = CrosstermBackend::new(writer);
        let opts = TerminalOptions {
            viewport: Viewport::Fixed(Rect {
                x: 0,
                y: 0,
                width: UI_SCREEN_SIZE.0,
                height: UI_SCREEN_SIZE.1,
            }),
        };
        let terminal = Terminal::with_options(backend, opts)?;
        let mut tui = Self {
            start: None,
            repeat_watch: false,
            skill: Skill::Normal,
            lan_hint: None,
            username,
            games_played: 0,
            games_won: 0,
            terminal,
            events,
        };
        tui.init()?;
        Ok(tui)
    }

    fn init(&mut self) -> AppResult<()> {
        crossterm::execute!(
            self.terminal.backend_mut(),
            EnterAlternateScreen,
            EnableMouseCapture,
            SetTitle("ssHattrick"),
            Clear(ClearType::All),
            Hide
        )?;
        Ok(())
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn record_game(&mut self, won: bool) {
        self.games_played += 1;
        if won {
            self.games_won += 1;
        }
    }

    /// Returns the next terminal event. If the channel is closed (client
    /// disconnected) we surface `Quit` so the game loop can wind down.
    pub async fn next(&mut self) -> TerminalEvent {
        self.events.recv().await.unwrap_or(TerminalEvent::Quit)
    }

    /// Non-blocking peek at the next event. Used by the matchmaker which
    /// services many pending Tuis from a single tick loop. Returns `None`
    /// if no event is queued; `Some(Quit)` if the channel was closed.
    pub fn try_next(&mut self) -> Option<TerminalEvent> {
        match self.events.try_recv() {
            Ok(event) => Some(event),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => None,
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => Some(TerminalEvent::Quit),
        }
    }

    pub fn draw(
        &mut self,
        game: &Game,
        image_lines: &[Line],
        viewer: GameSide,
        idle_warning: Option<u32>,
    ) -> AppResult<()> {
        self.terminal
            .draw(|frame| ui::render(frame, game, image_lines, viewer, idle_warning))?;
        Ok(())
    }

    pub fn draw_lobby(
        &mut self,
        stats: &crate::lobby::LobbyStats,
        view: crate::lobby::LobbyView,
        kick_warning_secs: Option<u32>,
    ) -> AppResult<()> {
        let Self {
            username,
            games_played,
            games_won,
            terminal,
            skill,
            lan_hint,
            ..
        } = self;
        let extra = ui::LobbyExtra { skill: skill.name(), lan_hint: lan_hint.as_deref() };
        terminal.draw(|frame| {
            ui::render_lobby(
                frame,
                username,
                *games_played,
                *games_won,
                stats,
                view,
                kick_warning_secs,
                &extra,
            )
        })?;
        Ok(())
    }

    pub async fn push_data(&mut self) -> AppResult<()> {
        self.terminal.backend_mut().writer_mut().send().await?;
        Ok(())
    }

    /// Restore the terminal and close the SSH channel, awaited end-to-end.
    pub async fn close(mut self) {
        self.terminal
            .backend_mut()
            .writer_mut()
            .send_and_close()
            .await;
    }
}
