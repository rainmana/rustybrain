-- rustybrain — initial schema
-- Migration: 001_initial
-- All timestamps are Unix milliseconds (INTEGER).
-- Embeddings are F32_BLOB(1024) — dimension locked at DB init.
-- Vector index created separately after table creation.

-- ─── Sessions ────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS sessions (
    id          TEXT    PRIMARY KEY,
    name        TEXT    NOT NULL,
    description TEXT    NOT NULL DEFAULT '',
    task_type   TEXT    NOT NULL,
    status      TEXT    NOT NULL DEFAULT 'active',
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_sessions_status   ON sessions(status);
CREATE INDEX IF NOT EXISTS idx_sessions_task     ON sessions(task_type);
CREATE INDEX IF NOT EXISTS idx_sessions_created  ON sessions(created_at DESC);

-- ─── MCW State ───────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS mcw_state (
    session_id           TEXT    PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
    health_signal        TEXT    NOT NULL DEFAULT 'nominal',
    active_failure_modes TEXT    NOT NULL DEFAULT '[]',   -- JSON array of failure mode names
    last_repair_at       INTEGER,
    last_sync_at         INTEGER,
    drift_score          REAL    NOT NULL DEFAULT 0.0,
    compression_depth    INTEGER NOT NULL DEFAULT 0,
    constraint_count     INTEGER NOT NULL DEFAULT 0,
    goals                TEXT    NOT NULL DEFAULT '[]',   -- JSON array of IU IDs
    active_constraints   TEXT    NOT NULL DEFAULT '[]',   -- JSON array of IU IDs
    updated_at           INTEGER NOT NULL
);

-- ─── Information Units ───────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS information_units (
    id                     TEXT    PRIMARY KEY,
    session_id             TEXT    NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,

    -- Content
    title                  TEXT    NOT NULL,
    content                TEXT    NOT NULL,
    content_type           TEXT    NOT NULL DEFAULT 'text',

    -- Transfer metadata
    transfer_direction     TEXT    NOT NULL DEFAULT 'unknown',
    source_actor           TEXT,
    target_actor           TEXT,

    -- Jev classification
    classification_status  TEXT    NOT NULL DEFAULT 'pending',
    semantic_type          TEXT,
    priority               INTEGER,
    is_coordination_signal REAL,
    is_repair_attempt      REAL,
    transfer_lossiness     TEXT,
    contains_assumption    REAL,
    contains_pii           REAL,
    mitre_tactic           TEXT,

    -- Human / AI metadata
    tags                   TEXT    NOT NULL DEFAULT '[]',  -- JSON array
    confidence             REAL    NOT NULL DEFAULT 0.5,

    -- Vector embedding (1024-dim F32; NULL until an embedding provider is configured)
    embedding              F32_BLOB(1024),

    -- Lifecycle
    created_at             INTEGER NOT NULL,
    updated_at             INTEGER NOT NULL,
    accessed_at            INTEGER NOT NULL,
    access_count           INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_iu_session    ON information_units(session_id);
CREATE INDEX IF NOT EXISTS idx_iu_type       ON information_units(semantic_type);
CREATE INDEX IF NOT EXISTS idx_iu_priority   ON information_units(priority DESC);
CREATE INDEX IF NOT EXISTS idx_iu_coord      ON information_units(is_coordination_signal DESC);
CREATE INDEX IF NOT EXISTS idx_iu_transfer   ON information_units(transfer_direction);
CREATE INDEX IF NOT EXISTS idx_iu_created    ON information_units(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_iu_classify   ON information_units(classification_status);
CREATE INDEX IF NOT EXISTS idx_iu_tactic     ON information_units(mitre_tactic);

-- DiskANN vector index for approximate nearest-neighbour search.
-- NOTE: libsql_vector_idx is supported on Turso Cloud but NOT in the
-- Limbo embedded engine (turso crate pre-release as of 2026-09).
-- This index will be created automatically when Limbo gains support.
-- For local deployments, vector similarity search falls back to a
-- full table scan using vector_distance_cos(), which is functional
-- but slower on large corpora.
--
-- CREATE INDEX IF NOT EXISTS idx_iu_embedding
--     ON information_units(libsql_vector_idx(embedding, 'metric=cosine', 'compress_neighbors=float8'));

-- Full-text search over title, content, tags
CREATE VIRTUAL TABLE IF NOT EXISTS iu_fts USING fts5(
    title,
    content,
    tags,
    content='information_units',
    content_rowid='rowid'
);

CREATE TRIGGER IF NOT EXISTS iu_fts_insert AFTER INSERT ON information_units BEGIN
    INSERT INTO iu_fts(rowid, title, content, tags)
    VALUES (new.rowid, new.title, new.content, new.tags);
END;

CREATE TRIGGER IF NOT EXISTS iu_fts_delete AFTER DELETE ON information_units BEGIN
    INSERT INTO iu_fts(iu_fts, rowid, title, content, tags)
    VALUES ('delete', old.rowid, old.title, old.content, old.tags);
END;

CREATE TRIGGER IF NOT EXISTS iu_fts_update AFTER UPDATE ON information_units BEGIN
    INSERT INTO iu_fts(iu_fts, rowid, title, content, tags)
    VALUES ('delete', old.rowid, old.title, old.content, old.tags);
    INSERT INTO iu_fts(rowid, title, content, tags)
    VALUES (new.rowid, new.title, new.content, new.tags);
END;

-- ─── IU Edges ────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS iu_edges (
    id           TEXT    PRIMARY KEY,
    source_iu_id TEXT    NOT NULL REFERENCES information_units(id) ON DELETE CASCADE,
    target_iu_id TEXT    NOT NULL REFERENCES information_units(id) ON DELETE CASCADE,
    edge_type    TEXT    NOT NULL,
    strength     REAL    NOT NULL DEFAULT 0.5,
    description  TEXT    NOT NULL DEFAULT '',
    inferred     INTEGER NOT NULL DEFAULT 0,   -- 0 = explicit, 1 = AI-inferred
    created_at   INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_edge_source ON iu_edges(source_iu_id);
CREATE INDEX IF NOT EXISTS idx_edge_target ON iu_edges(target_iu_id);
CREATE INDEX IF NOT EXISTS idx_edge_type   ON iu_edges(edge_type);

-- ─── Repair Log ──────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS repair_log (
    id           TEXT    PRIMARY KEY,
    session_id   TEXT    NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    repair_type  TEXT    NOT NULL,
    failure_mode TEXT,
    description  TEXT    NOT NULL,
    iu_ids       TEXT    NOT NULL DEFAULT '[]',   -- JSON array
    before_state TEXT,   -- JSON snapshot of mcw_state
    after_state  TEXT,   -- JSON snapshot of mcw_state
    initiated_by TEXT    NOT NULL DEFAULT 'human',
    created_at   INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_repair_session ON repair_log(session_id);
CREATE INDEX IF NOT EXISTS idx_repair_type    ON repair_log(repair_type);

-- ─── Classification Queue ────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS classification_queue (
    iu_id        TEXT    PRIMARY KEY REFERENCES information_units(id) ON DELETE CASCADE,
    attempts     INTEGER NOT NULL DEFAULT 0,
    last_attempt INTEGER,
    last_error   TEXT,
    created_at   INTEGER NOT NULL
);
