# P5：Skills

**状态：未开始（仅阶段级规划，暂不拆任务）**

---

## 一、阶段目标

让 Agent 具备**可动态加载的任务知识**：遇到某类任务时，知道该怎么做、按什么步骤做、参考哪些资料。

Skill 与 Tool 的职责必须严格区分：

- **Skill 告诉 Agent 如何完成任务**（知识、步骤、约束、参考材料）。
- **Tool 提供实际执行能力**（真正做事的函数）。

---

## 二、主要能力

| 能力 | 说明 |
|------|------|
| Skill Discovery | 从约定目录发现可用 Skill（本地目录、用户目录，可扩展） |
| Skill Registry | 已发现 Skill 的注册、启用/禁用、按任务筛选 |
| SKILL.md | Skill 的正文载体（人类可读、模型可读），格式尽量兼容开放的 Agent Skills 格式 |
| YAML Metadata | frontmatter：名称、描述、适用场景、依赖工具、版本、作者等 |
| Skill Loader | 解析与校验（YAML + Markdown），失败时给出可定位错误 |
| Progressive Disclosure | 默认只暴露元数据摘要，按需加载正文与引用材料，控制上下文预算 |
| Skill References | Skill 附带的参考资料（文档、示例、模板）按需加载 |
| Skill Instructions | 加载后的指令如何注入上下文（位置、优先级、与系统提示的关系） |
| Skill 与 Tool 协作 | Skill 声明所需工具；工具不可用时给出明确降级说明 |
| Skill 管理 | API 与客户端：列出、查看、启用/禁用、查看加载状态 |

---

## 三、核心技术

- **Serde YAML**：frontmatter 解析；格式校验。
- **Markdown 解析**：正文与章节切分，支持按章节按需加载。
- **Context Budget**（P2）：渐进披露的预算控制，避免一次性塞满上下文。
- **Tool Registry**（P3）：Skill 与工具的关联与可用性检查。
- **文件系统**：Skill 目录发现、路径安全（禁止越权读取）。
- **API / SDK 生成**：Skill 管理接口与前端的 Skill 列表界面。

---

## 四、与前一阶段的依赖

| 依赖项 | 用途 |
|--------|------|
| P2 的 Context Builder / Budget | 渐进披露必须受上下文预算约束 |
| P3 的 Tool Registry | Skill 声明的工具依赖需要校验 |
| P4 的 MCP（可选） | MCP 提供的工具也可能被 Skill 依赖 |
| P2 的持久化 | Skill 启用状态与使用记录 |
| P1 的 SDK / SSE | 管理界面与“当前加载了哪个 Skill”的可视化 |

---

## 五、主要交付物

1. Skill 目录结构与格式规范（SKILL.md + YAML frontmatter）。
2. Discovery + Loader + Registry。
3. 渐进披露机制（元数据 → 正文 → 引用材料的分级加载）。
4. Skill 与 Tool 的依赖声明与可用性校验。
5. Skill 管理 API 与客户端入口（TUI / WebUI）。
6. 至少 2–3 个示例 Skill 作为规范验证。

---

## 六、阶段验收方向

1. 放入一个新 Skill 目录后，无需重启即可被发现（或按明确规则重载）。
2. Agent 能根据任务描述识别适用的 Skill，并按需加载正文，而不是把所有 Skill 全部塞进上下文。
3. 加载大体积 Skill 时上下文预算不被打穿（可验证的 token/字符占用）。
4. Skill 声明的工具不可用时，Agent 收到明确提示，不会“假装执行”。
5. 格式错误的 Skill 被拒绝加载，并给出可定位的错误（文件、行、原因）。
6. Skill 目录中的脚本**不会**因为位于该目录而获得执行权限（与 Tool 权限体系一致）。

---

## 七、Non-goals（本阶段明确不做）

- Skill 商店、远程下载与签名校验（后续议题）。
- Skill 沙箱执行（脚本执行仍走 P3/P6 的工具与沙箱体系）。
- 自动生成 Skill（从对话中提炼 Skill）。
- Skill 之间的继承、组合语言（先观察真实需求）。
- 多 Agent 场景下的 Skill 分配（P6）。

---

## 八、风险与待讨论问题

1. **格式兼容性**：开放的 Agent Skills 格式（SKILL.md + YAML frontmatter）与项目自定义扩展字段的边界，是否需要严格兼容或允许超集。
2. **Progressive Disclosure 的实现位置**：由 Agent 主动请求（如 `load_skill(name)` 工具）还是由系统在构建上下文时被动注入——两种方案对上下文预算和可预测性影响很大。
3. **注入位置**：Skill 指令放在系统提示、首条用户消息、还是独立消息段；不同供应商模型对位置敏感度不同。
4. **目录安全**：Skill 目录可能出现在工作区内（用户项目自带 Skill）；读取范围与信任级别如何界定。
5. **“脚本不自动获得执行权限”的落地**：需要明确声明式依赖（工具白名单）与运行时校验点。
6. **性能**：大量 Skill 时的发现开销与缓存策略。
7. **与 Memory 的边界**（P6）：Skill 是静态知识，Memory 是动态积累，避免两者互相污染。
