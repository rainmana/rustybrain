use std::sync::Arc;

use crate::classifier::Classifier;
use crate::db::connection::Db;
use crate::embedding::EmbeddingProvider;

/// Shared application state threaded through every handler.
pub struct AppState {
    /// Version string, set from `CARGO_PKG_VERSION` at build time.
    pub version: &'static str,
    /// Database handle. `Db` is `Clone` — background tasks clone this
    /// and call `new_connection()` to get their own `Connection`.
    pub db: Db,
    /// Jev classifier (or MockClassifier in tests).
    pub classifier: Arc<dyn Classifier>,
    /// Optional embedding provider. `None` disables vector search.
    pub embedder: Option<Arc<dyn EmbeddingProvider>>,
}
