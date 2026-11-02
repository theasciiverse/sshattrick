pub mod ai;
mod big_text;
pub mod img_lines;
pub mod local;
mod lobby;
mod matchmaker;
mod out;
mod ssh_game;
mod tui;
mod ui;

pub use ai::Skill;
pub use ssh_game::SshattrickGame;
pub use tui::Start;
pub use sshattrick_core::types::AppResult;
pub use sshattrick_core::utils::store_path;
