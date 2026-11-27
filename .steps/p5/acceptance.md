# P5 验收清单

**状态：已完成（2026-11-27，10 组 53 条逐条取证）。** 每条都要能实际验证；**任一条不通过，P5 不视为完成**。逐条证据见 [`docs/reports/p5-020-acceptance.md`](../../docs/reports/p5-020-acceptance.md)；未采集项与结转风险（`KI-P5-xxx`）登记在同一文件 §六。

验证方式只有四类：

- **命令**：可直接执行的命令与期望退出码 / 输出特征。
- **用例**：`make test ARGS="skill"` / `ARGS="--all"` 下的 `crates/bin-tests` 用例（用例名即证据）。
- **静态核对**：`cargo tree`、源码检索、`git diff` 限定到本次改动文件、逐字段比对表。
- **人工**：只能人工判定的项，必须写明判定动作。

除「用例」类外，不得以「跑一遍看看」作为证据；**未获明确授权不新增单元测试**（`AGENTS.md` 第 28 条只允许 `bin-tests`），**未获明确授权也不执行任何用例**——未授权的条目在报告中标注「未授权」，不得留空、不得算通过。

证据只允许来自接口响应、客户端事件与 `metrics` 快照；不得读取、解析或检索日志文本（`AGENTS.md` 测试数据采集条款）。

---

## 1. Build（B-1 ~ B-4）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| B-1 | workspace 全量可编译 | `cargo build --workspace` 退出码 0，无 error |
| B-2 | 格式一致 | `cargo fmt --all --check` 退出码 0 |
| B-3 | Clippy 零告警 | `cargo clippy --workspace --all-targets -- -D warnings` 退出码 0 |
| B-4 | SDK 与前端门禁 | `make gen`、`make gen-check`、`make ui-check` 均退出码 0 |

## 2. Architecture（A-1 ~ A-7）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| A-1 | `agent-skill` 依赖方向正确 | `cargo tree -p agent-skill` 只有 `agent-core` / `agent-context-core` / `agent-tool` / `lib-core` / 通用库与框架 crate；**没有** `agent-runtime` / `agent-route` / `rig-core` / `service-core` / `lib-db` / `types-core` / `lib-web*` / `bin-*` |
| A-2 | 新增依赖边只有一条 | `cargo tree -i agent-tool` 的消费者新增且仅新增 `agent-skill`；`git diff` 的 `Cargo.toml` 只含 `serde_yaml_ng` 与成员登记 |
| A-3 | 配置类型不进领域 crate | `grep -rn "types_core" crates/agent-skill/src` 为空；`skills.toml` 只在 `lib-bin-cli::config` 解析 |
| A-4 | 装配层不承载领域逻辑 | `crates/manager-agent/src/skill.rs` 只有「翻译配置 + 发现 + 注册 + 日志 + 门面转发」，无解析、无索引渲染、无章节切分（`grep` 这些关键词为空） |
| A-5 | 运行时改动只有构造签名 | `git diff --stat` 显示 `crates/agent-runtime/` 只有 `src/port.rs` 与 `src/lib.rs`；`src/loop_.rs`、`src/loop_/*` 零改动 |
| A-6 | 未创建被禁止的 Crate | 仓库中不存在 `manager-core` / `types-api` / `agent-skill-tool` / `agent-skills` / `bin-tui` / `bin-web` / `bin-mock-server` |
| A-7 | 结构文档同步 | `docs/workspace.md` 的 `agent-skill` 段（职责、依赖、边界）与 `Cargo.toml` 的 members、`cargo tree` 的事实逐条一致；§8.2 的阶段范围表把 P5 行标为已创建 |

## 3. Discovery & Format（D-1 ~ D-7）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| D-1 | 目录布局唯一 | 用例 `skill.discover`：只认 `<home>/skills/<dir>/SKILL.md`（一层，不递归）；缺 `SKILL.md` 的目录不进清单；目录缺失 / 空目录得空清单且无错误 |
| D-2 | frontmatter 必填项 | 用例 `skill.parse`：缺 `name`、缺 `description`、`name` 不合规三类各有独立文案；`name` 与目录名不一致**不报错** |
| D-3 | 未知字段被忽略 | 用例 `skill.parse`：带 `allowed-tools` / `license` 等未知字段的 `SKILL.md` 解析成功，字段不出现在 `SkillMetadata` 里 |
| D-4 | 错误可定位 | 用例 `skill.parse`：每类错误文案形如 `skills/<dir>/SKILL.md:<行号>: <原因>`，含行号与相对路径，不含绝对路径与正文片段 |
| D-5 | 章节切分可复算 | 用例 `skill.parse`：代码围栏内的 `#` 不生成章节；章节标题、层级、字符数与正文独立复算一致 |
| D-6 | 引用清单安全 | 用例 `skill.parse`：`references/../../config.toml` 不进清单；符号链接不被跟随；超过 200 条时截断且顺序确定 |
| D-7 | 同名冲突不静默 | 用例 `skill.discover`：两个目录声明同一 `name` 时两份都 `INVALID`，`error` 含两个目录；改名后两份都 `READY` |

## 4. Progressive Disclosure & Budget（P-1 ~ P-7）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| P-1 | 索引进系统提示 | 用例 `skill.index`：请求体的 system 消息含 `## 可用 Skill` 小节；有 `AGENTS.md` 时其后恰好一个空行接小节；无 Skill 时 system 消息与 P2 一致（逐字节） |
| P-2 | 正文不默认进上下文 | 用例 `skill.index` / `skill.budget`：未调用 `skill_load` 时，请求体里除索引外**不含** Skill 正文字符串；`run_started.input_tokens` 只随索引变化 |
| P-3 | 按需加载生效 | 用例 `skill.load`：模型请求 `skill_load` 后，该轮工具结果包含正文，且下一轮请求的历史里出现该结果 |
| P-4 | 大正文分级披露 | 用例 `skill.load`：正文超 `max_inline_chars` 且未给 `section` 时返回章节目录（标题 + 字数）；带 `section` 时只返回该节 |
| P-5 | 索引预算 | 用例 `skill.index` / `skill.budget`：索引字符数 ≤ `max_index_chars`（含「另有 N 个」那行），且 `shown + hidden` 等于启用项总数；例外：`max_index_chars` 小到连「另有 N 个」都放不下时渲染为空小节（`shown = hidden = 0`），按「三项全 0」断言，该等式不适用 |
| P-6 | 正文预算 | 用例 `skill.budget`：`skill_load` 返回值 ≤ `max_inline_chars`；执行器 `[tools] max_result_chars` 未触发截断；`max_inline_chars > max_result_chars` 时启动有 `warn` 且服务可用 |
| P-7 | 索引规模不影响正文 | 用例 `skill.budget`：同一 Skill 在「只启用它」与「启用 50 个」两种环境下的 `skill_load` 返回逐字符一致 |

## 5. Tool Collaboration（T-1 ~ T-6）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| T-1 | `skill_load` 是标准工具 | 用例 `skill.load`：出现在请求体 `tools` 字段，`name = skill_load`，schema 的 `required` 只含 `name`，`section` 为可选 |
| T-2 | 只读且并行 | 静态核对：`file_access` 为 `Read`、`risk` 为 `ReadOnly`；用例 `skill.load` 中两个 `skill_load` 并行调用时 `tool_call_started` 时间重叠（未被串行） |
| T-3 | 三种失败都是工具结果 | 用例 `skill.load`：未知技能 / 未启用 / `INVALID` 的调用分别得到可读文本，Run **不** `FAILED`、不挂起，模型可以继续下一轮 |
| T-4 | 缺工具不假装执行 | 用例 `skill.tools_missing`：声明工具不可用时索引行标注缺失清单，`skill_load` 结果含同一清单与「涉及这些工具的步骤必须停止并告知用户」的约束；最终回复不出现「已执行该工具」的表述 |
| T-5 | 工具可用性按现状判定 | 用例 `skill.tools_missing`：运行期注册缺失工具（或改权限为 `allow`）后，下一次索引渲染的缺失标记消失，无需重载 Skill |
| T-6 | 引用材料复用既有工具 | 静态核对：`agent-skill` 只实现一个 `Tool`；用例 `skill.load` 的引用清单路径能被既有 `file_read` 读取（`skills/<dir>/references/<rel>`，相对应用根目录，在默认允许范围内）。基准差别：示例正文里写的 `references/<rel>` 是相对技能目录的写法，不能直接当 `file_read` 入参 |

## 6. Interface（I-1 ~ I-6）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| I-1 | 接口清单 | 静态核对：`crates/lib-web/src/skill/api.rs` 只有 3 个 `#[web_api_*]`；接口总数 32（29 + 3） |
| I-2 | `list` 语义 | 用例 `skill.api`：按 `name` 升序；含禁用与 `INVALID` 项；`INVALID` 带 `error`；字段与 `docs/api/skill.md` 的表一致 |
| I-3 | `get` 语义 | 用例 `skill.api`：返回元数据 + 正文 + 引用清单；未知 `name` 返回 400 `技能不存在`；`INVALID` 与已禁用项返回 **200**（清单字段 + 空正文，原因在 `error` / `enabled`）；400 `技能加载失败：<原因>` 只用于「名字在清单里、正文取不出来」（D-P5-015） |
| I-4 | `update` 语义 | 用例 `skill.api`：`ENABLE` / `DISABLE` 返回动作完成后的整张清单；`RELOAD` 后新增项出现；`RELOAD` 带 `name` 返回 400 `RELOAD 不接受 name`；缺 `name` 返回 400 `技能更新缺少 name` |
| I-5 | 文档与代码一致 | 静态核对：`docs/api/skill.md` 的 path / 字段 / 枚举取值与代码逐项一致；`docs/api/readme.md` 的领域表与总数已更新 |
| I-6 | SDK 覆盖新接口 | `make gen` 产物里出现三条 path 与全部 DTO（`grep` 生成目录）；`make gen-check` 通过 |

## 7. Observability（O-1 ~ O-3）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| O-1 | `metrics` 有 `skills` 段 | 用例 `skill.metrics`（debug + `metrics` 构建）：`GET api/probe/metrics` 的 `skills` 含 `discovered` / `invalid` / `enabled` / `loads` / `load_failures` / `sections_served` / `index_chars` / `reloads` |
| O-2 | 数值可复算 | 用例 `skill.metrics`：`discovered` / `invalid` / `enabled` 与接口清单逐项相等；`index_chars` 与同一时刻索引渲染的字符数相等 |
| O-3 | 未开特性时显式「未采集」 | 未开 `metrics` 的构建快照全 `0`；报告写「未采集：构建未开 `metrics` 特性」，不得留空 |

## 8. Client（C-1 ~ C-4）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| C-1 | TUI 面板可读 | 用例 `skill.tui`：打开后清单含全部 Skill，`INVALID` 项带原因，禁用项有标记 |
| C-2 | TUI 可操作 | 用例 `skill.tui`：`Enter` 切换后与 `api/skill/list` 一致；`r` 重载后新增项出现；详情可读 |
| C-3 | WebUI 抽屉可读可操作 | 用例 `skill.web`：抽屉清单与接口一致、详情渲染正文、开关与重载后立刻反映返回值 |
| C-4 | 客户端不受影响 | 用例 `tui.*` / `web.*` 既有集合通过；聊天、会话切换、工具确认交互无回归 |

## 9. Error & Safety（E-1 ~ E-5）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| E-1 | 坏 Skill 不可能被用上 | 用例 `skill.invalid`：`INVALID` 项不进索引、不出现在请求体、`skill_load` 对其返回失败文案 |
| E-2 | 脚本不因目录获得执行权限 | 用例 `skill.security`：在 Skill 目录放入可执行文件后，进程内无新增工具、无子进程派生、无新增执行路径（工具清单与 `tools` 字段逐项对比） |
| E-3 | 不泄漏绝对路径 | 用例 `skill.security`：索引、工具结果、三条接口响应里检索 `AGENT_HOME` 的绝对路径，0 命中 |
| E-4 | 越权读取不可达 | 用例 `skill.security`：引用清单无 `..`；`skill_load` 的 `section` / `name` 参数无法构造出技能目录之外的读取路径 |
| E-5 | 文案单一产点 | 静态核对：加载失败、缺失工具、接口 400 三类固定文案分别集中在 `agent-skill/src/error.rs`、`availability.rs`、`manager.rs`；全仓无第二处同类拼接 |

## 10. Regression & Closure（R-1 ~ R-4）

| 编号 | 条目 | 验证方式 |
|---|---|---|
| R-1 | P4 用例全绿 | `make test ARGS="mcp"` 通过（MCP 工具注册与权限未变） |
| R-2 | 全量用例全绿 | `make test ARGS="--all"` 通过（需授权） |
| R-3 | 既有语义未变 | 用例 `tool.*` / `run.*` / `persist.*` / `runtime.*` / `client.*` 通过；`RunEventKind` 取值清单与 P4 相同（静态核对，事件 0 新增） |
| R-4 | 阶段收尾 | `docs/reports/p5-020-acceptance.md` 出稿；`.steps/README.md` 与 `.steps/p5/README.md` 的状态行一致；未取证项显式标注原因 |

---

## 口径说明

1. **「用例」类判据的取证前提是执行授权**。未授权时报告里写「未授权」，不得写「通过」。
2. **`metrics` 类判据**只在 `debug` + `metrics` 特性构建下成立；未启用时按「未采集：原因」显式报告。
3. **P-2 的「不含正文」口径**：断言的是「该 Skill 正文的**首个非空行**（≥ 16 字符）不出现在未加载时的请求体里」；索引行里的描述与正文首行相同的极端情况，改用正文中间的独有句子断言。
4. **P-5 / P-6 的字符数**是 `chars()` 计数的 Unicode 标量值个数，不是字节数；索引与正文的包装行（标题、提示语）都计入上限。
5. **A-5 的口径**：`agent-runtime` 的改动只允许「`port.rs` 的构造签名 + `lib.rs` 的 re-export / 文档」；循环、事件、工具执行、Context 组装流程一行不改。
6. **E-2 的判定方式**：不扫描日志，而是对比「Skill 目录存在前后」的工具清单（`api/mcp/tool/list` + 请求体 `tools`）与进程列表（用例内自行记录直接子进程 PID）。
