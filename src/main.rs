use clap::{Parser, Subcommand};
use std::io::{IsTerminal, Read, Write};
use tinybrain::{Config, Hit, Kind, LEVELS, Memory, TinyBrain, brain, current_project};

#[derive(Parser)]
#[command(
    name = "tinybrain",
    version,
    about = "🧠 a tiny memory for pentesters and their agents",
    after_help = "Just type to remember:  tinybrain \"jenkins on 10.0.4.12 has anon script console\""
)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Remember something (or pipe it in)
    #[command(visible_alias = "r")]
    Remember { text: Vec<String> },
    /// Find memories by meaning
    #[command(visible_alias = "f")]
    Recall {
        query: Vec<String>,
        #[arg(short = 'n', long, default_value_t = 5)]
        limit: usize,
        /// Only this engagement (git repo / folder name)
        #[arg(long)]
        here: bool,
        /// Show secrets unmasked
        #[arg(long)]
        reveal: bool,
        #[arg(long)]
        json: bool,
    },
    /// Show recent memories
    #[command(visible_alias = "ls")]
    List {
        #[arg(short = 'n', long, default_value_t = 10)]
        limit: usize,
        #[arg(short, long)]
        category: Option<String>,
        #[arg(long)]
        here: bool,
        #[arg(long)]
        reveal: bool,
        #[arg(long)]
        json: bool,
    },
    /// Forget a memory
    #[command(visible_alias = "rm")]
    Forget { id: i64 },
    /// Pick your brain: Jev, strands-decider, or offline
    Init,
    /// Which brain is active, and what's in it
    Status,
    /// Run as an MCP server on stdio
    Mcp,
}

const SUBCOMMANDS: &[&str] = &[
    "remember", "r", "recall", "f", "list", "ls", "forget", "rm", "init", "status", "mcp", "help",
];

fn main() {
    // `tinybrain some thought` == `tinybrain remember some thought`
    let mut args: Vec<String> = std::env::args().collect();
    if args
        .get(1)
        .is_some_and(|a| !a.starts_with('-') && !SUBCOMMANDS.contains(&a.as_str()))
    {
        args.insert(1, "remember".into());
    }
    let cli = Cli::parse_from(args);
    if let Err(e) = run(cli.cmd) {
        eprintln!("{} {e}", paint("31", "✗"));
        std::process::exit(1);
    }
}

fn run(cmd: Option<Cmd>) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(Cmd::Init) = cmd {
        return init();
    }
    let tb = TinyBrain::open()?;
    let here = |h: bool| if h { current_project() } else { None };
    match cmd {
        Some(Cmd::Remember { text }) => {
            let mut text = text.join(" ");
            if text.trim().is_empty() && !std::io::stdin().is_terminal() {
                std::io::stdin().read_to_string(&mut text)?;
            }
            if text.trim().is_empty() {
                return Err("nothing to remember — try: tinybrain \"your thought\"".into());
            }
            let spin = tb.brain().is_remote() && std::io::stderr().is_terminal();
            if spin {
                eprint!("🧠 asking {}…", tb.brain().name());
            }
            let r = tb.remember(&text, current_project().as_deref())?;
            if spin {
                eprint!("\r\x1b[2K");
            }
            let m = &r.memory;
            let by = match &r.verdict.fallback {
                Some(_) => "keywords (fallback)".to_string(),
                None if r.verdict.by == "offline" => "keywords".to_string(),
                None => format!("{} {:.0}%", r.verdict.by, r.verdict.confidence * 100.0),
            };
            println!(
                "{} {} {} {} {}",
                paint("1", &format!("🧠 #{}", m.id)),
                brain::emoji(&m.category),
                paint("36", &m.category),
                severity(m.importance),
                paint("2", &format!("· {by}")),
            );
            if m.secret {
                println!(
                    "   🔒 {}",
                    paint(
                        "33",
                        "contains a secret — masked in listings and agent output (--reveal to show)"
                    )
                );
            }
            if let Some(f) = &r.verdict.fallback {
                println!("   {}", paint("2", &format!("({f})")));
            }
        }
        Some(Cmd::Recall {
            query,
            limit,
            here: h,
            reveal,
            json,
        }) => {
            let query = query.join(" ");
            let hits = tb.recall(&query, limit, here(h).as_deref())?;
            if json {
                println!("{}", serde_json::to_string_pretty(&hits)?);
            } else if hits.is_empty() {
                println!("{}", paint("2", "🫥 nothing comes to mind"));
            } else {
                hits.iter().for_each(|h| show(h, reveal));
            }
        }
        Some(Cmd::List {
            limit,
            category,
            here: h,
            reveal,
            json,
        }) => {
            let ms = tb.recent(limit, here(h).as_deref(), category.as_deref())?;
            if json {
                println!("{}", serde_json::to_string_pretty(&ms)?);
            } else if ms.is_empty() {
                println!("🌱 empty brain. Try: tinybrain \"something worth remembering\"");
            } else {
                ms.into_iter().for_each(|memory| {
                    show(
                        &Hit {
                            memory,
                            relevance: None,
                        },
                        reveal,
                    )
                });
            }
        }
        Some(Cmd::Forget { id }) => match tb.forget(id)? {
            true => println!("💨 forgot #{id}"),
            false => return Err(format!("no memory #{id}").into()),
        },
        Some(Cmd::Status) => {
            let b = tb.brain();
            let where_ = if b.is_remote() {
                format!(" → {}", b.url)
            } else {
                String::new()
            };
            println!("🧠 brain: {}{}", paint("36", b.name()), paint("2", &where_));
            let stats = tb.stats()?;
            let total: i64 = stats.iter().map(|s| s.1).sum();
            println!(
                "   {total} memories{}",
                current_project().map_or(String::new(), |p| format!(" · here = {p}"))
            );
            for (c, n) in stats {
                println!("   {} {c:<14}{n}", brain::emoji(&c));
            }
        }
        Some(Cmd::Mcp) => {
            tinybrain::mcp::serve(&tb, std::io::stdin().lock(), std::io::stdout().lock())?
        }
        Some(Cmd::Init) => unreachable!(),
        None => {
            let ms = tb.recent(5, None, None)?;
            if ms.is_empty() {
                println!(
                    "🌱 empty brain. Try: tinybrain \"something worth remembering\"   (tinybrain init to pick a model)"
                );
            } else {
                println!("{}", paint("2", "latest:"));
                ms.into_iter().for_each(|memory| {
                    show(
                        &Hit {
                            memory,
                            relevance: None,
                        },
                        false,
                    )
                });
            }
        }
    }
    Ok(())
}

fn init() -> Result<(), Box<dyn std::error::Error>> {
    let has_key = std::env::var("TYPESAFE_API_KEY").is_ok_and(|k| !k.is_empty());
    let local = std::net::TcpStream::connect_timeout(
        &([127, 0, 0, 1], 8000).into(),
        std::time::Duration::from_millis(300),
    )
    .is_ok();
    let tick = |ok: bool, msg: &str| {
        if ok {
            paint("32", "✓ detected")
        } else {
            paint("2", msg)
        }
    };
    println!("🧠 Which brain should classify your memories?\n");
    println!(
        "  1) strands-decider  local, open weights, nothing leaves your box   {}",
        tick(
            local,
            "start: strands-decider serve StrandsAgents/strands-decider-2B-hobson-v19 --port 8000"
        )
    );
    println!(
        "  2) jev              TypeSafe hosted, most accurate                   {}",
        tick(has_key, "needs TYPESAFE_API_KEY")
    );
    println!("  3) offline          keyword rules, zero setup\n");
    let default = if local {
        "1"
    } else if has_key {
        "2"
    } else {
        "3"
    };
    print!("choice [{default}]: ");
    std::io::stdout().flush()?;
    let mut ans = String::new();
    std::io::stdin().read_line(&mut ans)?;
    let mut cfg = Config::load();
    cfg.brain = Some(
        match ans
            .trim()
            .chars()
            .next()
            .map_or(default.into(), String::from)
            .as_str()
        {
            "1" | "s" => Kind::Strands,
            "2" | "j" => Kind::Jev,
            _ => Kind::Offline,
        },
    );
    if cfg.brain == Some(Kind::Strands) && !local {
        print!("decider URL [{}]: ", brain::STRANDS_URL);
        std::io::stdout().flush()?;
        let mut u = String::new();
        std::io::stdin().read_line(&mut u)?;
        cfg.url = Some(u.trim()).filter(|u| !u.is_empty()).map(String::from);
    }
    cfg.save()?;
    println!(
        "\n{} saved to {}",
        paint("32", "✓"),
        Config::path().display()
    );
    if cfg.brain == Some(Kind::Jev) && !has_key {
        println!(
            "  {}",
            paint(
                "33",
                "export TYPESAFE_API_KEY=… (until then tinybrain runs offline)"
            )
        );
    }
    println!("  try: tinybrain \"found an open S3 bucket acme-backups with db dumps\"");
    Ok(())
}

fn show(h: &Hit, reveal: bool) {
    let m: &Memory = &h.memory;
    let rel = h.relevance.map_or(String::new(), |r| {
        paint("32", &format!("{:>3.0}% ", r * 100.0))
    });
    let meta = format!(
        "{} · {}",
        ago(m.created),
        m.project.as_deref().unwrap_or("-")
    );
    println!(
        "{rel}{} {} {} {} {}",
        paint("1", &format!("#{:<4}", m.id)),
        brain::emoji(&m.category),
        paint("36", &format!("{:<13}", m.category)),
        severity(m.importance),
        paint("2", &meta)
    );
    let text = if reveal { m.text.clone() } else { m.masked() };
    println!(
        "      {}{}",
        if m.secret { "🔒 " } else { "" },
        text.replace('\n', "\n      ")
    );
}

fn severity(i: u8) -> String {
    let color = ["2", "34", "33", "31", "1;31"][i as usize];
    paint(
        color,
        &format!(
            "{}{} {:<8}",
            "●".repeat(i as usize + 1),
            "○".repeat(4 - i as usize),
            LEVELS[i as usize]
        ),
    )
}

fn ago(ts: i64) -> String {
    let s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
        - ts;
    if s < 60 {
        "just now".into()
    } else if s < 3600 {
        format!("{}m ago", s / 60)
    } else if s < 86400 {
        format!("{}h ago", s / 3600)
    } else {
        format!("{}d ago", s / 86400)
    }
}

fn paint(code: &str, s: &str) -> String {
    if std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none() {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}
