# P1-005 TypeScript SDK 生成链路验证报告

本报告是 P1-005 的过程交付物。结论分两类，逐条标注：

- **实测**：实际执行生成命令、检查生成物内容得到的结论。
- **待验证**：尚未执行的验收项，明确写出阻塞原因。

---

## 1. 结论摘要

| 问题 | 结论 | 依据 |
|---|---|---|
| 生成链路是否成立 | **成立**。一条命令从 `api_metadata_iter()` 产出完整 npm ESM 包 | 实测 |
| 生成物落位 | `packages/sdk-ts/`（`package.json` + `dist/*.js` + `dist/*.d.ts`） | 实测 |
| 是否入库 | **不入库**，`packages/` 整体被 `.gitignore` 忽略（D-006 已修订） | 实测 |
| 前端引入方式 | `lingting-agent-ui/pnpm-workspace.yaml` 的 `../packages/*`，WebUI 侧用 `"@lingting/agent-api": "workspace:*"` | 脚手架已落地（P1-016 待接入调用） |
| 重复生成是否幂等 | **幂等**，两次生成的全部文件 md5 一致 | 实测 |
| `i64` 映射 | `auto_type` 字段在 TS 侧标为 `string`，wire 上也是字符串，标注与运行时一致 | 实测 + D-018 |
| 生成物类型检查（`tsc --noEmit`） | **通过**：在 `lingting-agent-ui/` 内用该工程的 typescript 检查生成物 `dist/*.d.ts`，无类型错误 | 实测 |

---

## 2. 实现形态

| 位置 | 作用 |
|---|---|
| `crates/lib-web/examples/gen_ts_sdk.rs` | 生成器（临时形态，最终形态归 P1-007） |
| `lingting-agent-ui/pnpm-workspace.yaml` | pnpm workspace 声明，`packages` 指向 `../packages/*` |
| `scripts/gen.sh` | 一条命令同时产出 TS 与 Rust 两份 SDK |

生成器要点：

1. 先调用 `lib_web::web_routes()`：作为**链接锚点**，否则 `inventory` 注册项被链接器丢弃，元数据为空。
2. 三个构建器分工：`TypescriptTypeBuilder`（DTO 类型）、`TypescriptEnumBuilder`（枚举运行时表）、`TypescriptApiBuilder`（`ApiDefinitions` + 抽象类 `ApiClient`）。
3. `PackageBuilder::new("@lingting/agent-api", <workspace 版本>, "packages/sdk-ts")` 落盘；版本取自 `CARGO_PKG_VERSION`（26.11.0），与 workspace 一致。
4. 元数据为空时直接返回错误（非 0 退出），不写半成品。

生成命令：

```text
cargo run -p lib-web --example gen_ts_sdk
```

---

## 3. 生成物结构（实测）

```text
packages/sdk-ts/
  package.json          # name/@lingting/agent-api, type=module, main+types+exports
  dist/
    index.js|d.ts       # export * from "./types.js" / "./enums.js" / "./api.js"
    types.js|d.ts       # DTO 类型声明
    enums.js|d.ts       # 枚举 <Name>All / <Name>Map
    api.js|d.ts         # ApiDefinitions + 抽象类 ApiClient
```

`package.json` 的 `exports` 同时暴露根入口与 `./types`、`./enums`、`./api` 子入口，`moduleResolution: bundler`
的消费方可以直接按子路径 import。

接口集合（与探针一致，共 7 条）：

| 方法名（TS camelCase） | 元数据来源 |
|---|---|
| `probeVoid` / `probeEcho` / `probeQuery` / `probeTyped` / `probeFail` / `probeResponse` / `probeResponseOf` | `crates/lib-web/src/probe/api.rs` |

生成物只提供**抽象类**：`protected abstract call<T>(method, path, body?, query?): Promise<T>`，实际 HTTP
Transport 由 WebUI 自己实现（D-002 的既有约定）。

---

## 4. 类型映射与已知差异

| 服务端 Rust 类型 | TS 生成结果 | 说明 |
|---|---|---|
| `i64`（`auto_type` 字段） | `string` | 与 `serde_as(DisplayFromStr)` 的 wire 形态一致，无“标 string 实际 number”的隐患 |
| `Option<T>` | `T \| null` | 与 `serde(default)` 一致 |
| `Vec<T>` | `T[]` | — |
| `WebResponse`（元数据 `Blob`） | `Blob` | 拿不到自定义响应头，只能取二进制体 |
| `WebResponseOf<T>` | `T` | 自定义响应头在 TS 侧同样丢失 |

---

## 5. 幂等性

连续执行两次 `scripts/gen.sh`，`packages/sdk-ts/package.json` 与 `dist/*` 全部文件 md5 一致。
构建器内部按接口名 / 类型名排序，元数据的 `inventory` 迭代顺序不影响产物。

---

## 6. 未覆盖 / 待后续

1. **`tsc --noEmit` 已补做（回填）**：前端工程 `lingting-agent-ui/` 落地后，用该工程的 typescript 依赖检查生成物 `dist/*.d.ts`，无类型错误；
   `workspace:*` 引入亦已实测（`import "@lingting/agent-api"` 可解析，`ApiClient` / `ApiDefinitions` 正常导出）。
   这是 P1-005 原先唯一未通过的验收项，现已通过。
2. **消费方调用验证未做**：前端脚手架 `lingting-agent-ui/` 已声明 `"@lingting/agent-api": "workspace:*"`，但尚未在页面中实际调用接口，属 P1-016 范围。
3. **错误分支验证未做**：方法名重复、`method + path` 重复、`class_name` 非法的报错信息未构造用例验证（框架已实现校验，
   属于框架行为，可在 P1-007 固化时补）。
4. **生成物头部标记**：`PackageBuilder` 不支持写文件头注释，TS 侧目前没有“自动生成，勿手改”标记；
   需要框架扩展或生成后处理，记入 P1-007。
