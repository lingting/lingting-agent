-- P2 初始 Schema（D-P2-030）：会话、消息与 Run。
--
-- 约定：
--   * 枚举列（role / status）存 serde wire 值（`USER` / `QUEUED` 等），不加 CHECK：
--     P3 追加枚举值时不需要迁移。
--   * `*_at_ms` 用 INTEGER（Unix 毫秒），`*_tokens` / `error` / `model` / `run_id` 可空。
--   * SessionSummary.message_count 用 COUNT(*) 计算（D-P2-005），不落列。
--   * 消息顺序由 seq 保证，UNIQUE(session_id, seq) 让并发写漏洞直接报错。

CREATE TABLE IF NOT EXISTS sessions (
    id            TEXT    PRIMARY KEY,
    title         TEXT    NOT NULL,
    model         TEXT    NOT NULL,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS runs (
    id             TEXT    PRIMARY KEY,
    session_id     TEXT    NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    status         TEXT    NOT NULL,
    model          TEXT    NOT NULL,
    error          TEXT,
    input_tokens   INTEGER,
    output_tokens  INTEGER,
    created_at_ms  INTEGER NOT NULL,
    updated_at_ms  INTEGER NOT NULL,
    finished_at_ms INTEGER
);
CREATE INDEX IF NOT EXISTS idx_runs_session_created ON runs (session_id, created_at_ms);
CREATE INDEX IF NOT EXISTS idx_runs_status ON runs (status);

CREATE TABLE IF NOT EXISTS messages (
    id            TEXT    PRIMARY KEY,
    session_id    TEXT    NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    run_id        TEXT             REFERENCES runs(id) ON DELETE SET NULL,
    role          TEXT    NOT NULL,
    content       TEXT    NOT NULL,
    model         TEXT,
    input_tokens  INTEGER,
    output_tokens INTEGER,
    seq           INTEGER NOT NULL,
    created_at_ms INTEGER NOT NULL,
    UNIQUE (session_id, seq)
);
CREATE INDEX IF NOT EXISTS idx_messages_session_seq ON messages (session_id, seq);
