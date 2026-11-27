# skill 接口（Skill 管理）

Skill 的运行期管理面：看有哪些技能、某个技能为什么不可用、读取它的正文，以及启停与重载。P5 起由 `agent-skill` 的注册表提供进程内快照，本领域只暴露只读快照、一份详情与一个动作入口；Skill 仍是**纯文本知识**，不注册工具、不提供执行路径（`.steps/p5/decisions.md` D-P5-001 / D-P5-013）。

- 定义位置：`crates/lib-web/src/skill/api.rs`（handler）、`crates/lib-web/src/skill/mod.rs`（错误映射）、`crates/agent-skill/src/vo.rs`（`SkillVO` / `SkillDetailVO` / `SkillGetParams` / `SkillUpdatePO` 与两个枚举），契约类型经 `manager-agent` 转发
- 业务入口：`manager-agent::skill_list` / `skill_get` / `skill_update` → `agent-skill` 的 `manager`（清单 / 详情 / 动作口径）与 `SkillRegistry`（快照、启用状态、上限）；`RELOAD` 的重扫由 `manager-agent` 执行（它持有应用根目录）
- 决策：D-P5-015（三条接口、`update` 返回整张清单）、D-P5-008（重载语义）、D-P5-009（工具可用性在渲染 / 加载时判定）、D-P5-018（错误文案与脱敏）

## 接口清单

| 方法 | path | 入参 | 返回 | 说明 |
|---|---|---|---|---|
| GET | `api/skill/list` | 无 | `SkillVO[]` | 全部 Skill（含 `INVALID` 与已禁用项），按 `name` 升序 |
| GET | `api/skill/get` | `SkillGetParams`（查询参数） | `SkillDetailVO` | 一条 Skill 的清单字段 + 正文 + 引用清单 |
| POST | `api/skill/update` | `SkillUpdatePO`（请求体） | `SkillVO[]` | 三个动作都返回**动作完成后**的整张清单 |

接口总数由 29 增至 32（新增 3 条），见 [readme.md](readme.md)。

## 入参

| 类型 | 字段 | 位置 | 必填 | 说明 |
|---|---|---|---|---|
| `SkillGetParams` | `name` | query | 是 | 目标 Skill 的逻辑标识；缺失或类型不符由框架返回 400 |
| `SkillUpdatePO` | `name` | body | 否 | `ENABLE` / `DISABLE` 必填，`RELOAD` 必须缺省或 `null`（空串按缺省处理） |
| `SkillUpdatePO` | `action` | body | 是 | `ENABLE` / `DISABLE` / `RELOAD` |

## 载荷类型

### SkillStatusKind

可用性状态，两个取值；与注册表的 `SkillStatus`、索引与加载的判定口径一致。

| 取值 | 含义 |
|---|---|
| `READY` | 解析成功：进索引、可加载 |
| `INVALID` | 解析失败或同名冲突：进清单、带 `error`，不进索引、不可加载 |

### SkillAction

动作取值，三个；`ENABLE` / `DISABLE` 幂等（重复调用结果相同），`RELOAD` 两次连续调用的清单逐字段相等。

| 取值 | 语义 |
|---|---|
| `ENABLE` | 启用一条 Skill：它重新进索引、可被 `skill_load` 加载 |
| `DISABLE` | 禁用一条 Skill：立即从索引消失、不可加载，但仍在清单里 |
| `RELOAD` | 重扫发现目录并整体替换快照：新增、删除、改名、格式修正都在这一次内生效；`disabled` 名单保留（只保留仍然存在的名字） |

### SkillVO

一条 Skill 的清单快照。

| 字段 | 类型 | 说明 |
|---|---|---|
| `name` | `string` | 逻辑标识（frontmatter 的 `name`；`INVALID` 条目回落到目录名，只为定位用） |
| `description` | `string` | frontmatter 的描述（`INVALID` 条目为空串） |
| `status` | `SkillStatusKind` | 可用性状态 |
| `enabled` | `boolean` | 进程内的启用标记（`skills.toml` 的 `disabled` 名单与接口动作的结果） |
| `requiresTools` | `string[]` | frontmatter 声明的工具，保持原样顺序 |
| `missingTools` | `string[]` | 声明里**此刻**不可用的工具（未注册、权限 `deny`，顺序同 `requiresTools`） |
| `directory` | `string` | 相对应用根目录的目录路径（`skills/<dir>`），不含绝对路径 |
| `referenceCount` | `number` | `references/` 下的引用清单条数 |
| `error` | `string \| null` | `INVALID` 的可定位原因（`skills/<dir>/SKILL.md:<行号>: <原因>`）；`READY` 时为 `null` |

### SkillDetailVO

清单字段（与 `SkillVO` 逐个相同，取自同一次读取，因此详情与清单不会出现「元数据与正文不一致」的窗口）之后补两个字段。

| 字段 | 类型 | 说明 |
|---|---|---|
| `name` | `string` | 逻辑标识，同 `SkillVO.name` |
| `description` | `string` | frontmatter 的描述 |
| `status` | `SkillStatusKind` | 可用性状态 |
| `enabled` | `boolean` | 进程内的启用标记 |
| `requiresTools` | `string[]` | frontmatter 声明的工具 |
| `missingTools` | `string[]` | 声明里此刻不可用的工具 |
| `directory` | `string` | 相对应用根目录的目录路径 |
| `referenceCount` | `number` | `references/` 下的引用清单条数 |
| `error` | `string \| null` | `INVALID` 的可定位原因 |
| `body` | `string` | 正文原样文本；禁用或 `INVALID` 时为空串 |
| `references` | `string[]` | 引用清单（相对**应用根目录**的路径，形如 `skills/<dir>/references/<rel>`）；读取交给既有 `file_read` 工具，本接口只给清单 |

说明：

1. `missingTools` 在**每次读取时**现场判定：MCP 在运行期连上工具之后，下一次 `list` / `get` 的缺失标记自动消失，不需要 `RELOAD`（D-P5-009）。缺工具**不阻断**加载，只做提示。
2. `INVALID` 与已禁用项照样出现在清单与详情里：管理面看到的是「目录里有什么」，不是「什么能用」。`INVALID` 的原因取自解析期或同名冲突，只含相对路径与行号，不含正文与 YAML 原文（D-P5-018）。
3. 清单与索引同源：`READY && enabled` 的条目才进系统提示的 `## 可用 Skill` 小节，正文字符一个都不进系统提示（D-P5-002）。
4. 状态与启用标记都是进程内的易失事实，不做持久化：重启后回到 `skills.toml`（`root`、`disabled` 名单）与磁盘现状。
5. `references` 的路径基准是**应用根目录**，与正文里的写法不同：示例 Skill 正文写的 `references/<rel>` 是相对技能目录的缩写，不能直接当 `file_read` 的入参；读取一律用清单里的路径（D-P5-012）。

## 请求与响应样例

```text
GET api/skill/list

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":[{"name":"broken","description":"","status":"INVALID","enabled":true,"requiresTools":[],"missingTools":[],"directory":"skills/broken","referenceCount":0,"error":"skills/broken/SKILL.md:2: frontmatter 缺少 name"},{"name":"db-migrate","description":"迁移检查与回滚预案","status":"READY","enabled":true,"requiresTools":["sqlite3"],"missingTools":["sqlite3"],"directory":"skills/db-migrate","referenceCount":1,"error":null},{"name":"pdf-report","description":"生成 PDF 报表：给定指标与时间范围，产出图表与结论文档。","status":"READY","enabled":false,"requiresTools":["file_read","file_search"],"missingTools":[],"directory":"skills/pdf-report","referenceCount":0,"error":null}]}
```

```text
GET api/skill/get?name=db-migrate

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":{"name":"db-migrate","description":"迁移检查与回滚预案","status":"READY","enabled":true,"requiresTools":["sqlite3"],"missingTools":["sqlite3"],"directory":"skills/db-migrate","referenceCount":1,"error":null,"body":"# 步骤\n\n1. 检查当前版本……\n","references":["skills/db-migrate/references/rollback.md"]}}
```

```text
POST api/skill/update
{"name":"pdf-report","action":"DISABLE"}

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":[{"name":"broken","description":"","status":"INVALID","enabled":true,"requiresTools":[],"missingTools":[],"directory":"skills/broken","referenceCount":0,"error":"skills/broken/SKILL.md:2: frontmatter 缺少 name"},{"name":"db-migrate","description":"迁移检查与回滚预案","status":"READY","enabled":true,"requiresTools":["sqlite3"],"missingTools":["sqlite3"],"directory":"skills/db-migrate","referenceCount":1,"error":null},{"name":"pdf-report","description":"生成 PDF 报表：给定指标与时间范围，产出图表与结论文档。","status":"READY","enabled":false,"requiresTools":["file_read","file_search"],"missingTools":[],"directory":"skills/pdf-report","referenceCount":0,"error":null}]}
```

`RELOAD` 不带 `name`，返回值就是重扫后的整张清单：

```text
POST api/skill/update
{"action":"RELOAD"}

HTTP/1.1 200 OK
{"code":200,"message":"Success","data":[...]}
```

## 错误响应

| 场景 | HTTP | `message` |
|---|---|---|
| `name` 查询参数缺失或类型不符 | 400 | 框架固定文案（`查询参数转换失败`） |
| `action` 缺失或不在三个取值内 | 400 | 框架固定文案（`请求体不是有效的 JSON`） |
| `get` 的 `name` 不在清单里（含功能未启用、门面未装配） | 400 | 固定 `技能不存在` |
| `ENABLE` / `DISABLE` 没给 `name` | 400 | 固定 `技能更新缺少 name` |
| `RELOAD` 带了 `name` | 400 | 固定 `RELOAD 不接受 name` |
| 名字在清单里但正文取不出来（`INVALID` 与已禁用项不算：它们走 200 + 空正文 + `error` / `enabled`） | 400 | `技能加载失败：<原因>` |
| 内部失败 | 500 | 固定 `服务器内部错误`，细节只进日志 |

未知 `name` 是 400 而不是 404：它是请求参数里的标识，与 MCP 的 `serverId`、运行时的「客户端未注册」同一套判定口径（客户端按 `message` 识别）。

## 与配置的关系

`skills.toml` 是唯一持久化来源（D-P5-007）：

- `root` 决定发现目录（相对应用根目录解析，绝对路径按原样）；`disabled` 是启动即禁用的名字清单。
- `ENABLE` / `DISABLE` 只改进程内状态，不写配置、不入库；重启后回到 `disabled` 名单。
- `RELOAD` 重扫磁盘：新增、删除、改名、格式修正都在一次调用内生效；`disabled` 名单保留（只保留仍然存在的名字）。
- `enabled = false` 时：不发现、不注册 `skill_load`、不注入索引，`api/skill/list` 返回空清单，`api/skill/get` 与 `ENABLE` / `DISABLE` 对任何 `name` 返回 400 `技能不存在`，`RELOAD` 是空操作（返回空清单）。
