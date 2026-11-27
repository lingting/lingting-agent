# 数据字段说明

- `metric`：指标名，与模板里的 `{{metric}}` 一一对应。
- `at_ms`：采样时间（Unix 毫秒）。
- `value`：采样值，缺失时为 `null`，不得当成 0 处理。

数据文件放在指标目录下，文件名形如 `metrics-<name>.jsonl`。
