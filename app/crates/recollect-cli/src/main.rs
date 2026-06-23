//! `recollect` — the one client binary. Two orthogonal axes: transport (a local
//! in-process engine, or `--server` WebSocket to the authoritative server) and
//! interface (interactive TUI, the default, or headless JSON/autoplay). All
//! modes embed the same `recollect-core`, so a preview never diverges from the
//! server's ruling, and each renders only its own seat's view.
//!
//!   recollect                       # local TUI vs the AI (you are Seat A)
//!   recollect hotseat               # local TUI, two humans
//!   recollect watch                 # local TUI, spectate two AIs
//!   recollect online new            # networked TUI: create + claim seat A
//!   recollect online join ID TOKEN  # networked TUI: join a match
//!   recollect autoplay              # headless: AI self-play → result (--ndjson)
//!   recollect headless              # headless JSON-lines: drive Seat A, AI = Seat B
//!
//! Global flags: --seed N · --difficulty <easy|normal|hard|expert> ·
//! --server URL · --json (machine output for `online`) · --ndjson (autoplay).
//!
//! The transports + render live in the crate's **library** (`lib.rs`) so the
//! `tui_capture` example can drive them too; this binary is the CLI shell over it.
#![forbid(unsafe_code)]
use recollect_cli::{headless, local, online};

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "recollect",
    version,
    about = "Recollect — a fading-Memory card game client"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
    /// Match seed (reproducible). Defaults to the wall clock.
    #[arg(long, global = true)]
    seed: Option<u64>,
    /// AI difficulty tier (for vs-AI / autoplay).
    #[arg(long, value_enum, global = true, default_value_t = Diff::Normal)]
    difficulty: Diff,
    /// Server base URL for `online` mode.
    #[arg(long, global = true, default_value = "http://localhost:8080")]
    server: String,
    /// `online`: emit/accept JSON instead of the TUI (machine driver).
    #[arg(long, global = true)]
    json: bool,
    /// `autoplay`: stream every event as NDJSON.
    #[arg(long, global = true)]
    ndjson: bool,
    /// `online new`: play the server's bot (Seat B) instead of waiting for a human.
    #[arg(long, global = true)]
    vs_bot: bool,
    /// `online new`: open a 2v2 lobby (prints the other three slot tokens).
    #[arg(long = "2v2", global = true)]
    two_v_two: bool,
    /// The faction the AI opponent fields in local play — the Solace (the default
    /// antagonist) or a Lorekeeper. Either way the bot seat fields a NAMED character
    /// (a Solace disposition or a Lorekeeper style), exactly as online matchmaking does.
    #[arg(long, value_enum, global = true, default_value_t = Faction::Solace)]
    faction: Faction,
}

/// The faction the bot opponent fields — a CLI mirror of `recollect_core::types::Faction`,
/// so a named character (Solace disposition or Lorekeeper style) is picked the same way the
/// server's matchmaking does. The player's own seat is always a Lorekeeper.
#[derive(Copy, Clone, ValueEnum)]
enum Faction {
    /// The Solace — the fading-Memory antagonist (the default single-player foe).
    Solace,
    /// A Lorekeeper opponent — a keeper character, a Skirmish-style mirror.
    Lorekeeper,
}

impl Faction {
    fn to_core(self) -> recollect_core::types::Faction {
        match self {
            Faction::Solace => recollect_core::types::Faction::Solace,
            Faction::Lorekeeper => recollect_core::types::Faction::Lorekeeper,
        }
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// Local TUI vs the AI (you are Seat A). The default.
    Play,
    /// Local TUI, two humans on one terminal.
    Hotseat,
    /// Local TUI, spectate two AIs.
    Watch,
    /// Networked play against the authoritative server.
    Online {
        #[command(subcommand)]
        what: OnlineCmd,
    },
    /// Headless: the AI plays one seeded match and prints the result.
    Autoplay,
    /// Headless JSON-lines: drive Seat A via stdin, the AI plays Seat B.
    Headless,
}

#[derive(Subcommand)]
enum OnlineCmd {
    /// Create a match, claim seat A, print the seat-B token.
    New,
    /// Join an existing match as seat B.
    Join { match_id: String, token: String },
}

#[derive(Copy, Clone, ValueEnum)]
enum Diff {
    Easy,
    Normal,
    Hard,
    Expert,
}

impl Diff {
    fn to_bot(self) -> recollect_bot::Difficulty {
        match self {
            Diff::Easy => recollect_bot::Difficulty::Easy,
            Diff::Normal => recollect_bot::Difficulty::Normal,
            Diff::Hard => recollect_bot::Difficulty::Hard,
            Diff::Expert => recollect_bot::Difficulty::Expert,
        }
    }
    /// The `?difficulty=` query value the server parses.
    fn as_str(self) -> &'static str {
        match self {
            Diff::Easy => "easy",
            Diff::Normal => "normal",
            Diff::Hard => "hard",
            Diff::Expert => "expert",
        }
    }
}

fn main() {
    let cli = Cli::parse();
    let seed = cli.seed.unwrap_or_else(local::now_seed);
    let diff = cli.difficulty.to_bot();
    let opp_faction = cli.faction.to_core();
    match cli.cmd.unwrap_or(Cmd::Play) {
        Cmd::Play => local::run(seed, false, true, diff, opp_faction),
        Cmd::Hotseat => local::run(seed, false, false, diff, opp_faction),
        Cmd::Watch => local::run(seed, true, true, diff, opp_faction),
        Cmd::Online { what } => {
            let action = match what {
                OnlineCmd::New => {
                    // `--2v2` opens a four-slot lobby; `--vs-bot` fills Seat B with
                    // the AI (1v1 only). Otherwise a plain human-vs-human match.
                    let mut query = if cli.two_v_two {
                        "?mode=2v2".to_string()
                    } else if cli.vs_bot {
                        format!("?opponent=bot&difficulty={}", cli.difficulty.as_str())
                    } else {
                        String::new()
                    };
                    // An explicit `--seed` makes the online match reproducible (the server uses
                    // it; otherwise it draws OS entropy) — the same `--seed` that pins a local match.
                    if let Some(s) = cli.seed {
                        query.push_str(if query.is_empty() { "?" } else { "&" });
                        query.push_str(&format!("seed={s}"));
                    }
                    online::Action::New { query }
                }
                OnlineCmd::Join { match_id, token } => online::Action::Join { match_id, token },
            };
            // Only this transport needs the async runtime; local/headless are sync.
            tokio::runtime::Runtime::new()
                .expect("tokio runtime")
                .block_on(online::run(cli.server, action, cli.json));
        }
        Cmd::Autoplay => headless::autoplay(seed, diff, cli.ndjson),
        Cmd::Headless => headless::protocol(seed, diff),
    }
}
