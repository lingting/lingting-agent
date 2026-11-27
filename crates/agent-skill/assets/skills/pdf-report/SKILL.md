---
name: pdf-report
description: 按模板产出 PDF 报表：给定指标与时间范围，产出图表与结论文档。
requires_tools: [file_read, file_search]
version: "1"
author: lingting
---

# 适用场景

需要把一段时间内的指标数据整理成一份可交付的 PDF 报表时使用本技能。

# 步骤

1. 先读模板：`references/template.md`，确认章节顺序与必填字段。
2. 再查数据：用 `file_search` 定位指标文件，用 `file_read` 读取 `references/data.md` 里的字段说明。
3. 最后汇总：按模板的章节顺序写结论，取不到的数据必须写明「未提供」。

# 约束

- 不臆造数字：任何指标都必须来自 `file_read` 读到的文件。
- 模板里的占位符不得原样留在最终文档里。
