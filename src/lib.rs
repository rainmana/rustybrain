//! # tinybrain
//!
//! A tiny memory for pentesters and their agents. Memories are classified (category,
//! severity-style importance, contains-a-secret) by Jev, a self-hosted strands-decider,
//! or an offline keyword fallback, and recalled with SQLite FTS5 plus model reranking.
//!
//! ```no_run
//! use tinybrain::{Brain, TinyBrain};
//!
//! let tb = TinyBrain::open_at("engagement.db", Brain::strands("http://localhost:8000/v1/systemone"))?;
//! let m = tb.remember("Jenkins on 10.0.4.12:8080 allows anonymous script console", Some("acme-int"))?;
//! println!("{} {} {}", m.memory.id, m.memory.category, m.memory.importance);
//! for hit in tb.recall("groovy rce", 5, None)? {
//!     println!("{} {:?} {}", hit.memory.id, hit.relevance, hit.memory.masked());
//! }
//! # Ok::<_, tinybrain::Error>(())
//! ```
//!
//! `TinyBrain` is `Send` but not `Sync` (one SQLite connection); wrap it in a `Mutex` to share.
//! Embed the MCP server in your own harness with [`mcp::serve`].

pub mod brain;
pub mod mcp;

pub use brain::{Brain, CATEGORIES, Kind, LEVELS, Verdict};
pub use rusqlite::Error;

use rusqlite::{Connection, Row, params};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug, Serialize)]
pub struct Memory {
    pub id: i64,
    pub text: String,
    pub category: String,
    /// Index into [`LEVELS`] (info..critical).
    pub importance: u8,
    pub secret: bool,
    /// Engagement / project this belongs to.
    pub project: Option<String>,
    /// Unix seconds.
    pub created: i64,
    /// Which brain classified it.
    pub by: String,
}

/// Result of [`TinyBrain::remember`]: the stored memory plus how it was classified.
#[derive(Clone, Debug, Serialize)]
pub struct Remembered {
    pub memory: Memory,
    pub verdict: Verdict,
}

#[derive(Clone, Debug, Serialize)]
pub struct Hit {
    pub memory: Memory,
    /// Model-judged relevance 0..1; `None` when ranked by keywords only.
    pub relevance: Option<f64>,
}

impl Memory {
    /// Text with secret-looking values masked. Use this anywhere output reaches an LLM or a log.
    pub fn masked(&self) -> String {
        if self.secret {
            mask_secrets(&self.text)
        } else {
            self.text.clone()
        }
    }
}

pub struct TinyBrain {
    db: Connection,
    brain: Brain,
}

impl TinyBrain {
    /// Open the default store (`$TINYBRAIN_DB` or `~/.tinybrain/brain.db`) with the configured brain.
    pub fn open() -> Result<Self> {
        let path = std::env::var_os("TINYBRAIN_DB")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join("brain.db"));
        Self::open_at(path, Config::load().brain())
    }

    /// Open (or create) a store at `path`. One file per engagement keeps client data separable.
    pub fn open_at(path: impl AsRef<Path>, brain: Brain) -> Result<Self> {
        if let Some(dir) = path.as_ref().parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        Self::init(Connection::open(path)?, brain)
    }

    /// Throwaway store, handy for tests and ephemeral agents.
    pub fn in_memory(brain: Brain) -> Result<Self> {
        Self::init(Connection::open_in_memory()?, brain)
    }

    fn init(db: Connection, brain: Brain) -> Result<Self> {
        db.execute_batch(
            "pragma journal_mode=wal;
             create table if not exists memories(
               id integer primary key, text text not null, category text not null,
               importance integer not null, secret integer not null, project text,
               created integer not null, brain text not null);
             create virtual table if not exists memories_fts using fts5(text, category, tokenize='porter unicode61');",
        )?;
        Ok(TinyBrain { db, brain })
    }

    pub fn brain(&self) -> &Brain {
        &self.brain
    }

    /// Classify and store. Classification never fails (falls back to keywords), so neither does this
    /// unless the database does.
    pub fn remember(&self, text: &str, project: Option<&str>) -> Result<Remembered> {
        let text = text.trim();
        let v = self.brain.classify(text);
        let tx = self.db.unchecked_transaction()?;
        tx.execute(
            "insert into memories(text,category,importance,secret,project,created,brain) values(?,?,?,?,?,?,?)",
            params![text, v.category, v.importance, v.secret, project, now(), v.by],
        )?;
        let id = tx.last_insert_rowid();
        tx.execute(
            "insert into memories_fts(rowid,text,category) values(?,?,?)",
            params![id, text, v.category],
        )?;
        tx.commit()?;
        Ok(Remembered {
            memory: self.get(id)?.expect("just inserted"),
            verdict: v,
        })
    }

    /// Search by meaning: FTS5 finds keyword candidates, a remote brain reranks them (and recent
    /// memories, so matches with no shared words still surface) and drops the irrelevant ones.
    pub fn recall(&self, query: &str, limit: usize, project: Option<&str>) -> Result<Vec<Hit>> {
        const POOL: usize = 12;
        let mut cands = self.search(query, POOL, project)?;
        if !self.brain.is_remote() {
            cands.truncate(limit);
            return Ok(cands
                .into_iter()
                .map(|memory| Hit {
                    memory,
                    relevance: None,
                })
                .collect());
        }
        let keyword_hits = cands.len();
        for m in self.recent(POOL, project, None)? {
            if cands.len() >= POOL {
                break;
            }
            if !cands.iter().any(|c| c.id == m.id) {
                cands.push(m);
            }
        }
        // the model sees masked text: secrets never leave the box for ranking
        let masked: Vec<String> = cands.iter().map(Memory::masked).collect();
        let pairs: Vec<(i64, &str)> = cands
            .iter()
            .zip(&masked)
            .map(|(m, t)| (m.id, t.as_str()))
            .collect();
        let Some(scores) = self.brain.relevance(query, &pairs) else {
            cands.truncate(limit.min(keyword_hits)); // brain down: keyword hits only, no padding

            return Ok(cands
                .into_iter()
                .map(|memory| Hit {
                    memory,
                    relevance: None,
                })
                .collect());
        };
        let mut hits: Vec<Hit> = cands
            .into_iter()
            .zip(scores)
            .filter(|(_, s)| *s >= 0.25)
            .map(|(memory, s)| Hit {
                memory,
                relevance: Some(s),
            })
            .collect();
        hits.sort_by(|a, b| b.relevance.partial_cmp(&a.relevance).unwrap());
        hits.truncate(limit);
        Ok(hits)
    }

    /// Plain keyword search (FTS5 bm25), no model involved.
    pub fn search(&self, query: &str, limit: usize, project: Option<&str>) -> Result<Vec<Memory>> {
        // quote every term so user input can't inject FTS syntax; OR so partial matches rank
        let q = query
            .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
            .filter(|w| !w.is_empty())
            .map(|w| format!("\"{w}\""))
            .collect::<Vec<_>>()
            .join(" OR ");
        if q.is_empty() {
            return Ok(vec![]);
        }
        let mut st = self.db.prepare(
            "select m.* from memories_fts f join memories m on m.id=f.rowid
             where memories_fts match ?1 and (?2 is null or m.project=?2)
             order by bm25(memories_fts) limit ?3",
        )?;
        let rows = st.query_map(params![q, project, limit as i64], row)?;
        rows.collect()
    }

    /// Newest first, optionally filtered by project and category.
    pub fn recent(
        &self,
        limit: usize,
        project: Option<&str>,
        category: Option<&str>,
    ) -> Result<Vec<Memory>> {
        let mut st = self.db.prepare(
            "select * from memories where (?1 is null or project=?1) and (?2 is null or category=?2)
             order by id desc limit ?3",
        )?;
        let rows = st.query_map(params![project, category, limit as i64], row)?;
        rows.collect()
    }

    pub fn get(&self, id: i64) -> Result<Option<Memory>> {
        let mut st = self.db.prepare("select * from memories where id=?")?;
        st.query_map([id], row)?.next().transpose()
    }

    /// Delete a memory. Returns whether it existed.
    pub fn forget(&self, id: i64) -> Result<bool> {
        self.db
            .execute("delete from memories_fts where rowid=?", [id])?;
        Ok(self.db.execute("delete from memories where id=?", [id])? > 0)
    }

    /// (category, count), most common first.
    pub fn stats(&self) -> Result<Vec<(String, i64)>> {
        let mut st = self
            .db
            .prepare("select category, count(*) n from memories group by 1 order by n desc")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect()
    }
}

fn row(r: &Row) -> rusqlite::Result<Memory> {
    Ok(Memory {
        id: r.get("id")?,
        text: r.get("text")?,
        category: r.get("category")?,
        importance: r.get("importance")?,
        secret: r.get("secret")?,
        project: r.get("project")?,
        created: r.get("created")?,
        by: r.get("brain")?,
    })
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// `~/.tinybrain`
pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join(".tinybrain")
}

/// Current engagement name: the git repo's folder, else the working directory's.
pub fn current_project() -> Option<String> {
    let git = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok();
    let dir = match git {
        Some(o) if o.status.success() => PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()),
        _ => std::env::current_dir().ok()?,
    };
    dir.file_name().map(|n| n.to_string_lossy().into_owned())
}

/// Mask values that look like secrets, keeping enough shape to recognise them.
// ponytail: token-level heuristics; a secret split oddly across words can slip through, so we
// also never send unmasked secret memories to a remote brain during recall
pub fn mask_secrets(text: &str) -> String {
    if let Some(i) = text.find("-----BEGIN") {
        return format!("{}-----BEGIN ••• [key masked]", &text[..i]);
    }
    const KEYS: &[&str] = &[
        "password", "pass", "passwd", "pwd", "token", "secret", "key", "apikey", "api_key",
        "bearer", "hash", "cookie",
    ];
    let mut out = Vec::new();
    let mut mask_next = false;
    for w in text.split(' ') {
        let lw = w.to_lowercase();
        let bare = lw.trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
        let filler = matches!(bare, "" | "is" | "was" | "of" | "for" | "to");
        let kv = w
            .find(['=', ':'])
            .filter(|_| KEYS.iter().any(|k| lw.contains(k)));
        let masked = if mask_next && !filler {
            dots(w)
        } else if let Some(i) = kv {
            format!("{}{}", &w[..=i], dots(&w[i + 1..]))
        } else if ["AKIA", "ghp_", "github_pat_", "sk-", "xox"]
            .iter()
            .any(|p| w.starts_with(p))
        {
            dots(w)
        } else {
            w.to_string()
        };
        // "password hunter2", "password is hunter2", "password: hunter2" all mask the next word
        mask_next = (mask_next && filler)
            || KEYS.contains(&bare) && kv.is_none()
            || kv.is_some_and(|i| i + 1 == w.len());
        out.push(masked);
    }
    out.join(" ")
}

fn dots(s: &str) -> String {
    if s.is_empty() {
        String::new()
    } else {
        format!("{}•••", s.chars().take(2).collect::<String>())
    }
}

/// `~/.tinybrain/config.toml`. API keys stay in the environment, never in this file.
#[derive(Default, Serialize, Deserialize)]
pub struct Config {
    pub brain: Option<Kind>,
    pub url: Option<String>,
    pub model: Option<String>,
}

impl Config {
    pub fn path() -> PathBuf {
        home().join("config.toml")
    }
    pub fn load() -> Self {
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(home())?;
        std::fs::write(
            Self::path(),
            toml::to_string(self).expect("config serializes"),
        )
    }

    /// Resolve the brain. `TINYBRAIN_BRAIN` / `TINYBRAIN_URL` override the file; Jev's key comes
    /// from `TYPESAFE_API_KEY`. With nothing configured, use Jev if a key is set, else offline.
    pub fn brain(&self) -> Brain {
        let env = |k| std::env::var(k).ok().filter(|v: &String| !v.is_empty());
        let key = env("TYPESAFE_API_KEY");
        let kind = env("TINYBRAIN_BRAIN")
            .and_then(|s| toml::Value::String(s).try_into().ok())
            .or(self.brain)
            .unwrap_or(if key.is_some() {
                Kind::Jev
            } else {
                Kind::Offline
            });
        let url = env("TINYBRAIN_URL").or(self.url.clone());
        match kind {
            Kind::Offline => Brain::offline(),
            Kind::Jev if key.is_none() => Brain::offline(), // no key: degrade rather than fail every call
            Kind::Jev => Brain::custom(
                Kind::Jev,
                url.unwrap_or(brain::JEV_URL.into()),
                self.model.clone().unwrap_or("jev-latest".into()),
                key,
            ),
            Kind::Strands => Brain::custom(
                Kind::Strands,
                url.unwrap_or(brain::STRANDS_URL.into()),
                self.model.clone().unwrap_or(brain::STRANDS_MODEL.into()),
                None,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remember_recall_forget() {
        let tb = TinyBrain::in_memory(Brain::offline()).unwrap();
        let r = tb
            .remember(
                "SSRF in /render?url= reaches 169.254.169.254 metadata",
                Some("acme"),
            )
            .unwrap();
        assert_eq!(r.memory.category, "vulnerability");
        tb.remember("bought milk", None).unwrap();
        let hits = tb.recall("metadata ssrf", 5, None).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(tb.recall("metadata", 5, Some("other")).unwrap().len(), 0);
        assert_eq!(tb.search("\" OR * NEAR(", 5, None).unwrap().len(), 0); // FTS injection is inert
        assert!(tb.forget(r.memory.id).unwrap());
        assert!(tb.recall("ssrf", 5, None).unwrap().is_empty());
    }

    #[test]
    fn masking() {
        assert_eq!(
            mask_secrets("svc_backup password=Summer2026! on dc01"),
            "svc_backup password=Su••• on dc01"
        );
        assert_eq!(
            mask_secrets("admin password is hunter22"),
            "admin password is hu•••"
        );
        assert_eq!(
            mask_secrets("key AKIAIOSFODNN7EXAMPLE works"),
            "key AK••• works"
        );
        assert!(
            mask_secrets("id_rsa: -----BEGIN OPENSSH PRIVATE KEY----- abc")
                .ends_with("[key masked]")
        );
    }
}
