# tinybrain 🧠

A tiny memory for pentesters and their agents. Type a thought and it gets filed: category, severity, and whether it holds a secret. Ask a question later and it finds the right memory, even when you don't use the same words.

```console
$ tinybrain "jenkins on 10.0.4.12:8080 allows anonymous script console"
🧠 #14 🐛 vulnerability ●●●●● critical · strands 96%

$ tinybrain "svc_backup password=Summer2026! works on dc01"
🧠 #15 🔑 credential ●●●●○ high · strands 98%
   🔒 contains a secret — masked in listings and agent output (--reveal to show)

$ tinybrain f build server groovy rce
 91% #14   🐛 vulnerability ●●●●● critical  2m ago · acme-int
      jenkins on 10.0.4.12:8080 allows anonymous script console
```

A single native binary and a single SQLite file. It doesn't need Docker or a daemon.

## Install

```sh
cargo install --path .        # or: cargo install --git https://github.com/rainmana/rustybrain
tinybrain init                # pick a brain (it detects a running strands-decider / TYPESAFE_API_KEY)
```

## Brains

Memories are classified by a **decision model**, a "System One" model that answers typed questions with calibrated probabilities rather than generating text. Jev and strands-decider use the same `/v1/systemone` protocol, so tinybrain has one client for both.

| brain | where it runs | setup | when to pick it |
|---|---|---|---|
| `strands` | your box | `pip install strands-decider && strands-decider serve StrandsAgents/strands-decider-2B-hobson-v19 --port 8000` | Client data must stay local. It runs on CPU, Apple silicon or CUDA. |
| `jev` | TypeSafe API | `export TYPESAFE_API_KEY=…` | When accuracy matters more and sending text to a third party is acceptable. |
| `offline` | your box | none | Uses keyword rules. It's the default, and also the fallback whenever a remote brain is unreachable. |

Each remember is **one** request that asks three questions:
- `category`: a choice question across finding, vulnerability, credential, recon, technique, exploit, todo, idea, reference and note.
- `importance`: a score question from info to critical.
- `secret`: a noul (yes/no) question.

Each recall is also one request. It asks one noul question per candidate: "is memory [n] relevant to the query?"

Configuration resolves in this order:
1. The env vars `TINYBRAIN_BRAIN` (`jev|strands|offline`) and `TINYBRAIN_URL`.
2. The file `~/.tinybrain/config.toml` (`brain`, `url`, `model`).
3. Auto-detection: `jev` if `TYPESAFE_API_KEY` is set, otherwise `offline`.

`TINYBRAIN_URL` also accepts any System One gateway (OpenRouter, LiteLLM, DigitalOcean…).

## CLI

| command | alias | |
|---|---|---|
| `tinybrain <text…>` / `remember` | `r` | Store a memory. Text can also be piped in: `nmap -sV x \| tinybrain r` |
| `recall <query…>` | `f` | Ranked by meaning. Flags: `--here`, `-n`, `--reveal`, `--json` |
| `list` | `ls` | Newest first. Flags: `-c credential`, `--here`, `--json` |
| `forget <id>` | `rm` | |
| `status` / `init` / `mcp` | | |

The **project** is set automatically to the git repo name (or the folder name) you run the command from, so each engagement folder keeps its own scope. `--here` restricts a command to that project.

To give a client fully separate storage, set `TINYBRAIN_DB=/path/client.db`.

## MCP

```sh
claude mcp add tinybrain -- tinybrain mcp
```

The MCP server exposes four tools: `remember`, `recall`, `recent` and `forget`. Secrets are masked in tool output unless the agent passes `reveal: true`.

## Library

```rust
use tinybrain::{Brain, TinyBrain};

let tb = TinyBrain::open_at("acme.db", Brain::strands("http://localhost:8000/v1/systemone"))?;
// or Brain::jev(key), Brain::offline(), Brain::custom(kind, url, model, key), TinyBrain::open() for user config

let r = tb.remember("SSRF in /render?url= reaches 169.254.169.254", Some("acme"))?;
println!("{} {}", r.memory.category, tinybrain::LEVELS[r.memory.importance as usize]);
if let Some(why) = r.verdict.fallback { eprintln!("decider down, used keywords: {why}"); }

for hit in tb.recall("cloud metadata", 5, Some("acme"))? {
    println!("{:?} {}", hit.relevance, hit.memory.masked()); // masked() before it reaches an LLM
}

// embed the MCP server on any transport
tinybrain::mcp::serve(&tb, reader, writer)?;
```

Library notes:
- `Memory`, `Hit`, `Verdict` and `Remembered` all implement `Serialize`.
- `TinyBrain` is `Send` but not `Sync`, because it holds one SQLite connection. To share it across threads, wrap it in a `Mutex`.
- The only error type is `rusqlite::Error`, because classification can't fail; see the next section.

## Design decisions

- **Classification never blocks a write.** If the brain times out, returns a 4xx/5xx, or replies with an unknown category, tinybrain falls back to the keyword rules and records the reason in `verdict.fallback`. Losing a finding mid-engagement is worse than misfiling it.
- **Secrets never go to the ranker.** Recall sends `masked()` text to the remote brain, so a cleartext credential stays in your SQLite file. Secret detection combines the model's answer with a pattern check using OR, so a model miss on an obvious `AKIA…` or `-----BEGIN` doesn't leave the secret unmasked.
- **Keyword search first, then the model judges.** FTS5 (porter stemming, bm25) finds keyword candidates, and the pool is padded with recent memories so the model can surface matches that share no words with the query. The model then scores at most 12 candidates in one call, which keeps recall at a single round trip and inside strands-decider's 4K context. If the model is down, recall falls back to keyword hits only, with no padding, so you never get confident-looking noise.
- **User input is never FTS syntax.** Every query term is quoted, so `" OR * NEAR(` is searched as literal text rather than interpreted.
- **API keys live in the environment, not the config file.** `config.toml` is safe to dotfile.
- **Local deciders bypass `HTTPS_PROXY`.** ureq ignores `NO_PROXY`, so loopback URLs are never sent through a corporate proxy.
- **Not ported from v3 (yet):** MITRE ATT&CK/NVD/OWASP datasets, sessions, relationships, templates, notifications, REST/dashboard. Those are easy to add later on top of the `category`/`project` model if you miss them.

## Test

```sh
cargo test
```

The tests cover the heuristics, parsing of System One answers, masking, FTS injection, the full remember/recall/forget cycle, and an MCP round trip.
