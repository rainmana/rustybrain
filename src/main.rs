mod app;
mod classifier;
mod db;
mod domain;
mod embedding;
mod mcp;
mod repository;
mod services;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use app::AppState;
use classifier::{Classifier, JevClassifier};
use db::connection::Db;
use embedding::{EmbeddingProvider, OpenAiCompatProvider};

const VERSION: &str = env!("CARGO_PKG_VERSION");

// ─── CLI ─────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "rustybrain",
    version = VERSION,
    about = "Security-and-intelligence-analyst-grade IU coordination substrate with MCW awareness"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Clone)]
enum Command {
    /// Start the MCP server over stdio (default when no subcommand given).
    Serve,
    /// Print version and exit.
    Version,
    /// Check DB and provider connectivity.
    Doctor,
}

// ─── Entry point ─────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    // Initialise tracing. Level controlled by RUST_LOG; default to info.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr) // keep stdout clean for MCP JSON-RPC
        .init();

    let cli = Cli::parse();
    let command = cli.command.clone().unwrap_or(Command::Serve);

    match command {
        Command::Version => {
            println!("rustybrain {VERSION}");
        }
        Command::Serve | Command::Doctor => {
            let state = Arc::new(build_state().await?);

            if matches!(cli.command, Some(Command::Doctor) | None) {
                let result = services::system::health_check(&state).await?;
                println!("{}", serde_json::to_string_pretty(&result)?);
                if matches!(cli.command, Some(Command::Doctor)) {
                    return Ok(());
                }
            }

            info!(version = VERSION, "rustybrain starting");

            // Spawn background classification retry worker.
            {
                let worker_db = state.db.clone();
                let worker_classifier = state.classifier.clone();
                tokio::spawn(services::classification_worker::run(
                    worker_db,
                    worker_classifier,
                ));
            }

            mcp::connection::serve_stdio(state).await?;
        }
    }

    Ok(())
}

// ─── State builder ───────────────────────────────────────────────────────────

async fn build_state() -> Result<AppState> {
    let db_path = resolve_db_path();
    info!(path = %db_path, "database path");

    let db = Db::open(&db_path)
        .await
        .with_context(|| format!("opening database at {db_path}"))?;

    let classifier: Arc<dyn Classifier> = match std::env::var("RUSTYBRAIN_JEV_API_KEY") {
        Ok(key) if !key.is_empty() => {
            info!("Jev classifier configured");
            Arc::new(JevClassifier::new(key))
        }
        _ => {
            warn!("RUSTYBRAIN_JEV_API_KEY not set — classification disabled (pending queue will not process)");
            // Fall through to a no-op placeholder. In tests, MockClassifier is used.
            // In production without a key, all IUs stay in classification_status=pending.
            Arc::new(NoopClassifier)
        }
    };

    let embedder: Option<Arc<dyn EmbeddingProvider>> = build_embedder();

    Ok(AppState {
        version: VERSION,
        db,
        classifier,
        embedder,
    })
}

fn resolve_db_path() -> String {
    if let Ok(p) = std::env::var("RUSTYBRAIN_DB_PATH") {
        return expand_tilde(&p);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let mut path = PathBuf::from(home);
    path.push(".rustybrain");
    path.push("memory.db");
    path.to_string_lossy().into_owned()
}

fn expand_tilde(p: &str) -> String {
    if p.starts_with("~/") || p == "~" {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        p.replacen("~", &home, 1)
    } else {
        p.to_owned()
    }
}

fn build_embedder() -> Option<Arc<dyn EmbeddingProvider>> {
    let provider = std::env::var("RUSTYBRAIN_EMBED_PROVIDER").ok()?;

    match provider.as_str() {
        "openai_compatible" => {
            let base_url = std::env::var("RUSTYBRAIN_EMBED_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".into());
            let api_key = std::env::var("RUSTYBRAIN_EMBED_API_KEY").unwrap_or_default();
            let model = std::env::var("RUSTYBRAIN_EMBED_MODEL")
                .unwrap_or_else(|_| "text-embedding-3-large".into());
            let dims: usize = std::env::var("RUSTYBRAIN_EMBED_DIMENSIONS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1024);

            info!(
                provider = "openai_compatible",
                base_url = %base_url,
                model = %model,
                dims = dims,
                "embedding provider configured"
            );

            Some(Arc::new(OpenAiCompatProvider::new(
                base_url, api_key, model, dims,
            )))
        }
        #[cfg(feature = "local-embed")]
        "local" => {
            // fastembed-rs local provider — implemented post-MVP when
            // the `local-embed` feature flag is enabled.
            warn!("local embedding provider not yet implemented — set RUSTYBRAIN_EMBED_PROVIDER=openai_compatible for Ollama/LM Studio");
            None
        }
        other => {
            warn!(provider = %other, "unknown embedding provider; vector search disabled");
            None
        }
    }
}

// ─── No-op classifier (production fallback when no API key is set) ────────────

struct NoopClassifier;

#[async_trait::async_trait]
impl Classifier for NoopClassifier {
    async fn classify(
        &self,
        _req: classifier::ClassificationRequest,
    ) -> anyhow::Result<crate::domain::classification::ClassificationResult> {
        anyhow::bail!("no classifier configured; set RUSTYBRAIN_JEV_API_KEY")
    }
}
