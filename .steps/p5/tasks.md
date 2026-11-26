# P5 任务清单

**状态：进行中（Batch A、Batch B、Batch C 已完成，2026-11-26）。** 20 个任务 / 6 个批次，10 个任务结案。阶段目标与边界见 [README.md](README.md) §一 / §二，判据见 [acceptance.md](acceptance.md)，决策见 [decisions.md](decisions.md)。

批次之间强序，批次内可并行。**每个任务的「测试要求」只允许在 `crates/bin-tests` 落地用例；其他 Crate 不得写 `#[cfg(test)]`**（仓库 `AGENTS.md` 第 28 条）。**所有需要执行用例的步骤都必须先取得明确授权**；未授权时只推进代码、静态检查与文档部分，相关验收项保持未勾选。

本阶段的四条硬线：

1. **知识不等于执行**：Skill 只提供文本（指令、步骤、约束、参考材料清单），不新增任何执行路径；所有动作仍经 P3 / P4 的工具与权限体系。
2. **索引常驻、正文按需**：系统提示里只有「名字 + 描述 + 依赖工具状态」，正文必须由模型显式调用 `skill_load` 才进上下文（D-P5-002 / D-P5-010）。
3. **预算不破**：索引受 `max_index_chars` 约束并计入 `available_tokens`；正文受 `max_inline_chars` 与执行器 `max_result_chars` 双重约束，不存在「一份大 Skill 把窗口塞满」的路径。
4. **失败可见**：格式错误、同名冲突、工具缺失、越权路径都产出可定位的错误（相对路径 + 行号 + 原因），并且**不静默降级**——坏 Skill 既不进索引，也不注册，但一定在 `api/skill/list` 里带着原因出现。

---

## Batch A：Crate、配置与格式（P5-001 ~ P5-003）

> 依赖：无。本批次只造地基：新 crate 可编译、`skills.toml` 能装载校验、一份 `SKILL.md` 能被解析成结构化数据并给出可定位错误。此时还没有发现、索引与工具。
>
> **状态：已完成（2026-11-26）。** 门禁：`cargo build --workspace`、`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings` 全绿；用例 `skill.config` 新增并通过（`make test ARGS="skill.config"`）。`serde_yaml_ng` 实测解析到 0.10.0，`cargo tree -i serde_yaml_ng` 只命中 `agent-skill`。P5-003 的解析口径（D-2 ~ D-6）由 P5-016 的 `skill.parse` 覆盖，本批次**未取证**，相关条目保持未勾选。

- [x] **P5-001 新增 Crate `agent-skill` 与依赖边界**

  **目标**：给 Skill 一个自己的领域 Crate，把「知识」与「执行」的边界从第一行代码起锁死；同时把本阶段唯一一处新增依赖边（`agent-skill → agent-tool`）写进结构文档。
  **前置依赖**：无。
  **涉及文件 / Crate**：`Cargo.toml`（members、`workspace.dependencies` 的 `serde_yaml_ng`）、新增 `crates/agent-skill/{Cargo.toml,src/lib.rs}`、`docs/workspace.md`（§3.2 `agent-skill` 段、§5.1 依赖清单、§8.1.2 之后新增的 P5 依赖边小节、§8.2 阶段范围表）。
  **实施步骤**：
  1. workspace 增加成员与内部依赖；`agent-skill` 的 `[dependencies]` 写 `agent-core` / `agent-context-core` / `agent-tool` / `lib-core` / `serde` / `serde_json` / `schemars` / `serde_yaml_ng` / `async-trait` / `tracing` / `framework-proc-auto` / `framework-proc-core`（`collect`）/ `serde_with` / `specta` / `strum` / `strum_macros`（契约 DTO）。
  2. `serde_yaml_ng` 固定在 workspace 一处（实测可解析到 0.10.0，见 D-P5-017）；成员只写 `workspace = true`。
  3. `lib.rs` 只声明模块与边界注释（照 `agent-mcp/src/lib.rs` 的写法）：说明「本 Crate 只产出知识：元数据、正文、章节、引用清单」，「唯一的工具形态是 `skill_load` 这一个只读检索工具，它不执行 Skill 声明的任何工具」，「不依赖 `types-core`（`skills.toml` 由装配层翻译）」「不依赖 `agent-runtime`」。
  4. 更新 `docs/workspace.md`：`agent-skill` 的职责清单补「章节切分」「索引渲染」「`skill_load` 适配器」，§5.1 依赖清单补 `agent-tool`（并删掉「禁止依赖」列里的 `agent-tool`），边界说明写清「新增这条边只为一个只读检索工具，不是执行 Skill 声明的工具」。
  **验收标准**：
  - [x] `cargo check -p agent-skill` 通过；`cargo tree -p agent-skill` 里没有 `agent-runtime` / `agent-route` / `rig-core` / `service-core` / `lib-db` / `lib-web*` / `bin-*` / `types-core`。
  - [x] `cargo tree -i agent-tool` 的消费者里出现 `agent-skill`，且这是本阶段唯一新增的消费者。
  - [x] `cargo tree -i serde_yaml_ng` 只命中 `agent-skill`。
  - [x] `docs/workspace.md` 的 §8.2 阶段范围表把 P5 行改为「已创建」，`agent-skill` 段的依赖与 `cargo tree` 事实一致。
  **测试要求**：无。检查方式为 `cargo check`、`cargo tree`、`git diff`。

- [x] **P5-002 `skills.toml` 配置类型与装载校验**

  **目标**：Skill 的发现根目录、启用名单与三个字符上限有类型、有缺省值、有启动期硬校验；错误信息能直接指出是哪个字段。
  **前置依赖**：无（与 P5-001 并行）。
  **涉及文件 / Crate**：新增 `crates/types-core/src/config/skills.rs`、`crates/types-core/src/config/mod.rs`、`crates/lib-core/src/directorys.rs`（`skills.toml` 路径）、`crates/lib-bin-cli/src/config.rs`。
  **实施步骤**：
  1. `SkillsConfig { enabled, root, disabled, max_index_chars, max_description_chars, max_skill_bytes, max_inline_chars }` + `Default`；缺省值：`enabled = true`、`root = "skills"`、`disabled = []`、`max_index_chars = 4000`、`max_description_chars = 200`、`max_skill_bytes = 65536`、`max_inline_chars = 6000`。
  2. 配置来自**独立文件** `skills.toml`（应用根目录，与 `config.toml` / `mcp.toml` 同级）：`lib-core` 的 `AppDirectories` 增加 `skills_file`（`SKILLS_CONFIG_FILE = "skills.toml"`），文件内容就是这张表本身，不再套 `[skills]`。
  3. 校验落在 `lib-bin-cli::config`（与 `[tools]` / `mcp.toml` 同一口径）：`root` 不能为空、`max_*` 不能为 `0`、`disabled` 元素非空且满足 Skill 名字规则（`lib-bin-cli` 不得依赖 `agent-skill`，此处按 D-P5-005 的同一正则本地校验一次；两处规则由用例同时锚定）、`max_description_chars` 不得大于 `max_index_chars`。错误文案包含字段名，退出码 2。
  4. `skills.toml` 缺失 = 全部缺省（Skill 功能照常启用）；文件存在但内容为空是硬错误（与 `mcp.toml` 一致）；`parse_skills(text)` 供用例直接喂文本。
  **验收标准**：
  - [x] `skills.toml` 缺失时装载成 `Default`；只写一个字段时其余字段回落到 `Default`。（用例 `skill.config`）
  - [x] 每类非法输入都能给出「字段名 + 原因」的启动期错误：空文件、TOML 语法错、`root` 空串与类型错、四个 `max_*` 为 `0`、`max_skill_bytes` 类型错、`max_description_chars > max_index_chars`、`disabled` 不是数组 / 含空串 / 含非法名字、`enabled` 类型错。（用例 `skill.config` 覆盖 14 条非法输入）
  - [x] `cargo build --workspace` 通过；`config.toml` 与 `mcp.toml` 的既有装载契约不被破坏。
  **测试要求**：新增用例 `skill.config`（`crates/bin-tests/src/cases/skill/config.rs`）覆盖装载与非法输入文案；已授权执行并通过（`make test ARGS="skill.config"`）。

- [x] **P5-003 `SKILL.md` 解析、章节切分与引用扫描**

  **目标**：一份 `SKILL.md` 能被解析成「元数据 + 正文 + 章节表 + 引用清单」，任何不合规都能报到「文件:行号: 原因」。
  **前置依赖**：P5-001。
  **涉及文件 / Crate**：`crates/agent-skill/src/{skill.rs,frontmatter.rs,section.rs,reference.rs,error.rs,lib.rs}`。
  **实施步骤**：
  1. `SkillFile { metadata: SkillMetadata, body: String, sections: Vec<SkillSection>, references: Vec<String> }`；`SkillMetadata { name, description, requires_tools, version, author }`；`SkillSection { title, level, start_line, chars }`。
  2. frontmatter：首行 `---`（容忍 BOM）到下一个独占一行的 `---`；中间用 `serde_yaml_ng` 反序列化到「只认已知字段」的结构——**未知字段忽略**（D-P5-004），`name` / `description` 必填且非空，`name` 满足 `validate_name`（`^[a-z0-9][a-z0-9_-]{0,63}$`），`description` 长度在解析期不截断（只做上限校验口径留到索引渲染）。`requires_tools` 的每个元素都过 `agent_tool::validate_name`。
  3. 正文：frontmatter 之后的部分，`trim` 后必须非空；整文件字节数 ≤ 上限（`max_skill_bytes`，由调用方注入，解析器不读配置），UTF-8 解码失败、缺 frontmatter、缺结束分隔符、YAML 语法错误各自给出独立文案，行号取 YAML 错误位置（`serde_yaml_ng` 的 `location()`）或 frontmatter 起始行。
  4. 章节切分：按行扫描 ATX 标题（`#` ~ `######` + 空格），**代码围栏内**（` ``` ` / `~~~` 起止）的 `#` 不算标题；返回每个标题的行号、层级与字符数（含子节）。
  5. 引用扫描：`references/` 子目录下递归的普通文件（跳过符号链接与目录），按相对路径字典序，上限 200 条；每条产出**相对应用根目录**的路径（`skills/<dir>/references/<rel>`），拒绝任何含 `..` 的路径分量。
  6. 所有错误集中在 `error.rs` 一处构造（`SkillError`），`Display` 形如 `<相对路径>:<行号>: <原因>`，不含绝对路径、不含正文内容。
  **验收标准**：
  - [ ] 合法 `SKILL.md` 解析出全部字段；`未知字段`（如 `allowed-tools`）被忽略且不产生错误。（待 P5-016 的 `skill.parse` 取证）
  - [ ] 六类非法输入各有独立文案：缺 frontmatter、缺结束分隔符、YAML 语法错（带行号）、缺 `name`、缺 `description`、`name` 不合规。`name` 与目录名不一致**不报错**（D-P5-004）。（待 P5-016 的 `skill.parse` 取证）
  - [ ] 章节表与正文逐条对得上：代码围栏内的 `#` 不生成章节；每个章节的字符数可由正文独立复算。（待 P5-016 的 `skill.parse` 取证）
  - [ ] 引用清单：`references/../../config.toml` 这类路径不出现在清单里；符号链接不被跟随；超过 200 条时截断并给出确定性顺序。（待 P5-016 的 `skill.parse` 取证）
  - [x] `cargo clippy -p agent-skill --all-targets -- -D warnings` 通过。
  **测试要求**：解析口径由 P5-016 的 `skill.parse` 用例覆盖；本任务不新增用例。

---

## Batch B：发现、注册与可用性（P5-004 ~ P5-006）

> 依赖：Batch A。本批次结束时，进程内能发现 `skills/` 下的全部 Skill、区分「可用 / 非法」、按名字查找与加载，并知道每个 Skill 声明的工具此刻是否可用。还没有上下文注入、没有工具、没有接口。
>
> **状态：已完成（2026-11-26）。** 门禁：`cargo build --workspace`、`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings` 全绿。本批次 12 条验收项（P5-004 / P5-005 / P5-006 各 4 条）由 P5-016 的 `skill.discover`、P5-017 的 `skill.index` / `skill.load`、P5-019 的 `skill.tools_missing` 取证；用例执行未授权，相关条目保持未勾选。

- [x] **P5-004 Skill Discovery 与快照**

  **目标**：把「目录里有什么」变成一张确定性快照：每个 Skill 一条，坏的也不丢，坏的带原因。
  **前置依赖**：P5-002、P5-003。
  **涉及文件 / Crate**：`crates/agent-skill/src/discover.rs`、`crates/agent-skill/src/registry.rs`（快照类型）、`crates/agent-skill/src/lib.rs`。
  **实施步骤**：
  1. 扫描 `<home>/<root>`（`root` 相对应用根目录，绝对路径按原样）：只认直接子目录下的 `SKILL.md`（大小写不敏感匹配文件名，`.md` 扩展名），一层，不递归找 Skill。
  2. 每条产出 `SkillEntry { name, directory, status, metadata, body_chars, sections, references }`，`status` 为 `READY` 或 `Invalid { error }`；`directory` 是相对应用根目录的路径（`skills/<dir>`）。
  3. 目录缺失 = 零 Skill（不是错误）；`SKILL.md` 缺失的目录**不进清单**（它不是声明过的 Skill，只是一坨文件）；`SKILL.md` 存在但解析失败 → `Invalid`。
  4. 同名冲突（两个目录声明同一个 `name`）：两份都置 `Invalid`，错误文案同时给出两个目录；不做「先到先得」。
  5. 快照按 `name` 升序（`Invalid` 也参与排序，同名的按目录名排序），保证两次扫描结果逐字节可比。
  **验收标准**：
  - 目录缺失、空目录、只有无关子目录时都得到空清单且无错误。（待 P5-016 的 `skill.discover` 取证）
  - 缺 `SKILL.md` 的目录不进清单；`SKILL.md` 内容非法的目录进清单且 `status = INVALID`、`error` 可定位。（待 P5-016 的 `skill.discover` 取证）
  - 同名两份都 `INVALID`，`error` 含两个目录名；改名后重新扫描两份都 `READY`。（待 P5-016 的 `skill.discover` 取证）
  - 两次扫描同一目录，快照完全一致（顺序、字段、错误文案）。（待 P5-016 的 `skill.discover` 取证）
  **测试要求**：由 P5-016 的 `skill.discover` 用例覆盖。

- [x] **P5-005 SkillRegistry：启用状态、索引渲染与加载**

  **目标**：注册表是「哪些 Skill 会让模型看见」的唯一入口：启用状态在内存、索引按预算渲染、加载按需返回正文或章节目录。
  **前置依赖**：P5-004。
  **涉及文件 / Crate**：`crates/agent-skill/src/{registry.rs,index.rs,load.rs}`。
  **实施步骤**：
  1. `SkillRegistry { snapshot: RwLock<Snapshot>, disabled: RwLock<BTreeSet<String>>, config: SkillLimits }`；`SkillLimits` 是 `agent-skill` 自己的上限类型（装配层从 `SkillsConfig` 翻译，本 Crate 不依赖 `types-core`）。
  2. 写路径只有两条：`reload(entries)`（整体替换快照，`Invalid` 条目随快照进来；`disabled` 只保留仍然存在的名字）与 `set_enabled(name, bool)`。读路径（渲染索引、按名查找、出清单）只取读锁，写锁不跨 `await`。
  3. 索引渲染 `index(&dyn ToolAvailability) -> SkillIndex { section: String, shown: usize, hidden: usize, chars: usize }`：只取 `READY && enabled`，每行 `- <name>：<description>[（缺少工具：a、b）]`，描述按 `max_description_chars` 截断并加 `…`；累计字符超过 `max_index_chars` 时按**整行**停止追加，末尾追加一行 `- （另有 <hidden> 个技能因篇幅未展示，可用 skill_load 按名字加载）`；无可用 Skill 时返回空 `section`。
  4. 加载 `load(name) -> Result<SkillContent, SkillError>`：`Ready`/`enabled` 才可加载；返回正文、章节表与引用清单；工具缺失**不阻断**，由调用方把缺失清单拼进返回文本（D-P5-009）。
  5. `entries()` 出清单用（含 `Invalid` 与 `enabled` 标记，按 `name` 升序）；`enabled_count()`、`invalid_count()` 供指标与接口。
  **验收标准**：
  - 索引只含启用项；`Invalid` 项永不出现；行格式与 D-P5-003 的样例逐字符一致。（待 P5-017 的 `skill.index` 取证）
  - 超 `max_index_chars` 时整行截断且 `shown + hidden` 等于启用项总数；`chars` ≤ `max_index_chars`（含结尾那行）。（待 P5-017 的 `skill.index` 取证）
  - `disable` 后索引立即不含该项，`enable` 后立即恢复；两次往返结果一致。（待 P5-017 的 `skill.index` 取证）
  - `load` 对禁用 / 未知 / `INVALID` 三种输入返回三种不同文案；`INVALID` 的文案带着解析期的可定位错误。（待 P5-017 的 `skill.load` 取证）
  **测试要求**：由 P5-017 的 `skill.index` / `skill.load` 覆盖。

- [x] **P5-006 工具可用性判定与降级文案**

  **目标**：「Skill 声明的工具此刻在不在」有一个判定点、一份文案，索引与加载都用它。
  **前置依赖**：P5-005。
  **涉及文件 / Crate**：`crates/agent-skill/src/{availability.rs,index.rs,load.rs}`、`crates/manager-agent/src/{assemble.rs,skill.rs}`。
  **实施步骤**：
  1. `agent-skill` 定义 `trait ToolAvailability { fn available(&self, name: &str) -> bool }`（不依赖 `agent-tool` 的实现细节，只依赖它的名字校验）。
  2. 判定时机是**渲染 / 加载时**，不是发现时：MCP 在 skills 之后才注册工具，发现期判定会漏（D-P5-009）。
  3. 文案单一产点在 `availability.rs`：索引行后缀 `（缺少工具：a、b）`，加载结果末尾追加一节「缺少的工具」+ 一句约束「涉及这些工具的步骤必须停止，并明确告知用户无法执行」。
  4. 装配层（`manager-agent`）提供实现：`struct RegistryAvailability(Arc<ToolRegistry>)`，`available` 走注册表查找；权限为 `deny` 的工具算**不可用**（它不会出现在模型清单里）。
  **验收标准**：
  - 声明两个工具、只注册一个时：索引行标注缺少的那一个；`load` 结果含同样清单与那句约束。（待 P5-019 的 `skill.tools_missing` 取证）
  - 权限 `deny` 的工具算缺失；改成 `allow` 后重新渲染即消失。（待 P5-019 的 `skill.tools_missing` 取证）
  - 运行期注册新工具（MCP 连接）后，下一次渲染的缺失标记自动消失，无需重载 Skill。（待 P5-019 的 `skill.tools_missing` 取证）
  - 文案只存在于 `availability.rs` 一处（全仓检索无第二个同类拼接点）。（静态核对：`agent-skill` 内只有 `availability.rs` 拼接索引后缀与缺失小节）
  **测试要求**：由 P5-019 的 `skill.tools_missing` 用例覆盖。

---

## Batch C：上下文、工具与装配（P5-007 ~ P5-010）

> 依赖：Batch B。本批次结束时，服务端的模型请求里带一段 Skill 索引，模型可以调用 `skill_load` 拿到正文，且预算口径全程成立。本批次不改 Agent Loop 的时序与事件。
>
> **状态：已完成（2026-11-26）。** 门禁：`cargo build --workspace`、`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings` 全绿，`make gen-check` 通过。新增依赖边两条：`agent-context -> agent-skill`（索引进系统提示）、`manager-agent -> agent-context`（`ContextBuilder` 由装配层构造），已同步 `docs/workspace.md` 的 §5.1 / §8.1.3。本批次的请求体级验收项由 P5-017 的 `skill.index` / `skill.load` / `skill.budget` 与 P5-019 的 `skill.tools_missing` 取证；用例执行未授权，相关条目保持未勾选。

- [x] **P5-007 索引注入系统提示**

  **目标**：索引作为系统提示的一部分进入请求，位置固定、可预测、不新增第二套上下文抽象。
  **前置依赖**：P5-005、P5-006。
  **涉及文件 / Crate**：`crates/agent-context/src/{builder.rs,system.rs,lib.rs}`、`crates/agent-context/Cargo.toml`（新增 `agent-skill` 依赖）、`crates/agent-context-core/src/{source.rs,policy.rs}`（注释与本阶段口径对齐）。
  **实施步骤**：
  1. `ContextBuilder` 增加可选 `skills: Option<Arc<SkillIndexSource>>`（`SkillIndexSource` = 注册表 + `ToolAvailability`，定义在 `agent-skill`），构造入口 `with_skills(...)`；未注入时行为与 P2 完全一致。
  2. 合成单条 system 文本：`[AGENTS.md 内容, 索引小节].filter(|s| !s.trim().is_empty()).join("\n\n")`；索引小节标题固定 `## 可用 Skill`，开头一句说明「以下技能提供任务步骤；需要时调用 `skill_load` 获取正文」（具体文案见 D-P5-003）。
  3. 合成后仍是一条 `ContextItem`（`ContextSource::System`、`MessageRole::System`），**不新增 `ContextSource` 取值**（D-P5-003）；`available_tokens(system_tokens)` 自动把索引算进系统开销。
  4. 两段都为空时不发 system 消息（P2 既有行为）；索引渲染在每次 `build` 时现场进行（无缓存），与系统提示词「改完即生效」的口径一致。
  **验收标准**：
  - 只配 AGENTS.md：请求体与 P2 逐字节一致（`git diff` 只含新增分支）。（待 P5-017 的 `skill.index` 取证）
  - 只配 Skill（无 AGENTS.md）：system 消息以 `## 可用 Skill` 开头，且只有一条 system 消息。（待 P5-017 的 `skill.index` 取证）
  - 两者都有：AGENTS.md 内容在前，中间恰一个空行，索引小节在后。（待 P5-017 的 `skill.index` 取证）
  - 无启用 Skill 时不产生空小节、不产生空 system 消息。（待 P5-017 的 `skill.index` 取证）
  - [x] `cargo clippy -p agent-context --all-targets -- -D warnings` 通过。
  **测试要求**：由 P5-017 的 `skill.index` 用例覆盖（断言请求体形态与 system 消息条数）。

- [x] **P5-008 `skill_load` 工具适配器**

  **目标**：模型只要一个只读工具就能拿到任意 Skill 的正文或某一节，结果形态固定、可复算、受 `max_inline_chars` 约束。
  **前置依赖**：P5-005、P5-006。
  **涉及文件 / Crate**：`crates/agent-skill/src/{tool.rs,lib.rs}`。
  **实施步骤**：
  1. 实现 `agent_tool::Tool`：`name = "skill_load"`；`description` 说明「加载技能的完整指令；技能较大时先给章节目录，再用 `section` 取某一节」；`risk = ToolRisk::ReadOnly`；`file_access = ToolFileAccess::Read`（只读应用目录内的 Skill 文件，并发取默认的 `Parallel`）。
  2. 参数 `SkillLoadArgs { name: String, section: Option<String> }`，schema 由 `agent_tool::schema::of::<SkillLoadArgs>()` 派生，`#[serde(deny_unknown_fields)]`。
  3. 结果文本固定三段：首行 `# 技能：<name>`；第二段是正文、章节目录或指定章节；末尾「缺少的工具」节（有缺失时）与「参考资料」节（列出相对路径，提示用 `file_read` 读）。
  4. 篇幅判定（D-P5-010）：`无 section` 时正文总字符 ≤ `max_inline_chars` → 全文；否则 → 章节目录（每节 `标题（n 字）`）+ 一句「请用 section 参数按节读取」。`有 section` 时按标题精确匹配（允许省略 `#` 前缀），只返回该节（含子节），超过 `max_inline_chars` 则截断并标注「本节已截断，剩余 n 字符」。
  5. 未知技能 / 禁用 / `INVALID` / 未知章节分别返回 `ToolError`，`Display` 文案来自 `error.rs` 单一产点；`ToolError` 经执行器映射为模型可读文本，Run 不 `FAILED`。
  **验收标准**：
  - 小正文：一次加载返回全文 + 引用清单；字符数与 `SKILL.md` 正文逐字符相等（除固定包装行）。（待 P5-017 的 `skill.load` 取证）
  - 大正文（超 `max_inline_chars`）：没有 `section` 时返回章节目录，目录里的标题与字符数与解析结果一致；带 `section` 时返回该节正文。（待 P5-017 的 `skill.load` 取证）
  - 未知章节的错误文案列出可用章节名（按正文顺序，最多前 10 个）。（待 P5-017 的 `skill.load` 取证）
  - 工具不派生进程、不写文件：`git diff` 与进程计数可核对；`file_access` 为 `Read`，在只读工具的并行执行里不被串行。（静态部分已核对：本工具不碰文件系统与进程 API；串行与否待 P5-017 取证）
  - [x] `cargo clippy -p agent-skill --all-targets -- -D warnings` 通过。
  **测试要求**：由 P5-017 的 `skill.load` 覆盖。

- [x] **P5-009 预算与截断口径**

  **目标**：把「索引 + 正文」的预算口径落成可断言的数字，任何一条路径都不能把窗口塞满。
  **前置依赖**：P5-007、P5-008。
  **涉及文件 / Crate**：`crates/agent-skill/src/{index.rs,load.rs}`、`crates/agent-context/src/builder.rs`（预算注释与日志）、`crates/agent-tool/src/executor.rs`（核对 `max_result_chars` 的既有截断点，不改逻辑）。
  **实施步骤**：
  1. 索引：字符上限 = `max_index_chars`，超限整行截断（P5-005）；索引字符数进 `tracing::info` 与指标（P5-011）。
  2. 正文：`skill_load` 自身按 `max_inline_chars` 约束；执行器的 `[tools] max_result_chars` 仍是最后一道闸，正常配置下不触发（`6000 < 8000`），装载校验不强制这一关系，但装配期在 `max_inline_chars > max_result_chars` 时记一条 `warn`（配置冲突可见，不阻断启动）。
  3. `run_started.input_tokens` 只随索引增长：正文只作为工具结果出现在**后续轮次**，不算进首次 `input_tokens`——这一点写进 `builder.rs` 与用例口径。
  4. 极端输入：`max_index_chars` 小于单行长度时，最多输出「另有 N 个」这一行（不输出半行）；正文为空、章节全空时返回明确文案而不是空字符串。
  **验收标准**：
  - 索引字符数 ≤ `max_index_chars`（含结尾行）；正文返回值 ≤ `max_inline_chars`（含包装行）。（待 P5-017 的 `skill.budget` 取证）
  - 同一份 Skill 在「只启用它」与「启用 50 个」两种场景下，`skill_load` 的返回逐字符一致（索引规模不影响正文）。（待 P5-017 的 `skill.budget` 取证）
  - 换一份 10 倍大的 Skill，`run_started.input_tokens` 不增长（排除索引变化后逐值相等）；工具结果长度上限不变。（待 P5-017 的 `skill.budget` 取证）
  - `max_inline_chars > max_result_chars` 时启动日志有一条 `warn`，服务照常启动。（`warn` 已在装配期落地；日志取证需授权执行，未取）
  **测试要求**：由 P5-017 的 `skill.budget` 覆盖。

- [x] **P5-010 装配接入与 `agent-runtime` 构造调整**

  **目标**：服务启动时按固定顺序接上 Skill：工具注册表 → Skill 发现 → `skill_load` 注册 → MCP → 运行时（ContextBuilder 带索引）。运行时只多一个构造参数，循环一行不改。
  **前置依赖**：P5-002、P5-005、P5-007、P5-008。
  **涉及文件 / Crate**：新增 `crates/manager-agent/src/skill.rs`、`crates/manager-agent/src/{assemble.rs,holder.rs}`、`crates/agent-runtime/src/{port.rs,lib.rs}`、`crates/lib-bin-cli/src/server.rs`（装配参数透传）。
  **实施步骤**：
  1. `manager-agent::skill`：`SkillManager`（持有 `Arc<SkillRegistry>` 与 `Arc<dyn ToolAvailability>`）+ `build_skills(config, home, tools) -> Result<Arc<SkillManager>>`（发现 + 注册 `skill_load` + 逐条装配日志，日志只记相对目录与状态，不记正文）。
  2. `assemble_with` 顺序：工具注册表（内置）→ `build_skills` → `build_mcp`（MCP 工具注册在 skills 之后，可用性因此天然按现状判定）→ 构造 `ContextBuilder::new(home).with_skills(source)` → `AgentRuntime::new(..., builder)`。
  3. `AgentRuntime::new` 增加 `builder: ContextBuilder` 参数并删除内部 `ContextBuilder::new(home)`；`home` 仍保留（执行器的路径基准）。**这是本阶段 `agent-runtime` 的唯一改动**（D-P5-016）。
  4. `AgentHolder` 增加 `skills: Arc<SkillManager>` 与访问器；`lib-web` 的接口经 `manager_agent::skill_*` 门面取用（P5-012）。
  5. `skills.toml` 的 `enabled = false` 时：不发现、不注册、不注入，等同 P2 行为，且 `api/skill/list` 返回空清单。
  **验收标准**：
  - 启动后一个 Skill 出现在请求体的 system 消息里；`skill_load` 出现在 `tools` 字段里。（待 P5-017 / P5-019 取证）
  - [x] `git diff --stat` 显示 `crates/agent-runtime/` 只有 `port.rs` 的构造签名改动，循环（`loop_.rs`）零改动。
  - `enabled = false` 时 `cargo tree` 不变、请求体与 P2 一致、`skill_load` 不在 `tools` 里。（`cargo tree` 已核对：该分支不新增依赖；请求体差异待 P5-017 取证）
  - [x] `cargo build --workspace` 与 `cargo clippy --workspace --all-targets -- -D warnings` 通过。
  **测试要求**：无新增用例；装配结果由 P5-017 / P5-019 用例间接取证。

---

## Batch D：管理面与可观测（P5-011 ~ P5-013）

> 依赖：Batch C。本批次结束时，Skill 的状态、正文与启停都可以从接口拿到，指标可断言，文档与 SDK 生成同步。
>
> **状态：未开始。**

- [ ] **P5-011 `agent-skill` 指标与 `metrics` 快照段**

  **目标**：把「发现了几个、坏了几个、加载了几次、索引多长」变成 `metrics` 特性下的进程内原子量，经 `api/probe/metrics` 输出。
  **前置依赖**：P5-005、P5-009。
  **涉及文件 / Crate**：新增 `crates/agent-skill/src/metrics.rs`、`crates/manager-agent/src/runtime/metrics.rs`、`crates/agent-skill/Cargo.toml`（`metrics` 特性）。
  **实施步骤**：
  1. `agent-skill`：`SkillMetrics { discovered, invalid, enabled, loads, load_failures, sections_served, index_chars, reloads }`，`#[cfg(feature = "metrics")]` 下用 `AtomicU64` 采集，未开特性时全部空实现、结构体恒为 `Default`（照 `agent-mcp` 的写法）。
  2. 采集点：发现（`discovered` / `invalid`）、启停与重载（`enabled` 取现状、`reloads` 累加）、`skill_load` 成功与失败、索引渲染（`index_chars` 取最近一次，`store` 语义）。
  3. 聚合：`MetricsSnapshot` 增加 `skills: agent_skill::metrics::SkillMetrics`，转写方式与 `mcp` 段一致（现状取 manager，累计量取原子量）。
  4. 特性打通：`manager-agent` / `bin-agent` 的 `metrics` 特性传递到 `agent-skill`（照 `agent-mcp` 的现有写法）。
  **验收标准**：
  - 开 `metrics` 构建下：放入 3 个 Skill（1 个坏）后 `discovered = 3`、`invalid = 1`、`enabled = 2`；一次成功加载与一次失败加载后 `loads` / `load_failures` 各加 1。（用例 `skill.metrics`）
  - `index_chars` 等于同一时刻索引渲染的字符数（用例内独立复算）。
  - 未开 `metrics` 时快照全 `0`，且 `cargo tree` 不新增依赖。
  **测试要求**：由 P5-018 的 `skill.metrics` 覆盖；未开特性时按「未采集：原因」显式报告。

- [ ] **P5-012 三条管理接口与契约类型**

  **目标**：Skill 清单、详情、启停 / 重载各有唯一入口，契约类型只有一份（`agent-skill` 定义、装配层转发、接口层只做解包与错误映射）。
  **前置依赖**：P5-005、P5-010、P5-011。
  **涉及文件 / Crate**：新增 `crates/agent-skill/src/{vo.rs,manager.rs}`、`crates/manager-agent/src/skill.rs`（门面自由函数）、新增 `crates/lib-web/src/skill/{mod.rs,api.rs,dto.rs}`、`crates/lib-web/src/lib.rs`（模块注册）。
  **实施步骤**：
  1. 契约类型（`#[auto_type]`）：`SkillVO { name, description, status, enabled, requiresTools, missingTools, directory, referenceCount, error }`、`SkillDetailVO`（在 `SkillVO` 上补 `body` 与 `references`）、`SkillGetParams { name }`（`list` 不带查询参数，与 `mcp_server_list` 同形）、`SkillUpdatePO { name: Option<String>, action: SkillAction }`、`SkillAction { ENABLE, DISABLE, RELOAD }`。
  2. 错误类型 `SkillManagerError { NotFound, LoadFailed, MissingName, ReloadWithName }`，`Display` 即对外 400 文案（`技能不存在` / `技能加载失败：<原因>` / `技能更新缺少 name` / `RELOAD 不接受 name`），一处产点。
  3. 三条 handler：`GET api/skill/list`（按 `name` 升序，含 `INVALID`）、`GET api/skill/get`（查询参数 `name`）、`POST api/skill/update`（返回**动作完成后**的整张清单 `Vec<SkillVO>`）。
  4. `api_error` 映射与 MCP 同形：`SkillManagerError` → 400 + `Display`，其余 → 500；`lib-web/src/skill/mod.rs` 是唯一映射点。
  5. 未装配 / `skills.toml` 的 `enabled = false` 时 `list` 返回空清单，`get` / `update` 对任何 `name` 返回 400 `技能不存在`（不是 404）。
  **验收标准**：
  - 三条接口的 path、方法、字段与 `docs/api/skill.md` 逐项一致；`lib-web/src/skill/api.rs` 恰有 3 个 `#[web_api_*]`；接口总数 29 → 32。
  - `list` 含禁用与 `INVALID` 项，`INVALID` 带 `error`；`get` 对未知名字返回 400 `技能不存在`；`RELOAD` 带 `name` 返回 400 `RELOAD 不接受 name`；`ENABLE`/`DISABLE` 缺 `name` 返回 400 `技能更新缺少 name`。
  - `update` 的返回值与紧随其后的 `list` 逐字段相等（无需二次查询）。
  - `cargo clippy -p lib-web --all-targets -- -D warnings` 通过。
  **测试要求**：由 P5-018 的 `skill.api` 覆盖。

- [ ] **P5-013 接口文档与生成门禁**

  **目标**：新增领域文档与总数登记，两份 SDK 都能生成出这三条接口与全部 DTO。
  **前置依赖**：P5-012。
  **涉及文件 / 文档**：新增 `docs/api/skill.md`、`docs/api/readme.md`（领域表 + 总数 32）。
  **实施步骤**：
  1. `docs/api/skill.md`：领域说明、三条接口的 path / 方法 / 参数表 / 返回表 / 样例、错误码与固定文案表（沿用 `mcp.md` 的结构）。
  2. `docs/api/readme.md`：领域表新增一行（skill，共 3 条），总数 `29` → `32`，并写清 `enabled = false` 时的空清单口径。
  3. `make gen` 重新生成 TS / Rust 两份 SDK，核对三条 path 与全部 DTO 出现在产物里。
  **验收标准**：
  - `make gen`、`make gen-check` 退出码 0；生成目录里 `grep` 到三条 path 与 `SkillVO` / `SkillDetailVO` / `SkillUpdatePO`。
  - `docs/api/skill.md` 的字段名、类型、枚举取值与代码逐项一致；`docs/api/readme.md` 的总数与 `lib-web` 的 handler 数一致。
  - `make ui-check` 退出码 0（TS 侧类型可用）。
  **测试要求**：无（生成与静态检查）。

---

## Batch E：客户端（P5-014 ~ P5-015）

> 依赖：Batch D（需要 SDK 与接口）。TUI 与 WebUI 可并行。
>
> **状态：未开始。**

- [ ] **P5-014 TUI：Skill 面板与启停**

  **目标**：在 TUI 里能看到有哪些 Skill、哪个坏了、哪个缺工具，并能启停与重载。
  **前置依赖**：P5-013。
  **涉及文件 / Crate**：`crates/lib-bin-tui/src/{state.rs,ui.rs,worker.rs,event.rs}`、`crates/lib-bin-cli/src/sdk.rs`（若无对应方法则补转发）。
  **实施步骤**：
  1. 键位与入口：`/skills` 命令（或 `F2`）打开覆盖层，`Esc` 关闭；打开时拉一次 `api/skill/list`（失败只显示一行可读错误，不阻塞聊天）。
  2. 列表列：名字、状态（`READY` / `INVALID` / `禁用`）、缺失工具数、目录；`INVALID` 行显示错误文案（可截断，`Enter` 展开）。
  3. 操作：`Enter` 切换启用（`ENABLE` / `DISABLE`），`r` 触发 `RELOAD`，操作后用返回的清单就地刷新，不再二次请求。
  4. 详情：`d` 打开 `api/skill/get` 的正文（只读滚动，复用既有消息渲染），`Esc` 返回列表。
  **验收标准**：
  - 面板打开后 3 秒内出清单（本地服务），`INVALID` 项带原因可见；断连时显示一行错误并保持可关闭。
  - `Enter` 切换后清单与 `api/skill/list` 一致；`r` 后新增的 Skill 出现在清单里。
  - 面板不影响既有聊天、会话切换与确认交互（既有 TUI 用例不回归）。
  **测试要求**：由 P5-019 的 `skill.tui` 用例覆盖（`testing` 特性下的既有 TUI 用例风格）。

- [ ] **P5-015 WebUI：Skill 抽屉与详情**

  **目标**：WebUI 里能查看 Skill 清单、正文与启停状态，不打断对话。
  **前置依赖**：P5-013。
  **涉及文件 / 目录**：`lingting-agent-ui/src/pages/chat/`（新增 `SkillDrawer.tsx` 等）、`lingting-agent-ui/src/api/`（TanStack Query hooks）。
  **实施步骤**：
  1. 聊天页头部加「技能」按钮（带数量徽标），点开右侧抽屉（antd `Drawer`），默认全宽 480。
  2. 列表：名字 + 状态标签 + 缺失工具标签；禁用项置灰但可点。详情区渲染 `body` Markdown（复用 `@ant-design/x-markdown`），引用清单显示为可点击的相对路径文本（不自动打开）。
  3. 操作：每行一个启用开关（`POST api/skill/update`），抽屉右上角「重载」按钮（`RELOAD`），成功后用返回值刷新缓存（不额外请求）。
  4. 错误：`INVALID` 项在详情区显示 `error`；接口 400 的错误文案原样展示，不做二次翻译。
  **验收标准**：
  - 抽屉打开后清单与接口一致；`INVALID` 项详情区显示可定位错误；大正文可滚动且不卡顿（虚拟化或分块渲染至少一项落地）。
  - 开关与重载后列表立刻反映接口返回值；失败时提示且状态不变。
  - `make ui-check`、`make ui-lint` 退出码 0；既有聊天页面不受影响。
  **测试要求**：由 P5-019 的 `skill.web` 用例覆盖（既有 web 用例风格）。

---

## Batch F：用例、回归与阶段验收（P5-016 ~ P5-020）

> 依赖：Batch A ~ E。用例只写在 `crates/bin-tests/src/cases/skill/`（`AGENTS.md` 第 28 条）。**执行任何用例前必须取得明确授权**；未授权的条目在验收报告里写「未授权」，不得写「通过」。

- [ ] **P5-016 用例：配置、解析与发现**

  **目标**：把 Batch A / B 的口径逐条钉成可执行用例。
  **涉及文件**：新增 `crates/bin-tests/src/cases/skill/{mod.rs,config.rs,parse.rs,discover.rs}`、`crates/bin-tests/src/cases/mod.rs`（登记）、`crates/bin-tests/src/util/paths.rs`（Skill 目录布局的临时 HOME 辅助）。
  **实施步骤**：
  1. `skill.config`：装载与 12 条非法输入（P5-002 验收标准逐条）。
  2. `skill.parse`：六类解析错误与合法样例（含未知字段被忽略、名字与目录不一致、代码围栏内的 `#`、引用路径越权、符号链接）。
  3. `skill.discover`：目录缺失 / 空目录 / 缺 `SKILL.md` / `INVALID` / 同名冲突 / 两次扫描一致。
  **验收标准**：三个用例各自断言与 P5-002 / P5-003 / P5-004 的验收标准一一对应；用例名可单独指定（`make test ARGS="skill.parse"`）。
  **测试要求**：本次落地三个用例；执行需授权。

- [ ] **P5-017 用例：索引、加载与预算**

  **目标**：证明「索引常驻、正文按需、预算不破」不是文档承诺。
  **涉及文件**：新增 `crates/bin-tests/src/cases/skill/{index.rs,load.rs,budget.rs}`。
  **实施步骤**：
  1. `skill.index`：请求体里 system 消息的形态（P5-007 验收标准逐条）、禁用项不出现在索引、缺工具标注、超 `max_index_chars` 的整行截断与「另有 N 个」。
  2. `skill.load`：全文 / 章节目录 / 指定章节 / 未知章节 / 未知技能 / 禁用 / `INVALID` / `INVALID` 的 `skill_load` 工具执行路径（Mock LLM 第一轮请求该工具）。
  3. `skill.budget`：`max_index_chars` 与 `max_inline_chars` 的边界、同一 Skill 在 1 个与 50 个 Skill 场景下返回一致、10 倍大正文不改变 `run_started.input_tokens`、执行器 `max_result_chars` 未触发。
  **验收标准**：断言值来自接口响应、SSE 事件与 `metrics` 快照（`AGENTS.md` 测试数据采集条款），不读日志。
  **测试要求**：本次落地三个用例；执行需授权。

- [ ] **P5-018 用例：格式拒绝、重载、接口、安全与指标**

  **目标**：把「坏 Skill 不可能被悄悄用上」「重载即生效」「接口语义」「越权不可达」「指标可复算」钉住。
  **涉及文件**：新增 `crates/bin-tests/src/cases/skill/{invalid.rs,reload.rs,api.rs,security.rs,metrics.rs}`。
  **实施步骤**：
  1. `skill.invalid`：`INVALID` 的 Skill 不进索引、`skill_load` 不可用、不在请求体里；`api/skill/list` 带原因可见。
  2. `skill.reload`：新增 / 删除 / 改内容后用 `RELOAD` 生效，无需重启；两次 `RELOAD` 幂等；`disabled` 名单在重载后保留。
  3. `skill.api`：三条接口语义与错误文案（P5-012 验收标准逐条）。
  4. `skill.security`：引用清单无 `..`；技能目录下的可执行文件不产生任何执行路径（无工具注册、无子进程）；索引 / 接口 / 事件里不出现绝对路径。
  5. `skill.metrics`：`metrics` 特性下的八项数值（`discovered` / `invalid` / `enabled` / `loads` / `load_failures` / `sections_served` / `index_chars` / `reloads`）。
  **验收标准**：每条判据都能在 `docs/reports/p5-020-acceptance.md` 里指名到用例与断言；未开 `metrics` 特性的构建按「未采集：原因」报告。
  **测试要求**：本次落地五个用例；执行需授权。

- [ ] **P5-019 示例 Skill 与端到端用例**

  **目标**：用 2–3 个真实示例证明「模型的下一步确实按 Skill 的步骤走」，并覆盖客户端与工具缺失两条路径。
  **涉及文件**：新增 `crates/agent-skill/assets/skills/{pdf-report,db-migrate,log-triage}/SKILL.md`（含 `references/` 与一份长正文）、新增 `crates/bin-tests/src/cases/skill/{e2e.rs,tools_missing.rs,tui.rs,web.rs}`。
  **实施步骤**：
  1. 示例 Skill 三份：`pdf-report`（声明 `file_read` / `file_search`，正文给出「先读模板 → 再查数据 → 最后汇总」三步）、`db-migrate`（声明一个内置工具有意缺失的名字，用于降级路径）、`log-triage`（正文章节多且长，用于 `section` 路径）。
  2. `skill.e2e`：Mock LLM 剧本按「先请求 `skill_load`，再按正文步骤请求 `file_read`，最后一轮文本」走完；断言最终助手回复体现步骤，且工具调用顺序与正文步骤一致。
  3. `skill.tools_missing`：声明工具不可用时，索引标注 + `skill_load` 结果含缺失清单与那句约束；模型在此之后的回复里不出现「已执行该工具」的表述。
  4. `skill.tui` / `skill.web`：打开面板 / 抽屉、切换启用、重载、读取详情四条路径。
  **验收标准**：示例 Skill 三份都在 `assets/skills/` 下且能原样被解析器接受；端到端用例的两个关键断言（步骤顺序、降级表述）可核对。
  **测试要求**：本次落地四个用例；执行需授权。

- [ ] **P5-020 全量回归与阶段验收报告**

  **目标**：确认本阶段没有破坏 P1 ~ P4 的既有语义，并按判据逐条出稿。
  **涉及文件**：新增 `docs/reports/p5-020-acceptance.md`、`.steps/README.md`（P5 状态行）、`.steps/p5/README.md`（状态行）、`docs/workspace.md`（若 §3.2 有遗漏）。
  **实施步骤**：
  1. 门禁：`cargo build --workspace`、`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`make gen` / `make gen-check` / `make ui-check`。
  2. 回归：`tool` / `mcp` / `run` / `client` / `persist` / `runtime` 六组用例与 `--all`（**需授权**）。
  3. 报告：按 `acceptance.md` 的 10 组逐条取证，未取证项写明「未授权」或「未采集：原因」；结转风险以 `KI-P5-xxx` 登记在报告末章。
  4. 同步 `.steps/README.md` 的进度表与 `p5/README.md` 的状态行。
  **验收标准**：
  - 报告中每条判据都有「证据类型 + 具体命令/用例名/断言」。
  - 未取证项为零遗漏（逐条注明原因），不得留空。
  - `.steps/README.md`、`p5/README.md`、`decisions.md` 三处状态口径一致（20 个任务、6 个批次、判据组数）。
  **测试要求**：回归执行需授权；未授权时报告标注「未授权」，阶段不视为完成。
