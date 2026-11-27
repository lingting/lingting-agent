-- P6 6A 迁移 0003（P6A-002）：检查点表、Run 的恢复列与工具执行的开始标记。
--
-- 约定与 0001 / 0002 一致：
--   * 枚举列（phase）存 serde wire 值，不加 CHECK。
--   * 只加列不改既有列：0001 / 0002 的数据保持可读，新列为 NULL（resume_count 为 0）。
--   * `ALTER TABLE ... ADD COLUMN` 没有 IF NOT EXISTS，由迁移 runner 用 PRAGMA table_info 预检，
--     因此本脚本与建表 / 建索引一样是幂等的。
--
-- run_checkpoints 是恢复的唯一进度指针（D-P6-002）：检查点不写事件文件。
-- UNIQUE (run_id, seq_no) 让序号缺口 / 重复写入直接报错，而不是静默覆盖。

CREATE TABLE IF NOT EXISTS run_checkpoints (
    id               TEXT    PRIMARY KEY,
    run_id           TEXT    NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    seq_no           INTEGER NOT NULL,
    phase            TEXT    NOT NULL,
    tool_round       INTEGER NOT NULL,
    last_message_seq INTEGER NOT NULL,
    payload          TEXT    NOT NULL,
    format_version   INTEGER NOT NULL,
    created_at_ms    INTEGER NOT NULL,
    UNIQUE (run_id, seq_no)
);

CREATE INDEX IF NOT EXISTS idx_checkpoints_run ON run_checkpoints (run_id, seq_no);

-- 中断原因只在非预期中断（RUNNING -> PAUSED）时写；resume_count 计成功恢复次数；
-- last_checkpoint_seq 是「最近一条检查点」的指针，与插行在同一事务里更新。
ALTER TABLE runs ADD COLUMN interrupted_reason TEXT;
ALTER TABLE runs ADD COLUMN resume_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE runs ADD COLUMN last_checkpoint_seq INTEGER;

-- 工具执行的开始标记：执行前写 PENDING 时置值，收尾不覆盖；NULL 表示旧库行（执行后写入）。
ALTER TABLE tool_calls ADD COLUMN started_at_ms INTEGER;
