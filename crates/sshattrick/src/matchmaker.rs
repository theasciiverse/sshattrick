use crate::ai::Ai;
use crate::lobby::{
    generate_invite_code, FriendCodeState, LobbyStats, PendingPlayer, PlayerMode, FRIEND_CODE_LEN,
};
use crate::tui::{Start, Tui};
use crossterm::event::KeyCode;
use frittura_ssh_core::TerminalEvent;
use sshattrick_core::{AppResult, Game, GameCommand, GameSide, GameState};

fn key_code_to_game_command(code: KeyCode) -> Option<GameCommand> {
    match code {
        KeyCode::Up => Some(GameCommand::Up),
        KeyCode::Down => Some(GameCommand::Down),
        KeyCode::Left => Some(GameCommand::Left),
        KeyCode::Right => Some(GameCommand::Right),
        KeyCode::Char(' ') => Some(GameCommand::Shoot),
        _ => None,
    }
}
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{self, Sender};
use tokio::task;
use tokio::time::MissedTickBehavior;
use tokio::{select, time};

const UPDATE_TIME_STEP: Duration = Duration::from_millis(1000 / 100);
const DRAW_TIME_STEP: Duration = Duration::from_millis(1000 / 30);
const AFTER_GAME_DELAY: Duration = Duration::from_millis(2000);

pub fn spawn(tui_receiver: mpsc::Receiver<Tui>, lobby_sender: Sender<Tui>) {
    let ongoing_games = Arc::new(AtomicUsize::new(0));
    task::spawn(matchmaker(tui_receiver, lobby_sender, ongoing_games));
}

async fn matchmaker(
    mut tui_receiver: mpsc::Receiver<Tui>,
    lobby_sender: Sender<Tui>,
    ongoing_games: Arc<AtomicUsize>,
) {
    let mut players: Vec<PendingPlayer> = vec![];
    let mut update_ticker = time::interval(UPDATE_TIME_STEP);
    let mut draw_ticker = time::interval(DRAW_TIME_STEP);
    draw_ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut last_stats = LobbyStats::default();

    loop {
        select! {
            new_tui = tui_receiver.recv() => match new_tui {
                Some(mut tui) => {
                    log::info!("Player joined: {}", tui.username());
                    // Asciiverse: a local player can come in straight to a mode
                    let start = tui.start.take().or(if tui.repeat_watch { Some(Start::Watch) } else { None });
                    let mut player = PendingPlayer::new(tui);
                    match start {
                        Some(Start::Computer) => player.mode = PlayerMode::Computer,
                        Some(Start::Watch) => player.mode = PlayerMode::Watching,
                        Some(Start::Practice) => player.mode = PlayerMode::Practicing(Box::new(Game::new_practice())),
                        None => {}
                    }
                    players.push(player);
                }
                None => return,
            },
            _ = update_ticker.tick() => {
                if !players.is_empty() {
                    process_lobby_events(&mut players).await;
                    for player in players.iter_mut() {
                        if let PlayerMode::Practicing(g) = &mut player.mode {
                            if let Err(e) = g.update() {
                                log::error!("Practice update error: {e}");
                                player.mode = PlayerMode::Idle;
                                player.dirty = true;
                            }
                        }
                    }
                    pair_friend_codes(&mut players, &lobby_sender, &ongoing_games);
                    pair_auto_queue(&mut players, &lobby_sender, &ongoing_games);
                    start_computer_games(&mut players, &lobby_sender, &ongoing_games);
                }
            }
            _ = draw_ticker.tick() => {
                let stats = LobbyStats {
                    connected: players.len(),
                    ongoing_games: ongoing_games.load(Ordering::Relaxed),
                };
                if stats.connected != last_stats.connected
                    || stats.ongoing_games != last_stats.ongoing_games
                {
                    for player in players.iter_mut() {
                        player.dirty = true;
                    }
                    last_stats = stats;
                }
                redraw_players(&mut players, &stats).await;
            }
        }
    }
}

async fn process_lobby_events(players: &mut Vec<PendingPlayer>) {
    let mut to_remove = vec![];
    for (i, player) in players.iter_mut().enumerate() {
        while let Some(event) = player.tui.try_next() {
            match event {
                TerminalEvent::Quit => {
                    log::info!("Player {} disconnected", player.tui.username());
                    to_remove.push(i);
                    break;
                }
                TerminalEvent::Resize(_, _) => {
                    player.dirty = true;
                }
                TerminalEvent::IdleWarning(secs) => {
                    player.idle_warning = Some(secs);
                    player.dirty = true;
                }
                TerminalEvent::Key(key) => {
                    log::debug!("lobby key {} {:?} {:?}", player.tui.username(), key.code, key.modifiers);
                    if player.idle_warning.take().is_some() {
                        player.dirty = true;
                    }
                    let prev_discriminant = std::mem::discriminant(&player.mode);

                    if matches!(player.mode, PlayerMode::Idle) {
                        match key.code {
                            KeyCode::Esc | KeyCode::Backspace => {
                                log::info!("Player {} left lobby", player.tui.username());
                                to_remove.push(i);
                                break;
                            }
                            KeyCode::Char('a') => player.mode = PlayerMode::AutoQueue,
                            KeyCode::Char('p') => {
                                player.mode =
                                    PlayerMode::Practicing(Box::new(Game::new_practice()));
                            }
                            KeyCode::Char('g') => {
                                player.mode = PlayerMode::ShowingCode(Box::new(
                                    FriendCodeState::new(generate_invite_code()),
                                ));
                            }
                            // Asciiverse: the computer player
                            KeyCode::Char('c') => player.mode = PlayerMode::Computer,
                            KeyCode::Char('w') => player.mode = PlayerMode::Watching,
                            KeyCode::Char('l') => {
                                player.tui.skill = player.tui.skill.next();
                                player.dirty = true;
                            }
                            _ => {}
                        }
                    } else if let PlayerMode::ShowingCode(state) = &mut player.mode {
                        match key.code {
                            KeyCode::Esc => player.mode = PlayerMode::Idle,
                            KeyCode::Backspace => {
                                if state.typed.is_empty() {
                                    player.mode = PlayerMode::Idle;
                                } else {
                                    state.typed.pop();
                                    state.last_attempt_failed = false;
                                    player.dirty = true;
                                }
                            }
                            KeyCode::Char(c) if c.is_ascii_alphanumeric() => {
                                if state.last_attempt_failed {
                                    state.typed.clear();
                                    state.last_attempt_failed = false;
                                }
                                if state.typed.len() < FRIEND_CODE_LEN {
                                    state.typed.push(c.to_ascii_uppercase());
                                    player.dirty = true;
                                }
                            }
                            _ => {}
                        }
                    } else if matches!(key.code, KeyCode::Esc | KeyCode::Backspace) {
                        player.mode = PlayerMode::Idle;
                    } else if let PlayerMode::Practicing(g) = &mut player.mode {
                        if let Some(cmd) = key_code_to_game_command(key.code) {
                            g.handle_command(GameSide::Red, cmd);
                        }
                    }

                    if prev_discriminant != std::mem::discriminant(&player.mode) {
                        player.dirty = true;
                    }
                }
                _ => {}
            }
        }
    }
    for i in to_remove.into_iter().rev() {
        let player = players.remove(i);
        player.tui.close().await;
    }
}

fn pair_friend_codes(
    players: &mut Vec<PendingPlayer>,
    lobby_sender: &Sender<Tui>,
    ongoing_games: &Arc<AtomicUsize>,
) {
    if !players
        .iter()
        .any(|p| matches!(&p.mode, PlayerMode::ShowingCode(s) if s.typed.len() == FRIEND_CODE_LEN))
    {
        return;
    }

    loop {
        let host_map: HashMap<&str, usize> = players
            .iter()
            .enumerate()
            .filter_map(|(i, p)| match &p.mode {
                PlayerMode::ShowingCode(s) => Some((s.code.as_str(), i)),
                _ => None,
            })
            .collect();

        let pair = players.iter().enumerate().find_map(|(i, typer)| {
            let s = match &typer.mode {
                PlayerMode::ShowingCode(s) if s.typed.len() == FRIEND_CODE_LEN => s,
                _ => return None,
            };
            let &j = host_map.get(s.typed.as_str())?;
            if i == j {
                return None;
            }
            Some((i.min(j), i.max(j)))
        });

        let Some((lo, hi)) = pair else { break };
        let high_player = players.remove(hi);
        let low_player = players.remove(lo);
        log::info!(
            "Friend-code match: {} and {}",
            low_player.tui.username(),
            high_player.tui.username()
        );
        spawn_game(
            low_player.tui,
            high_player.tui,
            lobby_sender.clone(),
            ongoing_games.clone(),
        );
    }

    for player in players.iter_mut() {
        if let PlayerMode::ShowingCode(state) = &mut player.mode {
            if state.typed.len() == FRIEND_CODE_LEN && !state.last_attempt_failed {
                state.last_attempt_failed = true;
                player.dirty = true;
            }
        }
    }
}

fn pair_auto_queue(
    players: &mut Vec<PendingPlayer>,
    lobby_sender: &Sender<Tui>,
    ongoing_games: &Arc<AtomicUsize>,
) {
    loop {
        let auto_indices: Vec<usize> = players
            .iter()
            .enumerate()
            .filter(|(_, p)| matches!(p.mode, PlayerMode::AutoQueue))
            .map(|(i, _)| i)
            .take(2)
            .collect();
        let [i1, i2] = auto_indices.as_slice() else {
            return;
        };
        let p2 = players.remove(*i2);
        let p1 = players.remove(*i1);
        log::info!(
            "Auto-pairing {} and {}",
            p1.tui.username(),
            p2.tui.username()
        );
        spawn_game(p1.tui, p2.tui, lobby_sender.clone(), ongoing_games.clone());
    }
}

/// Asciiverse: players who asked for the computer, or to watch two
/// computers, leave the lobby for a match of their own.
fn start_computer_games(
    players: &mut Vec<PendingPlayer>,
    lobby_sender: &Sender<Tui>,
    ongoing_games: &Arc<AtomicUsize>,
) {
    let mut i = 0;
    while i < players.len() {
        let watching = match players[i].mode {
            PlayerMode::Computer => false,
            PlayerMode::Watching => true,
            _ => {
                i += 1;
                continue;
            }
        };
        let player = players.remove(i);
        let skill = player.tui.skill;
        let seed = rand::random::<u64>();
        if watching {
            log::info!("{} watches two computers", player.tui.username());
            let ais = vec![Ai::new(GameSide::Red, skill, seed), Ai::new(GameSide::Blue, skill, seed ^ 0x9e37)];
            spawn_game_with(Some(player.tui), None, ais, true, lobby_sender.clone(), ongoing_games.clone());
        } else {
            log::info!("{} plays the computer ({})", player.tui.username(), skill.name());
            let ais = vec![Ai::new(GameSide::Blue, skill, seed)];
            spawn_game_with(Some(player.tui), None, ais, false, lobby_sender.clone(), ongoing_games.clone());
        }
    }
}

async fn redraw_players(players: &mut [PendingPlayer], stats: &LobbyStats) {
    for player in players.iter_mut() {
        let PendingPlayer {
            tui,
            mode,
            dirty,
            idle_warning,
        } = player;
        let mut wrote = false;
        match mode {
            PlayerMode::Practicing(g) => {
                if let Ok(img) = g.draw() {
                    let lines = crate::img_lines::img_to_lines(&img);
                    let _ = tui.draw(g, &lines, GameSide::Red, *idle_warning);
                    wrote = true;
                }
            }
            lobby_mode if *dirty => {
                let view = match lobby_mode {
                    PlayerMode::Idle => crate::lobby::LobbyView::Idle,
                    PlayerMode::AutoQueue => crate::lobby::LobbyView::AutoQueue,
                    PlayerMode::ShowingCode(state) => crate::lobby::LobbyView::ShowingCode {
                        code: state.code.as_str(),
                        typed: state.typed.as_str(),
                        last_attempt_failed: state.last_attempt_failed,
                    },
                    PlayerMode::Practicing(_) => unreachable!(),
                    // started on the next update tick
                    PlayerMode::Computer | PlayerMode::Watching => crate::lobby::LobbyView::Idle,
                };
                let _ = tui.draw_lobby(stats, view, *idle_warning);
                *dirty = false;
                wrote = true;
            }
            _ => {}
        }
        if wrote {
            let _ = tui.push_data().await;
        }
    }
}

fn spawn_game(
    red_tui: Tui,
    blue_tui: Tui,
    lobby_sender: Sender<Tui>,
    ongoing_games: Arc<AtomicUsize>,
) {
    spawn_game_with(Some(red_tui), Some(blue_tui), vec![], false, lobby_sender, ongoing_games);
}

/// A match where a side can be a person, the computer, or (Asciiverse)
/// both: in a watched game the person on Red only looks on, and Esc takes
/// them back to the lobby.
fn spawn_game_with(
    red_tui: Option<Tui>,
    blue_tui: Option<Tui>,
    mut ais: Vec<Ai>,
    watching: bool,
    lobby_sender: Sender<Tui>,
    ongoing_games: Arc<AtomicUsize>,
) {
    ongoing_games.fetch_add(1, Ordering::Relaxed);
    task::spawn(async move {
        let mut red = red_tui;
        let mut blue = blue_tui;
        let mut last_snapshot = Instant::now();
        let mut red_warn: Option<u32> = None;
        let mut blue_warn: Option<u32> = None;
        let mut game = Game::new();
        log::info!("Game {} spawned", game.id);

        let mut update_ticker = time::interval(UPDATE_TIME_STEP);
        let mut draw_ticker = time::interval(DRAW_TIME_STEP);
        draw_ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            if let GameState::Ending { time, .. } = game.state {
                if Instant::now() - time > AFTER_GAME_DELAY {
                    break;
                }
            }
            if red.is_none() && blue.is_none() {
                break;
            }

            select! {
                _ = update_ticker.tick() => {
                    for ai in ais.iter_mut() {
                        if let Some(cmd) = ai.step(&game, UPDATE_TIME_STEP.as_millis() as f32) {
                            game.handle_command(ai.side, cmd);
                        }
                    }
                    if let Err(e) = game.update() {
                        log::error!("Error updating game: {e}");
                        break;
                    }
                    if last_snapshot.elapsed() >= SNAPSHOT_EVERY {
                        last_snapshot = Instant::now();
                        log_snapshot(&game);
                    }
                }
                _ = draw_ticker.tick() => {
                    if let Err(e) = draw_and_push(&game, &mut red, &mut blue, red_warn, blue_warn).await {
                        log::error!("Error rendering game: {e}");
                        break;
                    }
                }
                event = next_or_pending(&mut red) => {
                    if watching {
                        // looking on: any way out goes back to the lobby
                        if matches!(event, TerminalEvent::Quit) {
                            if let Some(tui) = red.take() {
                                tui.close().await;
                            }
                        } else if matches!(event, TerminalEvent::Key(k) if k.code == KeyCode::Esc) {
                            if let Some(mut tui) = red.take() {
                                tui.repeat_watch = false;
                                if let Err(err) = lobby_sender.send(tui).await {
                                    err.0.close().await;
                                }
                            }
                        }
                    } else {
                        handle_event(&mut game, GameSide::Red, event, &mut red, &mut red_warn).await;
                    }
                }
                event = next_or_pending(&mut blue) => {
                    handle_event(&mut game, GameSide::Blue, event, &mut blue, &mut blue_warn).await;
                }
            }
        }

        let winner = game.winner();

        for (side, slot) in [(GameSide::Red, red), (GameSide::Blue, blue)] {
            if let Some(mut tui) = slot {
                if !watching {
                    tui.record_game(winner == Some(side));
                }
                if let Err(err) = lobby_sender.send(tui).await {
                    err.0.close().await;
                }
            }
        }
        ongoing_games.fetch_sub(1, Ordering::Relaxed);
    });
}

async fn draw_and_push(
    game: &Game,
    red: &mut Option<Tui>,
    blue: &mut Option<Tui>,
    red_warn: Option<u32>,
    blue_warn: Option<u32>,
) -> AppResult<()> {
    if red.is_none() && blue.is_none() {
        return Ok(());
    }
    let image = game.draw()?;
    let image_lines = crate::img_lines::img_to_lines(&image);
    for (slot, side, warn) in [
        (red.as_mut(), GameSide::Red, red_warn),
        (blue.as_mut(), GameSide::Blue, blue_warn),
    ] {
        if let Some(t) = slot {
            t.draw(game, &image_lines, side, warn)?;
        }
    }
    match (red.as_mut(), blue.as_mut()) {
        (Some(r), Some(b)) => {
            let (a, c) = tokio::join!(r.push_data(), b.push_data());
            a?;
            c?;
        }
        (Some(t), None) | (None, Some(t)) => t.push_data().await?,
        (None, None) => {}
    }
    Ok(())
}

const SNAPSHOT_EVERY: Duration = Duration::from_millis(500);

/// Session logging (Asciiverse): the state of a match twice a second.
fn log_snapshot(game: &Game) {
    use sshattrick_core::traits::Body;
    let (r, b) = (&game.red_data.player, &game.blue_data.player);
    log::debug!(
        "snapshot game {} state {} score {}-{} red {:.0?} v {:.3?} blue {:.0?} v {:.3?} puck {:?} possession {:?} time {}s",
        game.id,
        match game.state {
            GameState::Starting { .. } => "starting",
            GameState::Running => "running",
            GameState::AfterGoal { .. } => "after goal",
            GameState::Ending { .. } => "ending",
        },
        game.red_data.score,
        game.blue_data.score,
        r.position_float(),
        r.velocity,
        b.position_float(),
        b.velocity,
        game.puck.position(),
        game.puck.possession,
        game.timer / 1000
    );
}

async fn handle_event(
    game: &mut Game,
    side: GameSide,
    event: TerminalEvent,
    own_tui: &mut Option<Tui>,
    own_warn: &mut Option<u32>,
) {
    match event {
        TerminalEvent::Quit => {
            if let Some(tui) = own_tui.take() {
                tui.close().await;
            }
            *own_warn = None;
            if !matches!(game.state, GameState::Ending { .. }) {
                game.end_with_winner(Some(side.opposite()), true);
            }
        }
        TerminalEvent::IdleWarning(secs) => {
            *own_warn = Some(secs);
        }
        TerminalEvent::Key(crossterm::event::KeyEvent {
            code: KeyCode::Esc, ..
        }) => {
            *own_warn = None;
            if !matches!(game.state, GameState::Ending { .. }) {
                game.end_with_winner(Some(side.opposite()), true);
            }
        }
        TerminalEvent::Key(key) => {
            *own_warn = None;
            log::debug!("game key {side:?} {:?} {:?}", key.code, key.modifiers);
            if let Some(cmd) = key_code_to_game_command(key.code) {
                game.handle_command(side, cmd);
            }
        }
        _ => {}
    }
}

async fn next_or_pending(tui: &mut Option<Tui>) -> TerminalEvent {
    match tui {
        Some(t) => t.next().await,
        None => std::future::pending().await,
    }
}
