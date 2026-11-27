# P5 技术决策记录

**状态：已定稿（P5 已完成，2026-11-27）。** 本文件记录 P5 的阶段级决策，编号 `D-P5-xxx`；被否决与被修订的方案保留记录，不删除。

任务分解见 [tasks.md](tasks.md)，判据见 [acceptance.md](acceptance.md)，阶段边界见 [README.md](README.md) §二。

每项记录包含：问题 / 背景与证据 / 可选方案 / 决策 / 理由 / 约束 / 承接任务。

状态定义：

| 状态 | 含义 |
|------|------|
| `DECIDED` | 已确定，可直接执行 |
| `DECIDED (待验证)` | 方向已定，但依赖某个实现任务确认细节 |
| `OPEN` | 未确定，动手前必须讨论 |
| `REJECTED` | 已否决（保留记录，不删除） |
| `DEFERRED` | 明确推迟到后续阶段 |

---

## 决策索引

| ID | 主题 | 状态 |
|---|---|---|
| D-P5-001 | 新增 `agent-skill`，并允许它依赖 `agent-tool`（唯一工具是 `skill_load`） | DECIDED |
| D-P5-002 | 渐进披露：索引进系统提示 + `skill_load` 按需取正文 | DECIDED |
| D-P5-003 | 索引位置与形态：与 `AGENTS.md` 合并成同一条 system 消息，不新增 `ContextSource` 取值 | DECIDED |
| D-P5-004 | `SKILL.md` 规范：`name` / `description` 必填，未知字段忽略，名字可与目录不一致 | DECIDED |
| D-P5-005 | Skill 命名规则与同名冲突（两份都判 `INVALID`） | DECIDED |
| D-P5-006 | 发现目录：`<AGENT_HOME>/skills/<dir>/SKILL.md`，单根可配置，不做文件监视 | DECIDED |
| D-P5-007 | `skills.toml` 独立文件、缺省值、`disabled` 名单、启停只在内存 | DECIDED |
| D-P5-008 | 重载语义：启动期发现 + 显式 `RELOAD` | DECIDED |
| D-P5-009 | 工具可用性在渲染 / 加载时按注册表现状判定；缺工具不阻断加载但必须明确提示 | DECIDED |
| D-P5-010 | 三级披露与预算：索引行级截断、正文按章节切分、`max_inline_chars` | DECIDED |
| D-P5-011 | `skill_load` 契约：参数、风险、文件声明、结果文本结构 | DECIDED |
| D-P5-012 | 引用材料只给清单，读取复用既有 `file_read`，不新增第二个工具 | DECIDED |
| D-P5-013 | 脚本不因位于 Skill 目录而获得执行权限 | DECIDED |
| D-P5-014 | 可见性：不新增 Run 事件，加载事实走工具事件 + `metrics` 的 `skills` 段 | DECIDED |
| D-P5-015 | 管理接口形态：3 条（`list` / `get` / `update`），`update` 返回整张清单 | DECIDED |
| D-P5-016 | `agent-runtime` 改动边界：只把 `ContextBuilder` 的构造上移到装配层 | DECIDED |
| D-P5-017 | 解析依赖：`serde_yaml_ng` 0.10 + 自研 ATX 章节切分 | DECIDED（版本待 P5-001 验证） |
| D-P5-018 | 错误文案与脱敏：相对路径 + 行号 + 原因，不吐绝对路径与正文 | DECIDED |
| D-P5-019 | 客户端形态：TUI 覆盖层 + WebUI 抽屉，只读 + 启停 + 重载 | DECIDED |
| D-P5-020 | 示例 Skill 的数据位置：`crates/agent-skill/assets/skills/` | DECIDED |

---

## D-P5-001 新增 `agent-skill`，并允许它依赖 `agent-tool`

- **状态**：`DECIDED`
- **问题**：Skill 能力放在哪个 Crate？`skill_load` 工具的适配器放哪里？`docs/workspace.md` 原先声明 `agent-skill` 不得依赖 `agent-tool`，与本阶段需要是否冲突？
- **背景与证据**：
  - `docs/workspace.md` §3.2 已规划 `agent-skill`（职责：Discovery / Definition / SKILL.md 解析 / Metadata / Loader / References / Instructions / 对 Context 的贡献），边界写明「只提供任务知识，不提供执行能力；Skill 声明所需 Tool，但不由本 Crate 执行 Tool」；§5.1 依赖清单里 `agent-skill` 的「禁止依赖」列是 `agent-runtime`、`agent-tool`。
  - P4 的先例：`agent-mcp` **依赖** `agent-tool`，因为 MCP 工具就是 `Arc<dyn Tool>`；「协议不出 crate」靠的是 `rmcp` 只在该 crate，而不是禁止它认识工具契约。
  - 本阶段的选择是「索引进提示 + `skill_load` 工具」（D-P5-002），因此必须有一个实现 `agent_tool::Tool` 的适配器；把适配器放进 `agent-tool` 会让「工具层认识 Skill」，与 P4 判据 A-3（工具层不认识 MCP）同类口径冲突；放进装配层 `manager-agent` 会让装配层承载领域逻辑。
- **可选方案**：
  1. 新增 `agent-skill`，允许依赖 `agent-tool`，`skill_load` 适配器放本 crate，同步改 `docs/workspace.md`；
  2. 新增 `agent-skill` 但保持不依赖 `agent-tool`，把 `skill_load` 适配器放 `manager-agent`；
  3. 新增 `agent-skill`，再新增一个 `agent-skill-tool` 胶水 crate；
  4. 不加工具（改成服务端预注入正文），见 D-P5-002。
- **决策**：①。
- **理由**：
  - `skill_load` 是「知识检索」，不是「执行 Skill 声明的能力」；边界原文约束的是后者。②④ 的口径差异写清后不构成冲突。
  - ②会让装配层出现一个 `impl Tool`，而装配层的既有职责是「翻译配置 + 装配 + 门面转发」；同时 `bin-tests` 要覆盖这个工具时只能跨越装配层，用例结构更绕。
  - ③为了一个约 40 行的适配器新增 crate，与「不为单点能力新建 crate」的既有取舍相反，且会让 Crate 数量再+1（路线图里已经没有这个名字）。
- **约束**：
  - `agent-skill` 的依赖清单不得出现 `agent-runtime` / `agent-route` / `rig-core` / `service-core` / `lib-db` / `types-core` / `lib-web*` / `bin-*`（判据 A-1）。
  - `agent-skill` 只允许实现一个 `Tool`（`skill_load`），不得注册、不得执行 Skill 声明的任何工具（判据 T-6 / E-2）。
  - `docs/workspace.md` 的 §3.2 `agent-skill` 段、§5.1 依赖清单、§8.2 阶段范围表必须同步（P5-001 的第 4 步），写完以 `cargo tree` 为准。
- **承接任务**：P5-001、P5-008。

---

## D-P5-002 渐进披露：索引进系统提示 + `skill_load` 按需取正文

- **状态**：`DECIDED`
- **问题**：Skill 正文怎么进入上下文？由模型主动请求，还是由服务端在组装时被动注入？
- **背景与证据**：
  - README §八 风险 2 原样提出这个岔口，并指出「两种方案对上下文预算和可预测性影响很大」。
  - 现状：`agent-context::ContextBuilder::build` 每次现场读 `AGENTS.md` 并组装成一条 system 项，历史裁剪只丢最旧端（D-P2-013）；工具结果经执行器按 `[tools] max_result_chars` 截断；P3 已具备完整的工具调用回路。
  - 服务端按规则匹配用户输入（关键词 / 正则）需要额外维护一套触发规则，且命中与否对用户不可见——错命中等于白烧 token，未命中等于 Skill 不存在。
- **可选方案**：
  1. 索引（名字 + 描述 + 缺工具标注）进系统提示，正文由模型调用 `skill_load` 取；
  2. 服务端按关键词规则匹配后把正文直接注入系统提示；
  3. 完全工具化：连索引也不进系统提示，模型需先调 `skill_list`。
- **决策**：①。
- **理由**：
  - 索引只有几十到几百字，成本固定且可预算（D-P5-010 给硬上限）；正文按需，未用到的 Skill 一个字符都不进上下文。
  - 模型有能力判断「这件事要不要查技能」，而规则匹配没有；把判断权交给模型与「Agent Runtime 不特判」的既有取向一致（D-P3-001）。
  - ③多一轮请求才能知道有什么，发现率与延迟都更差；索引本身就是「可发现性」的实现。
- **约束**：
  - 索引内容只有「名字 + 描述（截断）+ 缺工具标注」，不得包含正文片段。
  - 正文只允许经 `skill_load` 进入上下文，不允许在 Context 组装的任何分支里被直接拼入（判据 P-2）。
  - `skill_load` 的结果是普通工具结果，不新增事件、不改 Run 生命周期（D-P5-014）。
- **承接任务**：P5-007、P5-008、P5-017。

---

## D-P5-003 索引位置与形态：与 `AGENTS.md` 合并成同一条 system 消息

- **状态**：`DECIDED`
- **问题**：索引放系统提示、放首条用户消息，还是放独立消息段？要不要为它新增 `ContextSource` 取值？
- **背景与证据**：
  - `system.rs` 现状：`AGENTS.md` → 一条 `ContextItem { source: System, role: System }`，为空则不发送系统消息。
  - `ContextSource` 已有 `System` / `History` / `Tool` / `User` 四个取值，注释写明「P3 起的 Tool / Skill / MCP 贡献点在此追加取值」。
  - 同一请求发两条 `role: system` 的消息在 OpenAI 兼容生态里并非所有实现都接受；这个风险不该由一个「锦上添花」的小节承担。
- **可选方案**：
  1. 与 `AGENTS.md` 文本合并成一条 system 消息（中间一个空行），`ContextSource` 不变；
  2. 单独成一条 system 消息，新增 `ContextSource::Skill`；
  3. 插在历史之后、本次用户输入之前，作为一条 `role: User` 的「系统注入」。
- **决策**：①。
- **理由**：
  - 只有一条 system 消息，模型侧兼容性最好，也最符合「系统提示 = 一组固定指令」的语义。
  - ②在预算上等价，但多一次兼容性风险，收益只是「能在 Context 里单独认出来」——本阶段的用例可以通过请求体文本断言，不需要枚举值。
  - ③把系统约束伪装成用户输入，模型可能把它当成可讨论的请求，且会污染 `trim` 的段切分。
- **约束**：
  - 合成顺序固定：`AGENTS.md` 内容 → 一个空行 → `## 可用 Skill` 小节；索引为空时完全不出现该小节，也不留空行（判据 P-1）。
  - 索引文本格式固定（用例逐字符核对）：
    ```text
    ## 可用 Skill

    以下技能提供任务步骤与约束；需要正文时调用 skill_load 获取。

    - pdf-report：生成 PDF 报表：给定指标与时间范围，产出图表与结论文档。
    - db-migrate：迁移检查与回滚预案（缺少工具：sqlite3）
    - （另有 12 个技能因篇幅未展示，可用 skill_load 按名字加载）
    ```
  - 不新增 `ContextSource` 取值；`source.rs` 与 `policy.rs` 只允许改注释。
- **承接任务**：P5-007、P5-017。

---

## D-P5-004 `SKILL.md` 规范

- **状态**：`DECIDED`
- **问题**：`SKILL.md` 的 frontmatter 允许哪些字段？严格闭集还是允许超集？`name` 必须与目录名一致吗？
- **背景与证据**：
  - README §二 要求「格式尽量兼容开放的 Agent Skills 格式」，§八 风险 1 提出「严格兼容或允许超集」。
  - 需求方在规划时明确：**允许未知字段；缺 `name` / `description` 报错；允许名字与目录不一致**。
- **可选方案**：
  1. 已知字段闭集、未知字段报错；
  2. 已知字段 + 未知字段忽略（超集兼容）；
  3. 不解析 frontmatter，全用纯文本。
- **决策**：②，并采用下面这张字段表。
- **理由**：
  - ①会让生态里现成的 Skill（常带 `license` / `allowed-tools` / `metadata` 之类的自有字段）无法直接使用，与「兼容开放格式」的目标相反。
  - ③丢掉「名字 / 描述 / 依赖工具」三项能力，退化成一个 Markdown 文件，无法做索引与可用性校验。
  - 名字与目录不一致不报错：目录只是存放位置，名字是逻辑标识；冲突由 D-P5-005 的同名规则兜住。
- **约束**：
  - 字段表（`agent-skill` 的 `SkillMetadata`）：
    | 字段 | 必填 | 规则 |
    |---|---|---|
    | `name` | 是 | `^[a-z0-9][a-z0-9_-]{0,63}$`，全局唯一（D-P5-005） |
    | `description` | 是 | 非空；索引展示时按 `max_description_chars` 截断 |
    | `requires_tools` | 否 | 字符串数组，每个元素过 `agent_tool::validate_name` |
    | `version` | 否 | 字符串，原样展示，不做语义化版本校验 |
    | `author` | 否 | 字符串，原样展示 |
  - frontmatter 必须以独占一行的 `---` 开始（容忍 BOM）与结束；正文（其后内容）`trim` 后必须非空。
  - 整文件 ≤ `max_skill_bytes`（默认 64 KiB），必须是 UTF-8。
  - 未知字段忽略，不产生告警，不出现在 `SkillMetadata` 里（判据 D-3）。
- **承接任务**：P5-003、P5-016。

---

## D-P5-005 Skill 命名规则与同名冲突

- **状态**：`DECIDED`
- **问题**：Skill 名字允许哪些字符？两个目录声明同一个名字怎么办？
- **背景与证据**：名字既出现在索引行里，又是 `skill_load(name)` 的参数、接口的查询参数与指标维度，必须是一个稳定的短标识；目录名由用户随手命名，不能承担这个职责。
- **可选方案**：
  1. 与工具名同规则（`^[a-z][a-z0-9_]{1,63}$`）；
  2. 允许 `-`：`^[a-z0-9][a-z0-9_-]{0,63}$`；
  3. 不做校验（任意字符串）。
- **决策**：②；同名冲突时**两份都判 `INVALID`**，不采用「先到先得」或「字典序优先」。
- **理由**：
  - ①不允许 `-`，会让生态里常见的 `pdf-report` 这类名字被迫改写，兼容性无谓受损；②仍能安全地放进索引行、URL 查询参数与 JSON 字段。
  - ③会让名字里出现空白、`：`、换行，直接破坏索引行的可解析性。
  - 「先到先得」会让另一个 Skill 静默消失——用户只看到自己明明放了两份却只有一份可用，且无法从任何界面发现原因；「两份都错」把问题暴露在 `api/skill/list` 里，符合「失败可见」。
- **约束**：
  - 名字与目录名可以不一致（D-P5-004），但**必须全局唯一**；冲突错误文案同时给出两个目录。
  - 冲突解除（改名）后重新 `RELOAD` 应两份都变 `READY`（判据 D-7）。
  - 名字大小写敏感，必须小写（大写直接判不合法），不做自动归一化——静默改写名字会让用户按原名加载失败。
  - 正则有两处落点：`agent-skill::validate_name`（解析与注册）与 `lib-bin-cli::config`（`skills.toml` 的 `disabled` 列表校验，该 crate 不依赖 `agent-skill`）；两处必须逐字符一致，由 `skill.config` 与 `skill.parse` 的用例同时锚定。
- **承接任务**：P5-003、P5-004、P5-016。

---

## D-P5-006 发现目录：`<AGENT_HOME>/skills/<dir>/SKILL.md`

- **状态**：`DECIDED`
- **问题**：Skill 从哪里发现？支持几个根目录？要不要监视文件变化？
- **背景与证据**：
  - README §二 写「本地目录、用户目录，可扩展」；本项目服务端是单体进程，`AGENT_HOME` 是唯一的「用户目录」（`config.toml` / `mcp.toml` / `AGENTS.md` 都在这一层）。
  - `[tools.file].allow_dirs` 为空时，文件类工具的可读范围就是 `AGENT_HOME`：Skill 目录天然在 `file_read` 的可读范围内，引用材料不需要额外放行。
- **可选方案**：
  1. 单根 `<AGENT_HOME>/skills`，`skills.toml` 允许改 `root`；
  2. 多根（`AGENT_HOME/skills` + 配置里的额外根目录 + 会话工作区）；
  3. 不做约定目录，完全由配置逐条列出 `SKILL.md` 路径。
- **决策**：①。
- **理由**：
  - ②需要定义同名跨根冲突、信任级别与路径逃逸规则，本阶段没有真实需求支撑，属于为假想场景付复杂度；`root` 可配置已经能覆盖「放在别处」的诉求。
  - ③让「放入一个新目录即被发现」变成「改配置才行」，与验收方向 1 相悖。
  - 文件监视（inotify / ReadDirectoryChanges）跨平台差异大、易误触发，本阶段用显式 `RELOAD` 覆盖（D-P5-008）。
- **约束**：
  - 只扫一层：`<root>/<dir>/SKILL.md`，不递归找 Skill；`<root>` 相对应用根目录解析，绝对路径按原样。
  - 目录缺失 = 零 Skill，不是错误；`SKILL.md` 缺失的目录不进清单。
  - 不做符号链接跟随（Skill 目录本身与 `references/` 都不跟随）。
  - 不引入工作区 / 会话目录（P6 再议，见 README §七）。
- **承接任务**：P5-004、P5-016。

---

## D-P5-007 `skills.toml` 独立文件

- **状态**：`DECIDED`
- **问题**：Skill 的启用状态与上限配置放哪里？要不要入库？
- **背景与证据**：
  - README §四 原规划提到「P2 的持久化：Skill 启用状态与使用记录」，但 P4 已经为同类问题定了口径：配置是**独立文件**、启停只在内存（D-P4-008）。
  - 启用状态是「运维开关」，不是业务数据：它没有历史、没有归属、没有并发写需求；放数据库只会引入迁移与「文件 / 库谁是事实源」的问题。
  - 「使用记录」本阶段没有被任何判据要求，指标（D-P5-014）已能回答「加载了几次」。
- **可选方案**：
  1. 独立 `skills.toml`（发现根、`disabled` 名单、四个上限），启停只在内存；
  2. 并进 `config.toml` 的 `[skills]` 段；
  3. SQLite 表（迁移 `0003`）+ repository 查询。
- **决策**：①。
- **理由**：
  - 与 `mcp.toml` 完全同构：一个能力一个文件，改一个能力的配置不牵动主配置的重启节奏。
  - ②会让主配置随 Skill 数量增长（`disabled` 名单会越写越长），且主配置由启动方一次性装载，语义上不适合「运行期可改的开关集合」。
  - ③的收益（跨重启保留启停）用配置文件的 `disabled` 名单就能达到，且用户可以直接编辑、可以进版本库；代价（迁移、并发写、事实源争议）全是不必要的。
- **约束**：
  - 文件缺失 = 全部缺省（Skill 功能启用，根目录 `skills`，`disabled` 为空，四个上限取默认值）；文件存在但内容为空是硬错误（与 `mcp.toml` 一致），退出码 2。
  - 缺省值：`max_index_chars = 4000`、`max_description_chars = 200`、`max_skill_bytes = 65536`、`max_inline_chars = 6000`。
  - 本阶段**不做数据库迁移**；「使用记录」留 P6（`DEFERRED`）。
  - `skills.toml` 的 `enabled = false` 时：不发现、不注入、不注册工具、`api/skill/list` 返回空清单。
- **承接任务**：P5-002、P5-010、P5-016。

---

## D-P5-008 重载语义：启动期发现 + 显式 `RELOAD`

- **状态**：`DECIDED`
- **问题**：Skill 更新后什么时候生效？要不要自动重扫？
- **背景与证据**：验收方向 1 允许「无需重启即可被发现（**或按明确规则重载**）」；索引与正文都在每次组装 / 调用时现场读取注册表快照，因此「重载」只需替换快照。
- **可选方案**：
  1. 启动期发现一次 + `api/skill/update { action: RELOAD }` 显式重扫；
  2. 周期性轮询目录 mtime；
  3. 文件系统监视（inotify / ReadDirectoryChangesW）。
- **决策**：①。
- **理由**：
  - ②会让「刚改完为什么没生效」变成 0~N 秒的随机体验，且轮询本身在容器 / 网络盘上不可靠。
  - ③需要平台特定依赖（本阶段没有）、需要处理编辑器「先写临时文件再 rename」的抖动、还要处理大量事件合并。
  - ①的语义边界最清晰：`RELOAD` 之后的状态就是磁盘上的状态，测试可以逐字符断言（判据 D-7 / I-4）。
- **约束**：
  - `RELOAD` 是「整体替换快照」：新增、删除、内容变更、格式修正都在一次 `RELOAD` 内生效。
  - `disabled` 名单在 `RELOAD` 之后保留（只保留仍然存在的名字），不重置。
  - 两次连续 `RELOAD` 幂等：清单与索引逐字段相等（判据里核对）。
  - 运行期注册的工具（MCP）不需要 `RELOAD` 就能影响「缺工具」标记（D-P5-009）。
- **承接任务**：P5-005、P5-012、P5-018。

---

## D-P5-009 工具可用性在渲染 / 加载时判定

- **状态**：`DECIDED`
- **问题**：`requires_tools` 声明的工具「此刻可用吗」在什么时刻判定？不可用时是拒绝加载还是带提示加载？
- **背景与证据**：
  - 装配顺序已定（D-P4-009 / P4 的 `build_mcp`）：MCP 在启动期并发连接、失败不阻塞启动，工具是**运行期**陆续进注册表的（`ToolRegistry` 在 P4 已改为 `RwLock` 可变）。
  - 在发现期判定，会把「MCP 还没连上」误判成「工具不存在」，且之后不会自愈。
  - `ToolRegistry::model_definitions` 只包含权限过滤后的工具：权限为 `deny` 的工具模型看不到，也就等于「无法执行」。
- **可选方案**：
  1. 渲染索引与 `skill_load` 时现场判定（经 `ToolAvailability` 抽象）；
  2. 发现时判定一次并缓存，`RELOAD` 时刷新；
  3. 不判定，只在执行期由模型自己撞墙。
- **决策**：①；不可用时**允许加载**，但结果里必须写明缺失清单与「涉及这些工具的步骤必须停止，并明确告知用户」；权限 `deny` 视为不可用。
- **理由**：
  - ①随注册表现状实时变化，MCP 连上后标记自动消失，不需要重载。
  - ②会在「MCP 连上但没重载」这段时间里持续误报，且误报是**反向**的（明明能用却说不能用），比漏报更伤。
  - ③等于让模型「假装执行」，直接违反 README 验收方向 4。
  - 拒绝加载的理由不成立：Skill 里往往只有部分步骤依赖该工具，整份禁止加载会丢掉可用的部分；带提示加载既保留信息，又把约束显式交给模型（并可在用例里断言）。
- **约束**：
  - `ToolAvailability` 定义在 `agent-skill`（只依赖名字），实现由装配层给出（`RegistryAvailability(Arc<ToolRegistry>)`）；判定不得跨 `await` 持锁。
  - 文案单一产点在 `availability.rs`：索引行后缀 `（缺少工具：a、b）`；加载结果末尾「缺少的工具」节 + 那句约束。
  - 不做「自动禁用缺工具的 Skill」——启用状态只由用户决定。
- **承接任务**：P5-006、P5-019。

---

## D-P5-010 三级披露与预算

- **状态**：`DECIDED`
- **问题**：`max_index_chars` / `max_inline_chars` / `[tools] max_result_chars` 三个上限怎么分工？大 Skill 的正文怎么才能既完整又不超预算？
- **背景与证据**：
  - README §三 要求「Markdown 解析：正文与章节切分，支持按章节按需加载」。
  - 执行器按 `[tools] max_result_chars`（默认 8000 字符）截断工具结果；`ContextPolicy::available_tokens(system_tokens)` 已把系统提示计入预算。
  - 一份写得好的 Skill（步骤 + 示例 + 检查表）很容易超过 6000 字符，一刀切截断会把最后几步切掉，模型就会「照着半截流程干」。
- **可选方案**：
  1. 三级披露：索引（行级）→ 章节目录 → 单节正文；上限分别是 `max_index_chars` / `max_inline_chars` / `max_inline_chars`；
  2. 只有两级：索引 → 全文，超限就截断；
  3. 全文不设上限，交给执行器截断。
- **决策**：①。
- **理由**：
  - ②在「超长正文」这个最常见的场景里给出最坏的体验：模型拿到前半截，且不知道自己拿的不全。
  - ③把「知识完整性」交给一个为「工具输出」设计的闸门，语义错位，且截断点不可预期（按 token 预算变化的截断点无法测试）。
  - ①的每次返回都是「完整的某个单元」，并且「不完整」这件事本身是显式的（目录里写着每节字数，截断时写着剩余字数）。
- **约束**：
  - 索引：字符数 ≤ `max_index_chars`，超限**整行**停止追加，末尾固定一句「另有 N 个技能因篇幅未展示」；不输出半行。
  - 正文：总字符 ≤ `max_inline_chars` → 全文；否则 → 章节目录（每节 `标题（n 字）`）；带 `section` → 该节（含子节），仍超限则截断并标注剩余字符数。
  - `max_inline_chars > [tools] max_result_chars` 时装配期记 `warn`（配置冲突可见，不阻断启动）；索引与正文的包装行都计入上限。
  - `run_started.input_tokens` 只随索引增长（正文在后续轮次才进历史），这条写进 `builder.rs` 注释与 `skill.budget` 用例。
- **承接任务**：P5-005、P5-008、P5-009、P5-017。

---

## D-P5-011 `skill_load` 契约

- **状态**：`DECIDED`
- **问题**：`skill_load` 的名字、参数、风险等级、文件访问声明与结果结构怎么定？
- **背景与证据**：P3 的 `Tool` 契约要求 `name` / `description` / `schema` / `risk` / `file_access`；`ToolFileAccess` 决定并发语义（`Read` → `Parallel`）与审计口径；`ToolContext.home` 是路径基准。
- **可选方案**：
  1. 一个工具 `skill_load(name, section?)`，只读，`ReadOnly` / `Read`；
  2. 两个工具：`skill_load(name)` + `skill_read_reference(name, path)`；
  3. 一个工具但返回体里嵌入引用文件内容。
- **决策**：①。
- **理由**：
  - ②的第二个工具与既有 `file_read` 高度重复（都是为了读一个文本文件），会形成「同一能力两条路径」，且审计里出现两个语义相近的工具名。
  - ③会让一次加载就吞掉整个 `references/`（可能是几万字），正好破坏本决策要守的预算。
  - ①的参数只有一个可选项，schema 稳定，`required` 只含 `name`（判据 T-1）。
- **约束**：
  - 名字 `skill_load`（满足 `validate_name`），`risk = ReadOnly`，`file_access = Read`，并发取默认（`Parallel`）。
  - 参数 `{ name: String, section: Option<String> }`，`deny_unknown_fields`；`section` 按标题文本匹配（允许省略 `#` 前缀与前后空白），重名取第一个。
  - 结果文本固定结构（用例逐段核对）：
    ```text
    # 技能：db-migrate

    <正文 / 章节目录 / 指定章节>

    ## 缺少的工具
    - sqlite3

    涉及这些工具的步骤必须停止，并明确告知用户无法执行。

    ## 参考资料（用 file_read 按路径读取）
    - skills/db-migrate/references/rollback.md
    ```
  - 未知技能 / 未启用 / `INVALID` / 未知章节都是 `ToolError`（映射为模型可读文本），Run 不 `FAILED`、不挂起。
- **承接任务**：P5-008、P5-017。

---

## D-P5-012 引用材料只给清单

- **状态**：`DECIDED`
- **问题**：Skill 附带的参考资料怎么「按需加载」？
- **背景与证据**：`[tools.file]` 的缺省 `allow_dirs` 为空，等价于「只允许应用根目录」，Skill 目录天然在内；`file_read` 已有截断与路径校验。
- **可选方案**：
  1. `skill_load` 只返回清单（相对路径 + 一句话提示），读取交给既有 `file_read`；
  2. `skill_load` 内联全部引用文件；
  3. 新增 `skill_read_reference` 工具。
- **决策**：①。
- **理由**：
  - ②在最坏情况下（模板 + 示例 + 数据字典）一次返回十几万字，必然被截断，且与「按需」相反。
  - ③是 D-P5-011 已否决的重复能力；而 `file_read` 的路径校验、截断、审计都已经过 P3 验证，复用它等于免费继承这些保证。
  - ①也让「Skill 与 Tool 协作」成为真实路径而不是文档说法：模型必须先 `skill_load`（拿清单）再 `file_read`（读材料），两步都在审计里可见。
- **约束**：
  - 清单里给的是**相对应用根目录**的路径（`skills/<dir>/references/<rel>`），不是绝对路径（判据 E-3）。
  - 清单包含「用 `file_read` 按路径读取」的提示；不保证目标文件在 `allow_dirs` 之外时可读（那是 `[tools.file]` 的职责，不由本阶段兜底）。
  - `references/` 下的可执行文件与脚本同样只以「路径字符串」出现，不解析、不执行（D-P5-013）。
- **承接任务**：P5-003、P5-008、P5-018。

---

## D-P5-013 脚本不因位于 Skill 目录而获得执行权限

- **状态**：`DECIDED`
- **问题**：README 验收方向 6 要求「Skill 目录中的脚本不会因为位于该目录而获得执行权限」。怎么落地？
- **背景与证据**：P3 / P4 的权限模型只有两个入口：工具清单（`model_definitions`）与执行期判定（`executor`）；不存在「按目录授权」的机制。
- **可选方案**：
  1. Skill 只是文本：不注册工具、不提供执行入口，`references/` 只读；执行能力仍只能来自既有工具；
  2. 给 Skill 目录一个「可执行」声明（如 frontmatter 里的 `scripts`），由 Skill 自己执行；
  3. 依赖 P6 的沙箱。
- **决策**：①；②③明确不做（②属于 README §七 Non-goals 的「Skill 沙箱执行」，③是 P6）。
- **理由**：
  - 权限体系的价值在于「只有登记过的能力可执行」；按目录授权会开一个旁路，且无法审计（脚本不产生 `tool_calls` 记录）。
  - ①下「脚本」在系统里只是一个可被 `file_read` 读到的文本文件，模型想执行它必须显式调用某个工具，而那一步的权限、超时、审计全部生效。
- **约束**：
  - `agent-skill` 不 `use std::process`、不 `use std::os::unix`/`windows` 的进程相关 API；全仓检索 `skill` 相关代码里 `Command::new` 0 命中（判据 E-2 的静态部分）。
  - 不解析可执行位、不按扩展名判定「这是脚本」；引用扫描只区分「普通文件 / 目录 / 符号链接」。
  - 用例中在 Skill 目录放入可执行文件后，工具清单与 `tools` 字段逐项不变。
- **承接任务**：P5-003、P5-008、P5-018。

---

## D-P5-014 可见性：不新增 Run 事件

- **状态**：`DECIDED`
- **问题**：「当前加载了哪个 Skill」要不要变成一个新的 Run 事件？要不要记进 `messages` 表？
- **背景与证据**：
  - P4 已定口径：工具来源的变化不新增事件、`tool_calls` 不加列（D-P4-015）；P3 的事件集合是 `run_started` / `run_completed` / `run_failed` / `run_cancelled` / `server_shutdown` / `session_event` / 三个 `tool_call_*`。
  - `skill_load` 是一次普通工具调用：`tool_call_started` / `tool_call_completed` 已经携带工具名与耗时，`GET api/tool/audit/list` 已有按 Run 的审计记录。
  - 客户端要「看到当前加载了哪个 Skill」，可以在同一份事件流里从工具名推出，不需要新事件。
- **可选方案**：
  1. 不新增事件；加载事实 = 工具事件 + 审计 + `metrics.skills.loads`；
  2. 新增 `skill_loaded` 事件；
  3. 把加载结果写进 `messages` 表的元数据列。
- **决策**：①。
- **理由**：
  - ②会让事件集合随「知识来源」的每次扩展而膨胀，且它携带的信息（技能名、是不是第一次加载）在工具事件里都能拿到。
  - ③需要迁移与契约扩展，而「加载了什么」本质是「调用了哪个工具」的派生信息，重复存储只会产生两套事实。
- **约束**：
  - `RunEventKind` 的取值清单与 P4 逐项相等（判据 R-3，事件 0 新增）。
  - `metrics.skills` 只提供计数（`loads` / `load_failures` / `sections_served`），不提供「按名字的分布」——那需要无界标签集，不属于进程内指标的形态。
  - 客户端展示「某轮加载了哪个 Skill」时，读 `tool_call_started` 的 `toolName == skill_load` 与参数，不引入新契约。
- **承接任务**：P5-011、P5-014、P5-015、P5-018。

---

## D-P5-015 管理接口形态：3 条，`update` 返回整张清单

- **状态**：`DECIDED`
- **问题**：Skill 管理需要几个接口？`update` 返回单条还是整张清单？未知名字是 400 还是 404？
- **背景与证据**：
  - `docs/api/readme.md` 的约定：path 统一 `api/` 开头、资源段小写单数、动作段只用 `create` / `update` / `delete` / `get` / `list` / `cancel` / `stream` / `post`，无路径占位符。
  - P4 的 MCP 管理接口是 3 条（`server/list` / `tool/list` / `server/update`），`update` 的四个动作都返回动作完成后的状态；未知 `serverId` 是 400 而不是 404。
- **可选方案**：
  1. 3 条：`skill/list`、`skill/get`（参数 `name`）、`skill/update`（动作 `ENABLE` / `DISABLE` / `RELOAD`，返回整张清单）；
  2. 4 条：把 `get` 拆成 `detail` + `body`（正文单独一条）；
  3. 5 条：`enable` / `disable` / `reload` 各一条独立 path。
- **决策**：①。
- **理由**：
  - ②的正文与元数据同源同一次读取，拆开只会带来「详情与正文不一致」的窗口。
  - ③违反「动作段只用固定集合」的约定（`enable` / `disable` 不在集合里），且客户端要维护更多 path。
  - `update` 返回整张清单的理由：`RELOAD` 的结果本来就是「整张清单」，而 `ENABLE` / `DISABLE` 只返回一条时客户端仍要再拉一次 `list` 才能刷新界面——返回整张清单让「动作 + 刷新」变成一次往返。
- **约束**：
  - 错误一律 400 + 固定文案：`技能不存在` / `技能加载失败：<原因>` / `技能更新缺少 name` / `RELOAD 不接受 name`；未知 `name` 不用 404（与 MCP 的 `serverId` 口径一致）。
  - `RELOAD` 时 `name` 必须缺省或 `null`；`ENABLE` / `DISABLE` 必须给 `name`。
  - 接口总数 29 → 32，`docs/api/skill.md` 与 `docs/api/readme.md` 同步，`make gen` 必须在 `make ui-check` 之前（D-006）。
- **承接任务**：P5-012、P5-013、P5-018。

---

## D-P5-016 `agent-runtime` 改动边界

- **状态**：`DECIDED`
- **问题**：索引要进系统提示，而系统提示由 `AgentRuntime::new` 内部构造的 `ContextBuilder` 组装。改谁？
- **背景与证据**：`port.rs` 的 `AgentRuntime::new` 现在自己做 `builder: ContextBuilder::new(home)`；`loop_.rs` 的 `prepare` 只调 `builder.build(...)`，不认识系统提示的来源。
- **可选方案**：
  1. `AgentRuntime::new` 增加一个 `ContextBuilder` 参数（装配层构造并注入），循环不改；
  2. 让 `AgentRuntime` 认识 `SkillRegistry`，自己拼索引；
  3. 在 `loop_.rs` 的 `prepare` 里追加索引项。
- **决策**：①。
- **理由**：
  - ②让运行时依赖 Skill 概念，与 P4「运行时零改动」的取向相反，且 `agent-runtime` 的依赖清单会多一条边。
  - ③把「上下文从哪来」的知识塞进循环，`prepare` 会变成第二个组装点，与「Context 组装只有一个入口」冲突。
  - ①的代价只是一个参数：`home` 仍在（执行器需要路径基准），`ContextBuilder` 的构造从「运行时自己造」变成「装配层造好给」。
- **约束**：
  - `crates/agent-runtime/` 的改动只允许 `src/port.rs`（构造签名）与 `src/lib.rs`（re-export / 文档）；`src/loop_.rs` 与 `src/loop_/*` 零改动（判据 A-5）。
  - `AgentRuntime` 的字段与 `drive` 的时序不变；`ContextBuilder::new` 的调用点从 `port.rs` 消失，全仓只保留装配层的构造。
  - 事件、工具执行、取消、限流逻辑一律不动。
- **承接任务**：P5-010、P5-020。

---

## D-P5-017 解析依赖：`serde_yaml_ng` + 自研 ATX 章节切分

- **状态**：`DECIDED（版本待 P5-001 验证）`
- **问题**：frontmatter 用什么 YAML 解析？正文要不要引入 Markdown 解析器？
- **背景与证据**：
  - workspace 当前没有 YAML crate，也没有 Markdown crate（`Cargo.toml` 只有 `toml` / `serde_json`）。
  - `serde_yaml` 已停止维护（0.9.34 起标记 deprecated），直接用会留下一个「不再修」的依赖。
  - `cargo add --dry-run -p types-core serde_yaml_ng` 实测解析到 **0.10.0**（2026-11-26），API 与 `serde_yaml` 同形（`from_str` + `Error::location()`），可以直接拿到行号。
  - 章节切分只需要 ATX 标题（`#` ~ `######`）与代码围栏两种语法；`pulldown-cmark` / `comrak` 会带来完整 CommonMark AST、额外依赖与编译时间。
- **可选方案**：
  1. `serde_yaml_ng` + 自研按行切分；
  2. `serde_norway`（同为 `serde_yaml` 的维护 fork）+ 自研切分；
  3. `saphyr`（YAML 1.2 解析器，需手工映射到结构体）+ 自研切分；
  4. 任意 YAML crate + `pulldown-cmark`。
- **决策**：①；若 P5-001 编译期发现 `serde_yaml_ng` 与 workspace 的 `serde` 版本或 MSRV 冲突，退到 ②（同为 drop-in），不选 ③④。
- **理由**：
  - ①最省事：`Deserialize` 直接落到 `SkillMetadata`，错误自带行列号；维护状态良好。
  - ③要手写「YAML 值 → 结构体」的映射与类型校验，等于把 serde 已有的能力再实现一遍。
  - ④为了切章节引入一整套 CommonMark 解析器，收益（更完整的语法支持）与本阶段需要（标题 + 围栏）不匹配，而且 AST 的字符偏移还要自己映射回行号。
- **约束**：
  - 版本固定在 workspace 的一处；`agent-skill` 只写 `workspace = true`。
  - 切分只认 ATX 标题与 ``` ``` / `~~~` 围栏；Setext 标题（`===` / `---` 下划线）**不识别**，写进模块注释与 `SKILL.md` 规范说明。
  - 不引入任何 Markdown crate；不把 `serde_yaml_ng` 泄漏到 `agent-skill` 之外的 crate（判据 A-2 / `cargo tree -i`）。
- **承接任务**：P5-001、P5-003。

---

## D-P5-018 错误文案与脱敏

- **状态**：`DECIDED`
- **问题**：Skill 相关的错误文案长什么样？哪些信息可以出进程？
- **背景与证据**：P4 的口径是「文案单一产点 + 不泄漏内部细节」（D-P4-022）：错误 `Display` 给用户看，原始错误只进 `tracing::debug`；接口 400 的文案取自领域错误类型，接口层不另写。
- **可选方案**：
  1. 统一形如 `skills/<dir>/SKILL.md:<行号>: <原因>`，原因是一句中文，不带 YAML 原文；
  2. 直接透出解析器的原始错误（含 YAML 片段）；
  3. 只给「加载失败」四个字。
- **决策**：①。
- **理由**：
  - ②会把 frontmatter 片段（可能含内部信息）带到接口与客户端；`serde_yaml_ng` 的错误文本是英文且带片段，不适合直接展示。
  - ③让用户无从下手，与「可定位错误」的验收方向直接冲突。
- **约束**：
  - `error.rs` 是唯一构造点，`Display` 不含绝对路径、不含正文、不含 YAML 原文；原始错误进 `tracing::debug`。
  - 缺失工具、接口 400 文案分别只在 `availability.rs`、`manager.rs` 出现一次（判据 E-5）。
  - 日志里只记相对目录、状态与计数，不记正文与 frontmatter 全文。
- **承接任务**：P5-003、P5-006、P5-012、P5-018。

---

## D-P5-019 客户端形态：TUI 覆盖层 + WebUI 抽屉

- **状态**：`DECIDED`
- **问题**：Skill 在 TUI 与 WebUI 里怎么呈现？做到哪一层？
- **背景与证据**：
  - TUI 现有布局是「状态行 + 通知 + 会话列表 + 消息 + 输入 + 提示」，没有可扩展的侧栏；P4 的 MCP 管理没有进 TUI。
  - WebUI 目前只有聊天页（`pages/chat/`），页面结构简单，`antd` 6 + `@ant-design/x` 的抽屉与 Markdown 组件都已在依赖里。
  - 判据只要求「清单 / 状态 / 启停 / 重载 / 详情」，不要求编辑或新建。
- **可选方案**：
  1. TUI 覆盖层（`/skills` 或 `F2`）+ WebUI 右侧抽屉；
  2. TUI 新增常驻侧栏 + WebUI 新增独立页面路由；
  3. 本阶段只做接口，客户端留 P6。
- **决策**：①。
- **理由**：
  - ②在 TUI 里挤掉消息区（窄终端下不可用），WebUI 的独立页面还需要路由、导航与空态，收益只是「多一个入口」。
  - ③会让「当前加载了哪个 Skill」在客户端不可见，与 README §五 交付物 5（管理 API 与客户端入口）不符。
  - ①的覆盖层 / 抽屉都是「不打断对话」的形态，符合 Skill 的使用场景（用户多半是在对话中想起某个技能）。
- **约束**：
  - 只做「列出 / 查看正文 / 启停 / 重载」四件事，不做编辑、不做新建、不做删除（文件系统仍是唯一事实源）。
  - 客户端只调用生成的 SDK（`AGENTS.md` 第 22 条），不手写 API 定义。
  - 接口失败时展示一行可读错误，不阻塞聊天输入；`INVALID` 项的错误文案原样展示，不做二次翻译。
- **承接任务**：P5-014、P5-015、P5-019。

---

## D-P5-020 示例 Skill 的数据位置

- **状态**：`DECIDED`
- **问题**：示例 Skill（README §五 交付物 6）放在仓库哪里？用例怎么用？
- **背景与证据**：
  - `crates/*/examples/` 会被 cargo 当作 example 目标扫描（`.rs` 文件），把 Markdown 目录放进去容易与构建脚本混淆。
  - 用例的 `AGENT_HOME` 是临时目录（`util/paths`），示例需要被**复制**进去才能参与发现。
  - P4 的测试替身走的是「`bin-tests` 隐藏子命令」，那是可执行逻辑；Skill 是纯数据，不需要子命令。
- **可选方案**：
  1. `crates/agent-skill/assets/skills/<name>/SKILL.md`（含 `references/`），用例复制到临时 `AGENT_HOME/skills/`；
  2. 仓库根 `examples/skills/`；
  3. 用例内联字符串生成 `SKILL.md`，不提供示例文件。
- **决策**：①。
- **理由**：
  - ②的根目录 `examples/` 与 cargo 的约定重名，容易被误读成 crate example。
  - ③没有「真实示例」可交付，且长正文 / `references/` 场景在字符串里写会非常难读。
  - ①与 crate 同仓，`cargo package` 之外不参与构建；用例复制后即是一份普通 Skill 目录，和用户手写的一模一样。
- **约束**：
  - 三份示例：`pdf-report`（声明两个可用工具，正文三步）、`db-migrate`（声明一个不可用工具，用于降级路径）、`log-triage`（多章节长正文，用于 `section` 路径）。
  - 示例必须能被解析器原样接受（`skill.parse` 用例里直接解析这三份）。
  - 示例不写入任何真实 `AGENT_HOME`；测试不依赖仓库外的路径。
- **承接任务**：P5-019。

---

## D-P5-021 用例取证与 Mock 剧本的形态

- **状态**：`DECIDED`
- **问题**：Batch F 的十条 `skill.*` 用例怎么取证？`skill.e2e` 要求「先 `skill_load`、再按正文步骤 `file_read`、最后文本」，而 Mock LLM 当时一个 Run 只能演一轮工具调用。
- **背景与证据**：
  - 既有 Mock 剧本（P3-030）的关键字 `tool:{…}` 写在用户输入里，`calls` 只描述「这一轮」；第二轮起最后一条消息是工具结果，剧本不再被读到，于是「一轮之后转文本」。
  - 断言的可用数据源按 `AGENTS.md` 的采集条款只有：接口响应、客户端事件、`metrics` 快照与 Mock 的 `/mock/last-request`、`/mock/stats`；日志一律不作数据来源。
  - 预算类口径（`max_index_chars` / `max_inline_chars` 的边界、`shown + hidden`、50 个技能）要稳定的精确值，走服务端会引入不必要的磁盘与启动开销。
- **可选方案**：
  1. 保持单轮剧本，`skill.e2e` 只断言「同一轮里两个调用的发出顺序」；
  2. 给 Mock 增加逐轮剧本 `rounds`，`skill.e2e` 真正跨三轮（`skill_load` → `file_read` → 文本）；
  3. 为 e2e 单独起一个「按轮次回放」的替身进程。
- **决策**：②，并按「请求级断言用 Mock 的 `/mock/last-request`，纯函数级口径用进程内契约调用」分工取证。
- **理由**：
  - ①的「顺序」只是 Mock 脚本里的数组顺序，管不住「下一轮真的按步骤走」；③是第二个替身，成本与维护面都更大。
  - ②的最小改动是在既有剧本章节里增加 `rounds`：轮次 = 「剧本所在用户消息之后出现了几条 assistant 的 `tool_calls`」，旧 `calls` 单轮格式与调用标识 `call_mock_<n>` 逐字保留，`tool.*` 的 28 个用例复跑全绿。
  - 纯函数级口径（解析、发现、索引渲染、预算、加载文本）在进程内直接调同一份实现，断言的是精确值而不是「服务端跑一遍看起来对」；请求级口径（system 消息形态、`tools` 清单、工具结果、Run 终态）仍只认 Mock 与接口。
- **约束**：
  - 剧本 JSON 必须放在用户消息**末尾**（`Script::parse` 只解析关键字之后的部分）。
  - 逐轮剧本的轮次用完即转文本，不允许无限重复；`repeat` 只用于「每轮重复第一轮」的既有场景。
  - 平台受限的能力（Windows 创建符号链接需要权限）在用例里显式记「未采集：原因」，不得写成通过，也不得为了凑数换平台。
- **承接任务**：P5-016 ~ P5-020。
