//! The classifier: Jev, strands-decider, or an offline keyword fallback.
//!
//! Jev and strands-decider both speak TypeSafe's System One protocol
//! (`POST /v1/systemone`, a `state` plus typed `questions`), so one client covers both.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::time::Duration;

/// Memory categories, tuned for offensive security work: (name, emoji, description for the model).
// ponytail: hardcoded set; make configurable when someone needs a different taxonomy
pub const CATEGORIES: &[(&str, &str, &str)] = &[
    (
        "finding",
        "🔎",
        "A security finding or observation made during testing",
    ),
    (
        "vulnerability",
        "🐛",
        "A specific weakness, bug, misconfiguration, CVE or CWE",
    ),
    (
        "credential",
        "🔑",
        "Credentials, keys, tokens, sessions, or access details",
    ),
    (
        "recon",
        "🛰",
        "Target information: hosts, ports, services, subdomains, people, infrastructure",
    ),
    (
        "technique",
        "🛠",
        "A method, command, payload, tool usage, or TTP",
    ),
    (
        "exploit",
        "💥",
        "A working or attempted exploit, PoC, or attack chain",
    ),
    ("todo", "☐", "Something to test, do, or follow up on"),
    ("idea", "💡", "A hypothesis, idea, or attack plan"),
    (
        "reference",
        "📎",
        "A link, doc, writeup, tool, or fact to look up later",
    ),
    ("note", "📝", "Anything else"),
];

/// Severity-style importance levels; `Verdict::importance` indexes this.
pub const LEVELS: [&str; 5] = ["info", "low", "medium", "high", "critical"];

pub fn emoji(category: &str) -> &'static str {
    CATEGORIES
        .iter()
        .find(|c| c.0 == category)
        .map_or("📝", |c| c.1)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Jev,
    Strands,
    Offline,
}

/// Which model classifies memories and ranks recall results.
#[derive(Clone, Debug)]
pub struct Brain {
    pub kind: Kind,
    pub url: String,
    pub model: String,
    api_key: Option<String>,
}

/// What the brain decided about a piece of text.
#[derive(Clone, Debug, Serialize)]
pub struct Verdict {
    pub category: String,
    /// Model confidence in `category`, 0..1 (keyword fallback reports 0.5 or less).
    pub confidence: f64,
    /// Index into [`LEVELS`].
    pub importance: u8,
    /// Looks like it contains a password, key, token or similar.
    pub secret: bool,
    /// "jev", "strands" or "offline".
    pub by: String,
    /// Set when a remote brain failed and the keyword fallback was used instead.
    pub fallback: Option<String>,
}

pub const JEV_URL: &str = "https://api.typesafe.ai/v1/systemone";
pub const STRANDS_URL: &str = "http://localhost:8000/v1/systemone";
pub const STRANDS_MODEL: &str = "StrandsAgents/strands-decider-2B-hobson-v19";

impl Brain {
    /// Keyword heuristics only. No network, always available.
    pub fn offline() -> Self {
        Brain {
            kind: Kind::Offline,
            url: String::new(),
            model: String::new(),
            api_key: None,
        }
    }
    /// TypeSafe's hosted Jev.
    pub fn jev(api_key: impl Into<String>) -> Self {
        Brain {
            kind: Kind::Jev,
            url: JEV_URL.into(),
            model: "jev-latest".into(),
            api_key: Some(api_key.into()),
        }
    }
    /// A self-hosted strands-decider (`strands-decider serve ... --port 8000`). Nothing leaves your box.
    pub fn strands(url: impl Into<String>) -> Self {
        Brain {
            kind: Kind::Strands,
            url: url.into(),
            model: STRANDS_MODEL.into(),
            api_key: None,
        }
    }
    /// Any System One endpoint (OpenRouter, LiteLLM, a gateway...).
    pub fn custom(
        kind: Kind,
        url: impl Into<String>,
        model: impl Into<String>,
        api_key: Option<String>,
    ) -> Self {
        Brain {
            kind,
            url: url.into(),
            model: model.into(),
            api_key,
        }
    }

    pub fn name(&self) -> &'static str {
        match self.kind {
            Kind::Jev => "jev",
            Kind::Strands => "strands",
            Kind::Offline => "offline",
        }
    }

    pub fn is_remote(&self) -> bool {
        self.kind != Kind::Offline
    }

    /// Classify text. Never fails: a remote error falls back to keywords and says so in `fallback`.
    pub fn classify(&self, text: &str) -> Verdict {
        if !self.is_remote() {
            return heuristic(text);
        }
        let criteria: Map<String, Value> = CATEGORIES
            .iter()
            .map(|(n, _, d)| (n.to_string(), json!(d)))
            .collect();
        let questions = json!({
            "category": {"type": "choice", "instructions": "What kind of security-work memory is this?", "criteria": criteria},
            "importance": {"type": "score", "instructions": "How important is this for a penetration test or security assessment?", "criteria": LEVELS},
            "secret": {"type": "noul", "instructions": "Does this text contain a secret such as a password, API key, token, hash, cookie, or private key?"},
        });
        match self
            .ask(text, questions)
            .and_then(|a| parse_verdict(&a, self.name()))
        {
            // belt and braces: a model miss on an obvious AKIA... key shouldn't leak it unmasked
            Ok(v) => Verdict {
                secret: v.secret || looks_secret(text),
                ..v
            },
            Err(e) => Verdict {
                fallback: Some(e),
                ..heuristic(text)
            },
        }
    }

    /// Relevance (0..1) of each candidate to `query`, in order. `None` if offline or the call failed.
    pub fn relevance(&self, query: &str, candidates: &[(i64, &str)]) -> Option<Vec<f64>> {
        if !self.is_remote() || candidates.is_empty() {
            return None;
        }
        let mut state = format!("Search query: {query}\n\nMemories:\n");
        let mut questions = Map::new();
        for (id, text) in candidates {
            // ponytail: truncate to fit strands-decider's 4K context; chunking if long memories matter
            let short: String = text.chars().take(280).collect();
            state.push_str(&format!("[{id}] {short}\n"));
            questions.insert(
                format!("m{id}"),
                json!({"type": "noul", "instructions": format!("Is memory [{id}] relevant to the search query?")}),
            );
        }
        let answers = self.ask(&state, Value::Object(questions)).ok()?;
        candidates
            .iter()
            .map(|(id, _)| answers[format!("m{id}")]["noul"].as_f64())
            .collect()
    }

    fn ask(&self, state: &str, questions: Value) -> Result<Value, String> {
        // ponytail: ureq ignores NO_PROXY; never proxy loopback so a local decider works behind corp proxies
        let local = ["://localhost", "://127.", "://[::1]"]
            .iter()
            .any(|h| self.url.contains(h));
        let agent = ureq::AgentBuilder::new()
            .try_proxy_from_env(!local)
            .timeout(Duration::from_secs(15))
            .build();
        let mut req = agent.post(&self.url);
        if let Some(k) = &self.api_key {
            req = req.set("Authorization", &format!("Bearer {k}"));
        }
        let body = json!({"model": self.model, "state": state, "questions": questions});
        let resp: Value = match req.send_json(body) {
            Ok(r) => r.into_json().map_err(|e| e.to_string())?,
            Err(ureq::Error::Status(code, r)) => {
                let msg: String = r
                    .into_string()
                    .unwrap_or_default()
                    .chars()
                    .take(200)
                    .collect();
                return Err(format!("{} returned {code}: {msg}", self.name()));
            }
            Err(e) => return Err(format!("{} unreachable: {e}", self.name())),
        };
        Ok(resp["answers"].clone())
    }
}

fn parse_verdict(a: &Value, by: &str) -> Result<Verdict, String> {
    let category = a["category"]["choice"]
        .as_str()
        .ok_or("no category in answer")?;
    if !CATEGORIES.iter().any(|c| c.0 == category) {
        return Err(format!("unknown category {category}"));
    }
    Ok(Verdict {
        category: category.into(),
        confidence: a["category"]["confidence"].as_f64().unwrap_or(0.0),
        importance: a["importance"]["score"]
            .as_f64()
            .unwrap_or(1.0)
            .round()
            .clamp(0.0, 4.0) as u8,
        secret: a["secret"]["noul"].as_f64().unwrap_or(0.0) >= 0.5,
        by: by.into(),
        fallback: None,
    })
}

/// Keyword fallback.
// ponytail: substring heuristics, will misfile anything subtle; point a decider at it for real classification
pub fn heuristic(text: &str) -> Verdict {
    let t = text.to_lowercase();
    const RULES: &[(&str, &[&str])] = &[
        (
            "credential",
            &[
                "password",
                "passwd",
                "creds",
                "api key",
                "apikey",
                "token",
                "ntlm",
                "hash",
                "cookie",
                "ssh key",
                "private key",
            ],
        ),
        (
            "exploit",
            &[
                "exploit",
                "poc",
                "reverse shell",
                "shellcode",
                "rop chain",
                "gadget",
                "popped",
                "got shell",
            ],
        ),
        (
            "vulnerability",
            &[
                "cve-",
                "cwe-",
                "vuln",
                "injection",
                "xss",
                "sqli",
                "rce",
                "lfi",
                "ssrf",
                "idor",
                "overflow",
                "bypass",
                "deserializ",
                "misconfig",
            ],
        ),
        (
            "recon",
            &[
                "subdomain",
                "open port",
                "port ",
                "nmap",
                "masscan",
                "dns",
                "whois",
                "asn",
                "banner",
                "fingerprint",
                "endpoint",
                "host ",
            ],
        ),
        (
            "technique",
            &[
                "payload",
                "how to",
                "command",
                "curl ",
                "burp",
                "ffuf",
                "sqlmap",
                "use ",
                "technique",
                "t1",
            ],
        ),
        (
            "todo",
            &[
                "todo",
                "follow up",
                "remember to",
                "need to",
                "don't forget",
                "check if",
                "retest",
            ],
        ),
        (
            "idea",
            &[
                "idea",
                "what if",
                "maybe",
                "hypothesis",
                "could we",
                "might be",
            ],
        ),
        (
            "reference",
            &[
                "http://", "https://", "writeup", "docs", "paper", "rfc", "blog",
            ],
        ),
        (
            "finding",
            &[
                "found",
                "discovered",
                "observed",
                "noticed",
                "returns",
                "leaks",
                "exposed",
                "disclos",
            ],
        ),
    ];
    let (category, hits) = RULES
        .iter()
        .map(|(c, kws)| (*c, kws.iter().filter(|k| t.contains(*k)).count()))
        .max_by_key(|&(_, h)| h)
        .filter(|&(_, h)| h > 0)
        .unwrap_or(("note", 0));
    let importance = if [
        "critical",
        "rce",
        "domain admin",
        "root",
        "prod",
        "unauth",
        "pre-auth",
    ]
    .iter()
    .any(|k| t.contains(k))
    {
        3
    } else if category == "note" || category == "reference" {
        1
    } else {
        2
    };
    Verdict {
        category: category.into(),
        confidence: (0.2 + 0.15 * hits as f64).min(0.5),
        importance,
        secret: looks_secret(text),
        by: "offline".into(),
        fallback: None,
    }
}

/// Cheap secret sniffing, used offline and to double-check the model.
pub fn looks_secret(text: &str) -> bool {
    let t = text.to_lowercase();
    [
        "-----begin",
        "password=",
        "password:",
        "passwd",
        "pwd=",
        "api_key",
        "apikey",
        "secret=",
        "token=",
        "bearer ",
        "authorization:",
        "aws_secret",
    ]
    .iter()
    .any(|k| t.contains(k))
        || ["AKIA", "ghp_", "github_pat_", "sk-", "xox"]
            .iter()
            .any(|k| text.contains(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heuristics() {
        let v = heuristic("SQLi in /api/login via username param, pre-auth");
        assert_eq!((v.category.as_str(), v.importance), ("vulnerability", 3));
        assert!(heuristic("svc_backup password=Summer2026!").secret);
        assert_eq!(heuristic("lunch was good").category, "note");
    }

    #[test]
    fn parses_systemone_answers() {
        let a = json!({
            "category": {"type": "choice", "choice": "recon", "confidence": 0.93, "probabilities": {}},
            "importance": {"type": "score", "score": 3, "confidence": 0.7, "legend": {}},
            "secret": {"type": "noul", "noul": 0.04}
        });
        let v = parse_verdict(&a, "jev").unwrap();
        assert_eq!(
            (v.category.as_str(), v.importance, v.secret),
            ("recon", 3, false)
        );
        assert!(parse_verdict(&json!({"category": {"choice": "banana"}}), "jev").is_err());
    }
}
