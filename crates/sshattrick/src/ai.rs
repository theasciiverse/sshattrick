//! The computer player, an Asciiverse addition: it plays one side through
//! the same commands a person sends, at a key-repeat rate rather than every
//! physics tick, so it accelerates, turns and shoots the way a person
//! holding the arrow keys does.
//!
//! Loose puck: skate the stick onto where the puck is going. Opponent has
//! it: chase the carrier and steal it (which also keeps our goalie, who
//! copies our height on the ice, level with the puck). Own puck: carry it
//! into range, face the goal, shoot, and steer the wind-up at the side of
//! the goal the keeper is not covering.

use sshattrick_core::constants::*;
use sshattrick_core::traits::Body;
use sshattrick_core::{Game, GameCommand, GameSide, GameState};
use glam::Vec2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skill {
    Easy,
    Normal,
    Hard,
}

impl Skill {
    pub const ALL: [Skill; 3] = [Skill::Easy, Skill::Normal, Skill::Hard];

    pub fn name(self) -> &'static str {
        match self {
            Skill::Easy => "easy",
            Skill::Normal => "normal",
            Skill::Hard => "hard",
        }
    }

    pub fn parse(s: &str) -> Option<Skill> {
        Skill::ALL.into_iter().find(|k| k.name() == s)
    }

    pub fn next(self) -> Skill {
        match self {
            Skill::Easy => Skill::Normal,
            Skill::Normal => Skill::Hard,
            Skill::Hard => Skill::Easy,
        }
    }

    /// Key presses per second: a held key repeats at about 30.
    fn press_hz(self) -> f32 {
        match self {
            Skill::Easy => 14.0,
            Skill::Normal => 22.0,
            Skill::Hard => 30.0,
        }
    }

    /// How often it looks again at where things are going, in ms.
    fn think_ms(self) -> f32 {
        match self {
            Skill::Easy => 240.0,
            Skill::Normal => 150.0,
            Skill::Hard => 80.0,
        }
    }

    /// How far off its aim can be, in pixels at the goal.
    fn aim_error(self) -> f32 {
        match self {
            Skill::Easy => 11.0,
            Skill::Normal => 6.0,
            Skill::Hard => 3.0,
        }
    }

    /// How far from the goal line it is willing to shoot.
    fn shoot_range(self) -> f32 {
        match self {
            Skill::Easy => 50.0,
            Skill::Normal => 62.0,
            Skill::Hard => 72.0,
        }
    }
}

pub struct Ai {
    pub side: GameSide,
    pub skill: Skill,
    press_in: f32,
    think_in: f32,
    /// where the stick is headed, refreshed every think
    target: Vec2,
    /// where on the goal line the current shot is going
    aim: Option<Vec2>,
    seed: u64,
    /// where it was, and for how long it has not got anywhere from there
    last_pos: Vec2,
    stuck_ms: f32,
    /// sliding sideways out of a clinch: which way, and for how long
    evade: Option<(GameCommand, f32)>,
    /// how many escapes in a row have not freed it; picks the next one
    escapes: u32,
}

const GOAL_MID_Y: f32 = (GOALIE_AREA_MIN_Y + GOALIE_AREA_MAX_Y) as f32 / 2.0;

impl Ai {
    pub fn new(side: GameSide, skill: Skill, seed: u64) -> Ai {
        Ai {
            side,
            skill,
            press_in: 0.0,
            think_in: 0.0,
            target: Vec2::ZERO,
            aim: None,
            seed: seed | 1,
            last_pos: Vec2::ZERO,
            stuck_ms: 0.0,
            evade: None,
            escapes: 0,
        }
    }

    fn unit(&mut self) -> f32 {
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.seed = x;
        (x >> 40) as f32 / (1u64 << 24) as f32
    }

    /// +1 when we attack to the right (Red), -1 to the left (Blue).
    fn attack(&self) -> f32 {
        match self.side {
            GameSide::Red => 1.0,
            GameSide::Blue => -1.0,
        }
    }

    fn goal_x(&self) -> f32 {
        match self.side {
            GameSide::Red => MAX_X as f32,
            GameSide::Blue => MIN_X as f32,
        }
    }

    /// One fixed step of `dt_ms`; returns the key it presses, if any.
    pub fn step(&mut self, game: &Game, dt_ms: f32) -> Option<GameCommand> {
        if game.state != GameState::Running {
            self.press_in = 0.0;
            self.think_in = 0.0;
            self.aim = None;
            return None;
        }
        self.think_in -= dt_ms;
        self.press_in -= dt_ms;
        let (me_pos, opp_pos) = match self.side {
            GameSide::Red => (game.red_data.player.position_float(), game.blue_data.player.position_float()),
            GameSide::Blue => (game.blue_data.player.position_float(), game.red_data.player.position_float()),
        };
        // two skaters pushing into each other lock up (a collision puts both
        // back where they were): notice, and slide out sideways
        if (me_pos - self.last_pos).length() < 1.0 {
            self.stuck_ms += dt_ms;
        } else {
            self.stuck_ms = 0.0;
            if (me_pos - self.last_pos).length() > 12.0 {
                self.escapes = 0;
            }
            self.last_pos = me_pos;
        }
        if let Some((_, left)) = &mut self.evade {
            *left -= dt_ms;
            if *left <= 0.0 {
                self.evade = None;
            }
        }
        if self.stuck_ms > 300.0 && self.evade.is_none() {
            // sideways away from the other skater, then back off from where
            // it was trying to go (a goal crease, the boards), then the
            // other way sideways
            let sideways = if me_pos.y <= opp_pos.y { GameCommand::Up } else { GameCommand::Down };
            let flip = |c| if c == GameCommand::Up { GameCommand::Down } else { GameCommand::Up };
            let back = if self.target.x > me_pos.x { GameCommand::Left } else { GameCommand::Right };
            let cmd = match self.escapes % 3 {
                0 => sideways,
                1 => back,
                _ => flip(sideways),
            };
            // never into the boards
            let cmd = match cmd {
                GameCommand::Up if me_pos.y < MIN_Y as f32 + 8.0 => GameCommand::Down,
                GameCommand::Down if me_pos.y > MAX_Y as f32 - 24.0 => GameCommand::Up,
                c => c,
            };
            self.evade = Some((cmd, 350.0));
            self.escapes += 1;
            self.stuck_ms = 0.0;
        }
        if self.press_in > 0.0 {
            return None;
        }
        let jitter = 0.85 + 0.3 * self.unit();
        self.press_in += 1000.0 / self.skill.press_hz() * jitter;

        let (me, opp) = match self.side {
            GameSide::Red => (&game.red_data, &game.blue_data),
            GameSide::Blue => (&game.blue_data, &game.red_data),
        };
        let player = &me.player;
        let puck = &game.puck;
        let puck_pos = puck.position().as_vec2();

        if let Some((cmd, _)) = self.evade {
            if !game.red_data.player.shooting_state.is_shooting() && !game.blue_data.player.shooting_state.is_shooting() {
                return Some(cmd);
            }
        }

        // winding up a shot: steer it at the open side, as hard as it goes
        if player.shooting_state.is_shooting() {
            let aim = *self.aim.get_or_insert(Vec2::new(self.goal_x(), GOAL_MID_Y));
            let want = (aim - puck_pos).normalize_or_zero();
            let have = player.shooting_state.direction.unwrap_or(player.velocity);
            // first turn the shot onto the aim, then put power behind it
            let turned = want * have.length().max(1.0) - have;
            if turned.length() > SHOOTING_DIRECTION_MODIFIER * 0.5 {
                return axis_press(turned, 0.0);
            }
            return axis_press(want * SHOOTING_DIRECTION_MAX_MAGNITUDE - have, SHOOTING_DIRECTION_MODIFIER * 0.5);
        }
        self.aim = None;

        let ours = puck.possession == Some(self.side);
        if ours {
            let to_goal = (self.goal_x() - puck_pos.x) * self.attack();
            let facing = player.orientation.shooting_direction().x * self.attack() > 0.6;
            // a real angle on the goal: in front of it, not from the corner
            let in_lane = to_goal > 14.0 && (puck_pos.y - GOAL_MID_Y).abs() < to_goal * 0.9;
            if to_goal < self.skill.shoot_range() && facing && in_lane && player.after_shooting_counter == 0.0 {
                // aim at the end of the goal the keeper is furthest from
                let keeper_y = opp.goalie.position().y as f32 + 3.0;
                let end = if keeper_y < GOAL_MID_Y { GOALIE_AREA_MAX_Y as f32 - 5.0 } else { GOALIE_AREA_MIN_Y as f32 + 4.0 };
                let err = (self.unit() * 2.0 - 1.0) * self.skill.aim_error();
                self.aim = Some(Vec2::new(self.goal_x(), end + err));
                return Some(GameCommand::Shoot);
            }
            if !facing {
                // turning toward the goal also skates toward it
                return Some(if self.attack() > 0.0 { GameCommand::Right } else { GameCommand::Left });
            }
            if self.think_in <= 0.0 {
                self.think_in = self.skill.think_ms();
                let spot_x = self.goal_x() - self.attack() * self.skill.shoot_range() * 0.7;
                let spot_y = GOAL_MID_Y + (self.unit() * 2.0 - 1.0) * 10.0;
                self.target = Vec2::new(spot_x, spot_y);
            }
            return steer(player.position_float(), player.velocity, self.target, 0.0025);
        }

        // loose or theirs: get the stick (the catcher) onto the puck
        if self.think_in <= 0.0 {
            self.think_in = self.skill.think_ms();
            let catcher = player.catcher_position().as_vec2();
            let lead = if puck.possession.is_none() { ((puck_pos - catcher).length() / 0.25).min(350.0) } else { 60.0 };
            let ahead = puck_pos + puck.velocity * lead;
            // move the body so that the catcher lands on the puck
            self.target = clamp_to_ice(player.position_float() + (ahead - catcher));
        }
        steer(player.position_float(), player.velocity, self.target, 0.004)
    }
}

fn clamp_to_ice(p: Vec2) -> Vec2 {
    Vec2::new(p.x.clamp(MIN_X as f32, MAX_X as f32 - 10.0), p.y.clamp(MIN_Y as f32, MAX_Y as f32 - 10.0))
}

/// The arrow that best turns `velocity` toward moving to `target`, slowing
/// down as it gets close (`gain`: speed per pixel of distance).
fn steer(pos: Vec2, velocity: Vec2, target: Vec2, gain: f32) -> Option<GameCommand> {
    let to = target - pos;
    let want = to.normalize_or_zero() * (to.length() * gain).min(MAX_PLAYER_VELOCITY);
    axis_press(want - velocity, ACCELERATION * 0.5)
}

/// Press along whichever axis is furthest off, if any is off by more than
/// `dead`.
fn axis_press(err: Vec2, dead: f32) -> Option<GameCommand> {
    if err.x.abs() < dead && err.y.abs() < dead {
        return None;
    }
    Some(if err.x.abs() >= err.y.abs() {
        if err.x > 0.0 { GameCommand::Right } else { GameCommand::Left }
    } else if err.y > 0.0 {
        GameCommand::Down
    } else {
        GameCommand::Up
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// A real-time match, as the matchmaker runs it (the engine keeps time
    /// with Instant): `secs` long, each side `None` for a player who never
    /// touches the keys. Returns the score.
    fn play(red: Option<Skill>, blue: Option<Skill>, secs: u64) -> (u8, u8) {
        let mut game = Game::new();
        let mut ais: Vec<Ai> = [(GameSide::Red, red), (GameSide::Blue, blue)]
            .into_iter()
            .filter_map(|(side, s)| s.map(|s| Ai::new(side, s, 7 + side as u64)))
            .collect();
        let end = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < end && !matches!(game.state, GameState::Ending { .. }) {
            std::thread::sleep(Duration::from_millis(10));
            for ai in &mut ais {
                if let Some(cmd) = ai.step(&game, 10.0) {
                    game.handle_command(ai.side, cmd);
                }
            }
            game.update().unwrap();
        }
        (game.red_data.score, game.blue_data.score)
    }

    #[test]
    #[ignore = "plays real time; run with --ignored"]
    fn two_computers_never_lock_up() {
        // Two skaters used to lock together for good (game.rs touching_ticks)
        // and the computer used to push into goal creases; a minute of play
        // must keep moving and see goals.
        let mut game = Game::new();
        let mut ais = [Ai::new(GameSide::Red, Skill::Normal, 3), Ai::new(GameSide::Blue, Skill::Normal, 5)];
        let end = Instant::now() + Duration::from_secs(60);
        let (mut still, mut last) = (0u32, (Vec2::ZERO, Vec2::ZERO));
        while Instant::now() < end && !matches!(game.state, GameState::Ending { .. }) {
            std::thread::sleep(Duration::from_millis(10));
            for ai in &mut ais {
                if let Some(cmd) = ai.step(&game, 10.0) {
                    game.handle_command(ai.side, cmd);
                }
            }
            game.update().unwrap();
            let now = (game.red_data.player.position_float(), game.blue_data.player.position_float());
            if game.state == GameState::Running && now == last {
                still += 1;
            }
            last = now;
        }
        let goals = game.red_data.score + game.blue_data.score;
        eprintln!("normal-normal {}-{}, both still for {} ticks", game.red_data.score, game.blue_data.score, still);
        assert!(still < 300, "stood still {still} ticks");
        assert!(goals >= 1, "no goals in a minute");
    }

    #[test]
    #[ignore = "plays real time; run with --ignored"]
    fn every_level_scores_against_an_empty_chair() {
        let results: Vec<_> = Skill::ALL
            .into_iter()
            .map(|s| std::thread::spawn(move || (s, play(None, Some(s), 40))))
            .collect::<Vec<_>>()
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect();
        for (s, (red, blue)) in results {
            eprintln!("{} against nobody: {red}-{blue}", s.name());
            assert!(blue >= 1, "{} never scored", s.name());
        }
    }

    #[test]
    #[ignore = "plays real time; run with --ignored"]
    fn hard_beats_easy_over_two_matches() {
        let a = std::thread::spawn(|| play(Some(Skill::Hard), Some(Skill::Easy), 90));
        let b = std::thread::spawn(|| play(Some(Skill::Easy), Some(Skill::Hard), 90));
        let (a, b) = (a.join().unwrap(), b.join().unwrap());
        eprintln!("hard-easy {}-{}, easy-hard {}-{}", a.0, a.1, b.0, b.1);
        assert!(a.0 as i32 + b.1 as i32 > a.1 as i32 + b.0 as i32, "hard {a:?} {b:?}");
    }
}
