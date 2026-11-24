-- P3 迁移 0002（P3-017）：工具审计表、消息的工具列与 Run 的挂起列（D-P3-003 / D-P3-023）。
--
-- 约定与 0001 一致：
--   * 枚举列（status / file_access）存 serde wire 值，不加 CHECK。
--   * 只加列不改既有列，0001 的数据（无工具）保持可读，新列为 NULL。
--   * `ALTER TABLE ... ADD COLUMN` 没有 IF NOT EXISTS，由迁移 runner 用 PRAGMA table_info 预检，
--     因此本脚本与建表 / 建索引一样是幂等的。

CREATE TABLE IF NOT EXISTS tool_calls (
    id            TEXT    PRIMARY KEY,
    run_id        TEXT    NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    session_id    TEXT    NOT NULL,
    tool_call_id  TEXT    NOT NULL,
    tool_name     TEXT    NOT NULL,
    file_access   TEXT    NOT NULL,
    status        TEXT    NOT NULL,
    arguments     TEXT    NOT NULL,
    result        TEXT,
    duration_ms   INTEGER NOT NULL,
    tool_round    INTEGER NOT NULL,
    created_at_ms INTEGER NOT NULL,
    UNIQUE (run_id, tool_call_id)
);

CREATE INDEX IF NOT EXISTS idx_tool_calls_run ON tool_calls (run_id, created_at_ms);

-- 工具轨迹进对话历史：TOOL 消息靠 tool_call_id 与助手的 tool_calls 配对，tool_calls 只对
-- ASSISTANT 消息有值（存本轮调用清单的 JSON 文本）。
ALTER TABLE messages ADD COLUMN tool_call_id TEXT;
ALTER TABLE messages ADD COLUMN tool_name TEXT;
ALTER TABLE messages ADD COLUMN tool_calls TEXT;

-- 挂起等待确认：pending_tool_call_id 非空表示「这个 Run 在等某个工具调用的确认」，此时
-- status 为 QUEUED 且不占全局并发许可（D-P3-023）。
ALTER TABLE runs ADD COLUMN pending_tool_call_id TEXT;
ALTER TABLE runs ADD COLUMN pending_tool_round INTEGER;
CREATE INDEX IF NOT EXISTS idx_runs_pending ON runs (pending_tool_call_id);
