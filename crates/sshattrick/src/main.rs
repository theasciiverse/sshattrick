use clap::{ArgAction, Parser};
use log::LevelFilter;
use log4rs::{
    append::file::FileAppender,
    config::{Appender, Root},
    encode::pattern::PatternEncoder,
    Config,
};
use sshattrick::local::{play_here, restore_terminal, LocalOptions};
use sshattrick::{store_path, AppResult, SshattrickGame, Skill, Start};
use std::net::{TcpListener, UdpSocket};

const DEFAULT_PORT: u16 = 3020;

/// Asciiverse: with no options ssHattrick plays here, in this terminal,
/// and opens the game to your local network so friends can join with ssh.
/// `--server` runs only the server, as upstream does.
#[derive(Parser, Debug)]
#[clap(
    name = "ssHattrick",
    about = "Hockey in the terminal: against the computer, a friend on your network, or anyone over ssh",
    author,
    version,
    long_about = None
)]
struct Args {
    #[clap(long, short = 'p', action=ArgAction::Set, help = "Port to open for friends (default 3020, or the next free one)")]
    port: Option<u16>,
    #[clap(long, help = "Only run the server, for people connecting with ssh")]
    server: bool,
    #[clap(long, help = "Play here without opening the game to the network")]
    offline: bool,
    #[clap(long, value_name = "HOST[:PORT]", help = "Join a friend's game (runs ssh)")]
    join: Option<String>,
    #[clap(long, value_name = "LEVEL", help = "Start straight into a match against the computer: easy, normal or hard")]
    cpu: Option<String>,
    #[clap(long, help = "Start straight into practice")]
    practice: bool,
    #[clap(long, help = "Watch two computers play, one match after another")]
    watch: bool,
}

/// Logging to sshattrick.log beside the settings; RUST_LOG=debug for
/// every key and a snapshot of each match twice a second.
fn init_logging() -> AppResult<()> {
    let level = match std::env::var("RUST_LOG").unwrap_or_default().to_ascii_lowercase().as_str() {
        "trace" => LevelFilter::Trace,
        "debug" => LevelFilter::Debug,
        "warn" => LevelFilter::Warn,
        "error" => LevelFilter::Error,
        "off" => LevelFilter::Off,
        _ => LevelFilter::Info,
    };
    let logfile_path = store_path("sshattrick.log")?;
    let logfile = FileAppender::builder()
        .append(false)
        .encoder(Box::new(PatternEncoder::new("{d(%H:%M:%S%.3f)} {l} - {m}\n")))
        .build(logfile_path)?;
    let config = Config::builder()
        .appender(Appender::builder().build("logfile", Box::new(logfile)))
        .build(Root::builder().appender("logfile").build(level))?;
    log4rs::init_config(config)?;
    Ok(())
}

/// The server keeps its host key in ./keys; keep it beside the settings
/// rather than wherever ssHattrick was started from.
fn enter_data_dir() -> AppResult<()> {
    let dir = store_path("keys")?;
    if let Some(parent) = dir.parent() {
        std::env::set_current_dir(parent)?;
    }
    Ok(())
}

/// The first free port from `from`, trying ten.
fn free_port(from: u16) -> Option<u16> {
    (from..from.saturating_add(10)).find(|&p| TcpListener::bind(("0.0.0.0", p)).is_ok())
}

/// This computer's address on the local network, as others see it.
fn lan_address() -> Option<String> {
    // a UDP "connect" sends nothing; it only picks the outgoing interface
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?;
    let ip = s.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then(|| ip.to_string())
}

fn join(target: &str) -> AppResult<()> {
    let (host, port) = match target.rsplit_once(':') {
        Some((h, p)) if p.parse::<u16>().is_ok() => (h.to_string(), p.to_string()),
        _ => (target.to_string(), DEFAULT_PORT.to_string()),
    };
    log::info!("joining {host} port {port} with ssh");
    let status = std::process::Command::new("ssh")
        .args(["-t", "-p", &port, "-o", "StrictHostKeyChecking=accept-new", &host])
        .status()
        .map_err(|e| anyhow::anyhow!("could not run ssh ({e}); joining a game needs an ssh client"))?;
    std::process::exit(status.code().unwrap_or(1));
}

#[tokio::main]
async fn main() -> AppResult<()> {
    let args = Args::parse();
    init_logging()?;
    log::info!("start {} {:?}", env!("CARGO_PKG_VERSION"), args);

    if let Some(target) = &args.join {
        return join(target);
    }
    let skill = match args.cpu.as_deref() {
        None => Skill::Normal,
        Some(s) => Skill::parse(s).ok_or_else(|| anyhow::anyhow!("--cpu takes easy, normal or hard"))?,
    };
    enter_data_dir()?;
    let game = SshattrickGame::new();

    if args.server {
        let port = args.port.unwrap_or(DEFAULT_PORT);
        frittura_ssh_core::run_server(game, port).await?;
        return Ok(());
    }

    let mut lan_hint = None;
    if !args.offline {
        match free_port(args.port.unwrap_or(DEFAULT_PORT)) {
            Some(port) => {
                let host = lan_address().unwrap_or_else(|| "<this computer's address>".to_string());
                lan_hint = Some(format!("ssh -p {port} {host}"));
                let server_game = game.clone();
                tokio::spawn(async move {
                    if let Err(e) = frittura_ssh_core::run_server(server_game, port).await {
                        log::warn!("not open to the network: {e}");
                    }
                });
            }
            None => log::warn!("no free port to open the game to the network"),
        }
    }

    let start = if args.watch {
        Some(Start::Watch)
    } else if args.practice {
        Some(Start::Practice)
    } else if args.cpu.is_some() {
        Some(Start::Computer)
    } else {
        None
    };
    let username = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "player".to_string());
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        default_hook(info);
    }));
    let result = play_here(
        &game,
        LocalOptions { username, start, repeat_watch: args.watch, skill, lan_hint },
    )
    .await;
    restore_terminal();
    if let Err(e) = &result {
        eprintln!("ssHattrick: {e}");
    }
    // the input thread and the server stop with the process
    std::process::exit(if result.is_ok() { 0 } else { 1 });
}
