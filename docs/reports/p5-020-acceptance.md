# P5-020 阶段验收与报告（Skills）

- **阶段**：P5 Skills（6 个批次 / 20 个任务）
- **任务**：P5-020（取证覆盖 Batch A ~ E 的已落地实现、P5-016 ~ P5-019 的 10 条 `skill.*` 用例、P5-019 的 3 份示例 Skill）
- **前置**：Batch A ~ E 已在早前提交落地（`abda7b5` ~ `fa659ec`）；Batch F（P5-016 ~ P5-020）在本轮结案
- **执行日期**：2026-11-27
- **环境**：Windows 11 / Git Bash；CPU 18 核；用例并发度 18（默认 `available_parallelism()`）；`bin-agent` 以 `metrics` 特性构建
- **判据来源**：[`.steps/p5/acceptance.md`](../../.steps/p5/acceptance.md)；任务清单见 [`.steps/p5/tasks.md`](../../.steps/p5/tasks.md)，决策见 [`.steps/p5/decisions.md`](../../.steps/p5/decisions.md)
- **测试授权**：本轮在**明确授权**下执行用例（用户确认「全部授权（新用例 + 全量回归）」）。最终代码的定向执行：`skill`（15/15，`run-1795748223087-35020`）、`tool`（28/28，`run-1795747549000-33376`）、`mcp`（19/19，`run-1795747512113-32940`）、`run`（10/10，`run-1795747557688-35656`）、`client`（2/2，`run-1795747567644-20980`）、`persist`（5/5，`run-1795747604112-27944`）、`runtime`（18/18，`run-1795747606462-17992`）；全量 `make test ARGS="--all"` → **138/138 通过**（`run-1795748180533-26100`）。数字全部来自用例报告与 `metrics` 快照，**不读日志**。
- **本轮改动面**：只动测试设施与新文件——`crates/bin-tests`（10 个新用例文件、Mock 剧本的多轮能力、`util/paths.rs` 的 Skill 目录辅助、`agent-skill` 依赖）、`crates/agent-skill/assets/skills/`（3 份示例 Skill）。**生产代码零改动**（`git status --short` 对 `agent-skill/src`、`agent-core`、`agent-runtime`、`agent-tool`、`manager-agent`、`lib-web`、`lib-bin-*`、`types-core` 全部为空）。

---

## 一、结论

1. **十组判据全部成立**（B / A / D / P / T / I / O / C / E / R），逐条证据见 §二 ~ §五。**没有「未授权」项**；**三项按「未采集」显式记录**（`P-6` 的启动 `warn`、`D-6` / `E-4` 的符号链接跟随、`O-3` 的无特性构建快照），见 §六。
2. **知识不等于执行**：`agent-skill` 只有 `skill_load` 一个 `Tool`（`file_access = Read`、`risk = ReadOnly`）；`skill.security` 证明技能目录里的 `run.sh` / `run.cmd` 不产生任何工具、不产生任何调用（审计为空），请求体里也没有对应工具名。
3. **索引常驻、正文按需**：`skill.index` 的请求体里只有一条 system 消息（`AGENTS.md` + 空行 + 索引小节，共 115 字符），正文里的独有句子一次都不出现；`skill.load` 证明正文只在工具结果里进入后续轮次。
4. **预算口径是数字而不是承诺**：索引 `chars ≤ max_index_chars`（`4000 → 200`、`200 → 200`、`120 → 119`、`60 / 10 → 0` 空小节，不输出半行）；正文 `≤ max_inline_chars`（`6000 / 300 / 120 / 40` 四档都守住）；1 个与 50 个技能场景下 `skill_load` 返回**逐字符一致**（23 字符）；10 倍大正文下 `run_started.input_tokens` 都是 **61**（逐值相等）。
5. **失败可见，不静默降级**：`INVALID` 的 Skill 不进索引、不在请求体、`skill_load` 对其回「技能不可用：broken：skills/broken/SKILL.md:2: frontmatter 缺少 name」，而 `api/skill/list` 与 `api/skill/get` 都带着这条原因出现；`RELOAD` 之后新增 / 改内容 / 删除都在一次动作内生效，两次 `RELOAD` 逐字段相等且 `disabled` 名单保留。
6. **端到端按步骤走**：`skill.e2e` 用 3 份示例 Skill（`pdf-report` / `db-migrate` / `log-triage`）之一跑出「第 1 轮 `skill_load` → 第 2 轮 `file_read`（读的正是正文第一步给出的模板）→ 第 3 轮文本」，`skill_load` 交出的正文里三步按序出现。为支持这一剧本，Mock LLM 的剧本章节增加了**逐轮**形态（`rounds`，见 D-P5-021），旧格式 `calls` 与调用标识 `call_mock_<n>` 逐字不变（`tool.*` 28 个用例复跑全绿）。

---

## 二、Build 与结构（B / A 组）

| 编号 | 判据 | 结论 | 证据 |
|---|---|---|---|
| B-1 | workspace 全量可编译 | 成立 | `cargo build --workspace` 退出码 0 |
| B-2 | 格式一致 | 成立 | `cargo fmt --all --check` 退出码 0 |
| B-3 | Clippy 零告警 | 成立 | `cargo clippy --workspace --all-targets -- -D warnings`：`No issues found` |
| B-4 | SDK 与前端门禁 | 成立 | `make gen`（路由 32 / 接口 32，两份 SDK 内容无变化）、`make gen-check`（「两份 SDK 可从零重建，产物完整且可编译」）、`make ui-check`（`pnpm exec tsc --noEmit`）退出码均为 0 |
| A-1 | `agent-skill` 依赖方向正确 | 成立 | `cargo tree -p agent-skill --depth 1` = `agent-context-core` / `agent-core` / `agent-tool` / `lib-core` / `serde_yaml_ng` / `schemars` / `serde*` / `serde_with` / `specta` / `strum*` / `async-trait` / `tracing` / `framework-proc-*`；**没有** `agent-runtime` / `agent-route` / `rig-core` / `service-core` / `lib-db` / `types-core` / `lib-web*` / `bin-*` |
| A-2 | 新增依赖边只有一条 | 成立 | `cargo tree -i agent-tool` 的消费者里出现 `agent-skill`，且是本阶段唯一新增的；`git status --short` 显示本轮 `Cargo.toml` 只改 `crates/bin-tests`（新增 `agent-skill` 测试依赖），生产 crate 的依赖表零改动 |
| A-3 | 配置类型不进领域 crate | 成立 | `grep -rn "types_core" crates/agent-skill/src` 命中 **0**；`skills.toml` 只在 `lib-bin-cli::config` 解析（`skill.config` 的 14 条非法输入都从该入口抛出） |
| A-4 | 装配层不承载领域逻辑 | 成立 | `crates/manager-agent/src/skill.rs` 只有「配置翻译 + `discover` 调用 + `skill_load` 注册 + 日志 + 门面转发」；`parse` / `render` / `section` / 章节切分在本文件里只出现在**文档注释**与一处日志字段（`sections = entry.sections.len()`），没有解析或渲染实现 |
| A-5 | 运行时改动只有构造签名 | 成立（本轮零改动） | `git status --short crates/agent-runtime` 为空；P5-010 的构造签名改动已随 `9437a64` 落地，`loop_.rs` / `loop_/*` 自那以后无改动 |
| A-6 | 未创建被禁止的 Crate | 成立 | `crates/` 下不存在 `manager-core` / `types-api` / `agent-skill-tool` / `agent-skills` / `bin-tui` / `bin-web` / `bin-mock-server` |
| A-7 | 结构文档同步 | 成立 | `docs/workspace.md` §3.2 `agent-skill` 段的职责 / 依赖 / 边界与 `Cargo.toml`、`cargo tree` 事实一致；§8.2 阶段范围表 P5 行标为已创建；本轮补写一条「示例 Skill 放 `assets/skills/`，不参与编译、不获得执行能力」 |

---

## 三、发现、格式与渐进披露（D / P 组）

### 3.1 发现与格式（D 组）

| 编号 | 判据 | 结论 | 证据 |
|---|---|---|---|
| D-1 | 目录布局唯一 | 成立 | `skill.discover`：`skills/` 缺少、为空、只有无关文件时都是空清单且无错误；二层的 `skills/nested/deeper/SKILL.md` 不被发现；缺 `SKILL.md` 的目录（`skills/plain/notes.md`）不进清单 |
| D-2 | frontmatter 必填项 | 成立 | `skill.parse`：缺 `name` → `skills/demo/SKILL.md:2: frontmatter 缺少 name`；缺 `description` → 同形的第 2 行文案；`name: Demo Skill` → `skills/demo/SKILL.md:2: name 不合规：Demo Skill（规则 ^[a-z0-9][a-z0-9_-]{0,63}$）`；同一份文件放在 `skills/another-directory/` 下解析成功（名字与目录名不一致**不报错**） |
| D-3 | 未知字段被忽略 | 成立 | `skill.parse`：带 `allowed-tools` / `license` / `metadata` 的文件解析成功，`requires_tools = ["file_read", "file_search"]`、`version = "1.2"`、`author = "lingting"`，三个未知字段不出现在 `SkillMetadata` 里 |
| D-4 | 错误可定位 | 成立 | `skill.parse`：六类非法输入（缺 frontmatter / 缺结束分隔符 / YAML 语法错 / 缺 name / 缺 description / name 不合规）各自独立文案，前五类带相对路径 + 行号；文案里没有 `AGENT_HOME` 绝对路径（`skill.discover` 与 `skill.security` 各再断言一次） |
| D-5 | 章节切分可复算 | 成立 | `skill.parse`：围栏内的 `# 这不是标题` 与 `~~~` 里的 `## 也不是标题` 都不生成章节，章节表为 `[("步骤", 1, 6, 76), ("细则", 2, 15, 25), ("收尾", 2, 21, 11)]`；用例用独立的按行扫描复算标题、层级、行号与字符数，逐项相等；父节 `步骤` 的字符数含子节 |
| D-6 | 引用清单安全 | 成立（符号链接部分未采集） | `skill.parse`：引用清单为 `["skills/demo/references/nested/deep.md", "skills/demo/references/notes.md", "skills/demo/references/z-last.md"]`（字典序、相对应用根目录、无 `..`）；205 个文件时截断到 200 条且保留字典序前 200（`item-000` ~ `item-199`）。符号链接未采集：Windows 上创建链接需要权限，用例记为「未采集（符号链接）：客户端没有所需的特权（os error 1314）」，不写成通过 |
| D-7 | 同名冲突不静默 | 成立 | `skill.discover`：`dup-a` / `dup-b` 都声明 `same` 时两份都 `INVALID`，文案同时含两个目录与名字；把 `dup-b` 改名为 `other` 后重新扫描两份都 `READY`，快照为 `[("other", "skills/dup-b"), ("same", "skills/dup-a")]` |

### 3.2 渐进披露与预算（P 组）

| 编号 | 判据 | 结论 | 证据 |
|---|---|---|---|
| P-1 | 索引进系统提示 | 成立 | `skill.index`：请求体里**恰好一条** system 消息，内容逐字符等于 `AGENTS.md` + 一个空行 + `## 可用 Skill` 小节（共 115 字符）；用例按 D-P5-003 的样例独立拼出期望文本，不调用渲染代码 |
| P-2 | 正文不默认进上下文 | 成立 | `skill.index`：正文里的独有句子「正文独有：本技能先读模板再查数据」在请求体里 0 命中；`INVALID` 的 Skill 名 `broken` 同样 0 命中；`skill.load` 证明正文只在 `skill_load` 的工具结果里出现；`skill.budget` 证明 `run_started.input_tokens` 只随索引变化 |
| P-3 | 按需加载生效 | 成立 | `skill.load`：第一轮剧本调用 `skill_load`，审计里该调用的 `status = OK`、`result` 以 `# 技能：alpha` 开头并含正文与引用清单；第二轮请求（`/mock/last-request`）的历史里带着这条工具结果 |
| P-4 | 大正文分级披露 | 成立 | `skill.load`：`max_inline_chars = 120` 时无 `section` 返回章节目录（每行 `- 标题（n 字）`，与解析出的章节表逐项相等）；带 `section: "步骤"` 或 `"## 步骤"` 时只返回该节（`细则` 不出现）；两者都 ≤ 120 字符 |
| P-5 | 索引预算 | 成立 | `skill.index`：`max_index_chars = 160` 时 `shown=2 hidden=4 chars=155`，结尾行是「另有 4 个技能因篇幅未展示」；`skill.budget`：`4000/200 → chars=200 shown=5`、`120 → chars=119 shown=1 hidden=4`、`60/10 → chars=0 shown=0`（连一行都装不下时返回空小节，不输出半行），每条索引行都以完整描述结尾 |
| P-6 | 正文预算 | 成立（启动 `warn` 未采集） | `skill.budget`：`max_inline_chars` 取 `6000 / 300 / 120 / 40` 四档，全文与单节的返回长度分别是 `168/168`、`168/168`、`51/119`、`40/40`，都 ≤ 上限；预算连说明都放不下时返回空串而不是超预算提示。`max_inline_chars > max_result_chars` 的启动 `warn` 未采集：该证据只能从日志读取，按采集条款不得作为数据来源；服务可用性由本轮全部 `self` 用例覆盖 |
| P-7 | 索引规模不影响正文 | 成立 | `skill.budget`：同一 Skill 在「只启用它」与「启用 50 个」两种环境下的 `skill_load` 返回逐字符一致（23 字符） |

---

## 四、工具协作、接口、可观测、客户端（T / I / O / C 组）

### 4.1 工具协作（T 组）

| 编号 | 判据 | 结论 | 证据 |
|---|---|---|---|
| T-1 | `skill_load` 是标准工具 | 成立 | `skill.index`：请求体 `tools` 为 `["calculator", "current_time", "file_read", "file_search", "skill_load"]`；`skill.load` 断言工具自述的 `file_access = Read`、`risk = ReadOnly`；schema 的 `required` 只含 `name`，`section` 为可选（`skill.api` 的 `p5-012` 反向核对同一份 schema） |
| T-2 | 只读且并行 | 成立 | `skill.load`：同一轮的两个 `skill_load` 的 `tool_call_started` 都早于第一个 `tool_call_completed`（事件序号 `2 / 3`），两次调用 `status = OK`，未被串行 |
| T-3 | 三种失败都是工具结果 | 成立 | `skill.load`：未知技能 → `工具执行失败：技能不存在：ghost`；禁用 → `工具执行失败：技能已禁用：many`；`INVALID` → `工具执行失败：技能不可用：broken：skills/broken/SKILL.md:2: frontmatter 缺少 name`。请求级再验一次：`INVALID` 的工具审计 `status = Error`、Run 仍 `RUN_COMPLETED` |
| T-4 | 缺工具不假装执行 | 成立 | `skill.tools_missing`：`db-migrate` 声明 `[calculator, sqlite3]`、只注册 `calculator` 时索引行标注 `（缺少工具：sqlite3）`，`skill_load` 结果含 `## 缺少的工具` + `- sqlite3` +「涉及这些工具的步骤必须停止，并明确告知用户无法执行。」；请求级确认最终回复含「无法执行」、不含「已执行」 |
| T-5 | 工具可用性按现状判定 | 成立 | `skill.tools_missing`：把 `calculator` 重新注册为 `Deny` 后下一次渲染即变成 `（缺少工具：calculator、sqlite3）`，改回 `allow` 后恢复；运行期注册 `current_time` 后 `needs-time` 的缺失标记在下一次渲染消失，全程**没有**重载 Skill |
| T-6 | 引用材料复用既有工具 | 成立 | 静态核对：`agent-skill` 只实现一个 `Tool`（`skill_load`）；`skill.e2e` 的 `file_read` 用 `skills/pdf-report/references/template.md` 读到模板内容（「文件：skills/pdf-report/references/template.md」），说明引用路径在默认允许范围内且不需要新工具 |

### 4.2 接口（I 组）

| 编号 | 判据 | 结论 | 证据 |
|---|---|---|---|
| I-1 | 接口清单 | 成立 | `crates/lib-web/src/skill/api.rs` 恰有 3 个 `#[web_api_*]`（`api/skill/list`、`get`、`update`）；`make gen` 输出「路由 32 条，接口 32 条」 |
| I-2 | `list` 语义 | 成立 | `skill.api`：按 `name` 升序 `["alpha", "beta", "broken"]`，含禁用与 `INVALID` 项，`INVALID` 的 `error` = `skills/broken/SKILL.md:…: frontmatter 缺少 name`；字段与 `docs/api/skill.md` 的表逐项一致 |
| I-3 | `get` 语义 | 成立（口径差异见 KI-P5-001） | `skill.api`：详情返回元数据 + 正文（与 `SKILL.md` 逐字符相等）+ 引用清单（相对路径）；未知 `name` 返回 400 `技能不存在`；`INVALID` 的 `name` 返回 **200 + 空正文 + `error`**（按 D-P5-015 与 `docs/api/skill.md` 的口径），不是 400 `技能加载失败` |
| I-4 | `update` 语义 | 成立 | `skill.api`：`ENABLE` / `DISABLE` 返回动作完成后的整张清单且与紧随其后的 `list` 逐字段相等、连续两次幂等；`RELOAD` 后新增项出现、`disabled` 保留、两次逐字段相等；`RELOAD` 带 `name` → 400 `RELOAD 不接受 name`；缺 `name` → 400 `技能更新缺少 name` |
| I-5 | 文档与代码一致 | 成立 | `docs/api/skill.md` 的 path / 字段 / 枚举取值与 `crates/lib-web/src/skill/` 的 handler 与 `agent-skill/src/vo.rs` 逐项一致；`docs/api/readme.md` 的领域表新增 skill（3 条）、总数 32、`enabled = false` 的空清单口径已写明 |
| I-6 | SDK 覆盖新接口 | 成立 | `make gen` 产物里出现三条 path 与 `SkillVO` / `SkillDetailVO` / `SkillUpdatePO` / `SkillAction`；`make gen-check` 通过（从零重建后仍可编译） |

### 4.3 可观测（O 组）

| 编号 | 判据 | 结论 | 证据 |
|---|---|---|---|
| O-1 | `metrics` 有 `skills` 段 | 成立 | `skill.metrics`（debug + `metrics` 构建）：`GET api/probe/metrics` 的 `skills` 段含八项；本轮实测 `discovered=3 invalid=1 enabled=2 loads=1 load_failures=1 sections_served=1 index_chars=77 reloads=2` |
| O-2 | 数值可复算 | 成立 | `skill.metrics`：状态三项与接口清单逐项相等（3 个 Skill、1 个 `INVALID`、2 个可用）；`index_chars = 77` 与用例独立拼出的索引小节字符数相等；`sections_served = 1` 等于被加载 Skill 的章节数 |
| O-3 | 未开特性时显式「未采集」 | 未采集 | 未开 `metrics` 的构建下 `api/probe/metrics` 不注册（探针只在 debug + `metrics` 注册），用例只能得到「未采集：接口不存在（用 `make test` 构建）」，无法读出「全 0」快照。本轮未单独构建无特性 binary，故记为未采集而不是通过 |

### 4.4 客户端（C 组）

| 编号 | 判据 | 结论 | 证据 |
|---|---|---|---|
| C-1 | TUI 面板可读 | 成立 | `skill.tui`：打开后 `PANEL_TIMEOUT = 3s` 内出清单，`INVALID` 项带原因、禁用项有标记；断连分支单独起一个指向空端口的界面，断言提示行有错误且 `Esc` 仍能关面板 |
| C-2 | TUI 可操作 | 成立 | `skill.tui`：`Enter` 切换后与 `api/skill/list` 逐项一致；`r` 重载后磁盘新写的 `gamma` 出现且 `missingTools = [pdf_render]`；`d` 打开详情 |
| C-3 | WebUI 抽屉可读可操作 | 成立 | `skill.web`（含 `tests/skill-drawer.test.tsx` 的 6 个前端用例）：清单与接口一致、`INVALID` 详情区原样显示 `error`、正文按空行分块每页 8 块在 `max-h-[50vh]` 内滚动；开关与重载用接口返回值写回缓存且 `skillList` 只被调用一次，失败分支断言服务端文案原样出现、开关保持原值 |
| C-4 | 客户端不受影响 | 成立 | 全量 `--all` 里 `tui.*`（14 个，含 `tui.reconnect` 15.1s）与 `web.*`（8 个，含 `web.e2e` / `web.ui` 的 45 个前端用例 + 7 个 Playwright 用例）全部通过 |

---

## 五、错误、安全与回归（E / R 组）

| 编号 | 判据 | 结论 | 证据 |
|---|---|---|---|
| E-1 | 坏 Skill 不可能被用上 | 成立 | `skill.invalid`：`list` 里 `broken` 为 `INVALID` 且带原因；请求体 system 小节只有 `- alpha：第一份技能`，全文没有 `broken`；`skill_load` 对它回「技能不可用：broken：…」且不吐出正文（`什么也不做` 0 命中）；Run 仍 `RUN_COMPLETED` |
| E-2 | 脚本不因目录获得执行权限 | 成立 | `skill.security`：技能目录下的 `scripts/run.sh` / `run.cmd` 存在，但请求体 `tools` 仍是 `["calculator", "current_time", "file_read", "file_search", "skill_load"]`（无任何 `run*` / `scripts*` 名字），一次普通 Run 的审计为空（没有工具被调用） |
| E-3 | 不泄漏绝对路径 | 成立 | `skill.security`：索引小节、`api/skill/list`、`api/skill/get` 与 400 文案里检索 `AGENT_HOME` 绝对路径，0 命中；`skill.discover` 对 `INVALID` 的文案再断言一次 |
| E-4 | 越权读取不可达 | 成立（符号链接部分未采集） | `skill.security`：`name` 取 `../../AGENTS.md` / `/etc/passwd` / `skills/alpha/SKILL.md` 都回「工具执行失败：技能不存在：<原名>」；`section` 取 `../../AGENTS.md` / `/etc/passwd` 都回「技能没有这个章节」（只列技能内的章节名），不产生目录外读取；引用清单无 `..`。符号链接跟随未采集：Windows 缺少创建链接的权限（与 D-6 同一原因） |
| E-5 | 文案单一产点 | 成立 | 加载失败文案集中在 `agent-skill/src/error.rs`（`SkillError::Display`）、缺工具文案集中在 `availability.rs`（`INDEX_MISSING_PREFIX` / `MISSING_HEADING` / `MISSING_CONSTRAINT`）、接口 400 文案集中在 `agent-skill/src/manager.rs`（`SkillManagerError::Display`）；全仓检索这三个文案串，除上述三处只剩文档注释与用例断言，无第二处拼接 |
| R-1 | P4 用例全绿 | 成立 | `mcp` 19/19（`run-1795747512113-32940`）；全量 `--all` 里同样 19/19 |
| R-2 | 全量用例全绿 | 成立 | `make test ARGS="--all"`：**138/138 通过**（`run-1795748180533-26100`） |
| R-3 | 既有语义未变 | 成立 | `tool` 28/28、`run` 10/10、`persist` 5/5、`runtime` 18/18、`client` 2/2 全通过；`RunEventKind` 取值清单未变（`git status --short crates/agent-core` 为空，本阶段没有新增事件类型；`skill.*` 的工具事件复用既有三个 `tool_call_*`） |
| R-4 | 阶段收尾 | 成立 | 本报告出稿；`.steps/README.md` 与 `.steps/p5/README.md` 的状态行同为「已完成（20 个任务 / 6 个批次 / 判据 10 组 53 条）」；未取证项在 §六逐条注明原因 |

---

## 六、未采集项与结转风险

### 6.1 未采集（逐条注明原因，不算通过）

| 编号 | 未采集的对象 | 原因 |
|---|---|---|
| D-6 / E-4 | 符号链接不被跟随的**实测** | Windows 上创建符号链接需要开发者模式或管理员权限，用例创建工作目录外的链接时得到 `os error 1314`（客户端没有所需的特权）。用例把这一项显式记为「未采集（符号链接）：…」并继续断言其余口径；`agent-skill/src/reference.rs` 的实现（`symlink_metadata` + `is_symlink` 跳过）由代码阅读核对，未取得运行期证据 |
| P-6 | `max_inline_chars > max_result_chars` 时的启动 `warn` | 该证据只能从日志读出（`AGENTS.md` 采集条款禁止把日志当数据来源）。服务「照常启动」已由本轮所有 `self` 用例覆盖；`warn` 文案本身未取证 |
| O-3 | 未开 `metrics` 特性的快照「全 0」 | 探针接口只在 debug + `metrics` 构建注册，未开特性时用例只能拿到「接口不存在」，读不出「全 0」快照。本轮未单独构建无特性 binary 取证 |

### 6.2 结转风险

| 编号 | 风险 | 说明与后续动作 |
|---|---|---|
| KI-P5-001 | `I-3` 的口径与实现不一致 | 判据 `I-3` 写「`INVALID` 的 `name` 返回 400 `技能加载失败：<原因>`」，而 D-P5-015 与 `docs/api/skill.md` 定的是「`get` 对 `INVALID` 返回 200 + 空正文 + `error`，400 只用于未知 `name`」。`skill.api` 按实现取证（P5-012 的判据也如此写），`I-3` 的这条措辞待与产品或后续阶段确认后统一 |
| KI-P5-002 | 索引极端预算下 `hidden` 归零 | `max_index_chars` 小到连「另有 N 个」都放不下时，渲染返回空小节（`shown = hidden = 0`），此时 `shown + hidden` 不等于启用项总数——`P-5` 的等式只在「至少展示一行或一行结尾」时成立。用例按「为空则三项全 0」断言，`P-5` 的措辞建议补一句例外 |
| KI-P5-003 | 示例 Skill 的引用路径基准 | `crates/agent-skill/assets/skills/*` 的正文用 `references/<name>` 这类相对技能目录的写法；而 `skill_load` 交出的引用清单是相对**应用根目录**的 `skills/<dir>/references/<name>`。`skill.e2e` 按后者喂 `file_read`。若后续要在正文里直接写可读路径，需要在装配期把根目录差异显示化 |
| KI-P5-004 | 全量跑里一次 `client.post` 的启动失败 | 本轮有一次全量执行中 `client.post` 的隔离服务端启动即退出（exit code 1，`run-1795748101685-24836`），单跑与紧随其后的复跑（`run-1795748142118-33076`、`run-1795748180533-26100`）均通过；判断为隔离用例「`free_port` 取端口到子进程绑定之间」的瞬时端口竞争，与本阶段改动无因果关系。未做进一步定位，留作测试设施的偶发风险 |
