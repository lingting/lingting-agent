# P1-006 Rust Client SDK 生成验证报告

本报告是 P1-006 的交付物。结论分三类，逐条标注：

- **实测**：实际执行生成 / 构建命令得到的结论。
- **源码核实**：只阅读框架源码得出的结论。
- **未实现**：能力缺口与降级方案，不静默放弃。

---

## 1. 结论摘要

| 问题 | 结论 | 依据 |
|---|---|---|
| Rust 生成能力在哪 | **不在独立 crate**：本地框架已移除 `framework-proc-ts`，并把 TS/Rust 生成器一起并入 `framework-proc-core` 的 `ts` / `rust` 特性（`RustApiBuilder`、`RustTypeBuilder`、`RustEnumBuilder`、`CrateBuilder`） | 源码核实 |
| 生成形态 | `ApiDefinition` 常量表 + `ApiClient` trait（`send` 必需方法 + 每个接口一个默认方法） | 实测 |
| 生成物落位 | `packages/sdk-rust/src/{types,enums,api}.rs`，**不生成 `Cargo.toml`**；`lib-bin-cli` 用 `#[path]` 引入 | 实测 |
| 是否入库 | 不入库（D-006 已修订） | 实测 |
| `cargo build` 是否通过 | 通过，无手写补丁、无告警 | 实测 |
| 与 TS 接口集合是否一致 | 一致：7 个接口，同源同一份 `ApiMetadata`（TS camelCase / Rust SCREAMING_SNAKE 同标识符） | 实测 |
| 重复生成是否幂等 | 幂等，两次生成 md5 一致 | 实测 |
| DTO 策略 | **(b) 自带快照定义**（`RustTypeBuilder` 从 `auto_type` 定义快照生成），客户端不依赖服务端 crate | 实测 |
| reqwest Transport 实现 | **未实现**：框架只产出 trait，Transport 由客户端实现（见第 6 节降级方案） | 未实现 |

---

## 2. 生成器的形态选择

P1-006 原计划的「新增 `framework-proc-rust` crate」已由上游实现替代（`framework-proc-core` 的 `rust` 特性）。
真正需要决定的是**生成器放在哪、产物放哪、客户端怎么消费**。逐个方案都实测过：

| 方案 | 结论 | 实测证据 |
|---|---|---|
| A. `lib-bin-cli/build.rs` 构建期生成到 `target/<profile>/gen_rust` + `include!` | **弃用** | 构建期写入 `target/` 会与「生成物不入库」叠加出「必须先构建才能构建」的语义混乱；且 cargo 对 `target/` 下文件的清理无感知 |
| B. `lib-web/build.rs` 里生成 | **不可行** | 实测：`[build-dependencies]` 指向自己 → `error: cyclic package dependency: package lib-web depends on itself`。build.rs 是独立 host 程序，拿不到本 crate 的 `inventory` 元数据 |
| C. `lib-bin-cli` path 依赖生成的 `packages/sdk-rust` crate | **不可行（引导死锁）** | 实测：加上 `sdk-rust = { path = "../../packages/sdk-rust" }` 而目录不存在时，**整个 workspace 加载失败**，连生成器本身（`cargo run -p lib-web --example ...`）都跑不起来 |
| D. **`lib-web` 的 example 生成 + `lib-bin-cli` 用 `#[path]` 引入（采用）** | 可行 | 生成后 `cargo build` 通过；缺生成物时报错明确（`couldn't read .../packages/sdk-rust/src/api.rs`），不污染 workspace 加载 |

采用 D 的原因：

1. example 是真正的二进制，链接 `lib-web` 后能读到 `inventory` 元数据（与 TS 侧同一套机制）。
2. 不生成 `Cargo.toml` ⇒ 不引入「manifest 先于生成存在」的引导死锁。
3. `#[path]` 让生成文件成为 `lib-bin-cli` 的真实模块，因此 `cargo fmt --check` 与 `clippy -D warnings` 能覆盖到生成代码。

---

## 3. 生成物结构（实测）

```text
packages/sdk-rust/src/
  types.rs    # DTO 定义快照（pub struct / pub enum + serde / serde_as 属性）
  enums.rs    # 枚举运行时信息：<Name> 结构体 + <NAME>_ALL / <NAME>_MAP / 强类型辅助函数
  api.rs      # ApiArgumentKind / ApiArgument / ApiDefinition + 每接口常量 + ApiCallError + ApiClient trait
```

`lib-bin-cli/src/sdk.rs` 的引入方式：

```rust
#[path = "../../../packages/sdk-rust/src/api.rs"]
pub mod api;
#[path = "../../../packages/sdk-rust/src/types.rs"]
pub mod types;
#[path = "../../../packages/sdk-rust/src/enums.rs"]
pub mod enums;

pub use api::*;
pub use types::*;   // enums 不参与 glob 再导出，避免与强类型枚举同名冲突
```

生成的 trait 形态（实测输出）：

```rust
pub trait ApiClient {
    type Error: std::error::Error + Send + Sync + 'static;

    fn send(&self, definition: &ApiDefinition, body: Option<Vec<u8>>, query: Option<Vec<u8>>)
        -> impl std::future::Future<Output = Result<Vec<u8>, Self::Error>> + Send;

    fn probe_echo(&self, echo: ProbeEchoPO)
        -> impl std::future::Future<Output = Result<ProbeEchoVO, ApiCallError<Self::Error>>> + Send
    where Self: Sync { /* 参数聚合 → send → 反序列化 */ }
}
```

返回值分三态：`Void → ()`、`Blob → Vec<u8>`、`Type → 反序列化后的 DTO`。

---

## 4. 生成文件头与 lint 策略

生成文件头固定为：

```rust
// 本文件由 `cargo run -p lib-web --example gen_rust_sdk` 自动生成，请勿手动修改。
// 服务端 API 定义变更后重新执行生成命令；生成物不入库。

#![allow(warnings)]
```

- `#![allow(warnings)]` 是模块级内层属性，覆盖该文件内全部 rustc / clippy 告警，且**优先级高于命令行 `-D warnings`**
  （实测：往生成文件里塞未使用变量后 `cargo clippy -p lib-bin-cli --all-targets -- -D warnings` 仍为 exit 0）。
- 它只压告警，不压编译错误：生成代码真的编译不过时构建照样失败。
- 格式化不靠属性：生成器内部调用 `rustfmt`，因为 `#[path]` 引入的文件在 `cargo fmt --check` 检查范围内。

---

## 5. 类型还原与已知差异

| 服务端 Rust 类型 | 生成结果 | 说明 |
|---|---|---|
| `i64`（`auto_type`） | 仍为 `i64` + `#[serde_as(as = "serde_with::DisplayFromStr")]` | 与 TS 侧 `string` 是**类型系统差异**，不是契约不一致；wire 上双方都是字符串 |
| `Option<T>` | `Option<T>` | 与 TS 侧 `T \| null` 对应 |
| `Vec<T>` | `Vec<T>` | 与 TS 侧 `T[]` 对应 |
| `HashMap<K,V>` / `BTreeMap<K,V>` | 原样保留 | TS 侧是 `Record<K,V>` |
| `WebResponse`（`Blob`） | `Vec<u8>` | 自定义响应头丢失，与 TS 侧 `Blob` 同源缺陷 |

`types.rs` 除探针 DTO 外还包含 `lib-web` 链接到的框架类型（`R`、`RCodeKind`、`PaginationParams`、
`PaginationResult`、`PaginationSort`、`IdPO`、`TimeMillisVO`）。是否需要按 namespace 过滤，归 P1-007。

---

## 6. 未实现能力与降级方案

| 能力 | 现状 | 降级方案 |
|---|---|---|
| reqwest Transport 实现 | 框架只生成 `ApiClient` trait 的 `send` 抽象，**没有**生成 reqwest 版本 | 在 `lib-bin-cli` 里手写一个实现 `send` 的 Transport（reqwest 封装、超时、重试、SSE 解析），属于客户端职责，与 `docs/workspace.md`「客户端只允许自实现 HTTP Transport」一致；不阻塞 SDK 生成链路 |
| 流式返回（SSE） | 元数据只有 `Void` / `Blob` / `Type` 三类，Stream 未建模 | 与 TS 侧完全一致；SSE 的类型化支持仍是 D-007 的开放项，退化方案是客户端自行处理 `Vec<u8>` 流 |
| 路径参数（`/api/x/{id}`） | 框架 `WebRouter` 按 method + path 精确匹配，不支持占位符 | 资源 id 一律走 body / query（D-008 方案 1），非生成器能力缺口 |
| 生成物头部标记 | 已实现（见第 4 节） | — |

---

## 7. 框架侧改动（`lingting-rust-framework`，未提交）

验证过程中发现并修复了 `framework-proc-core` 的一处生成缺陷：

- 位置：`framework-proc-core/src/rust/api_trait.rs`。
- 问题：`ApiReturnKind::Blob` 分支生成的代码是 `self.send(..).await.map_err(..)?`，缺 `Ok(..)` 包装，
  探针 `probe_response`（返回 `WebResponse`）的默认方法因此**编译失败**。
- 修法：把 `?` 从公共的 `send` 表达式里移到各分支（`Void` / `Type` 分支加 `?`，`Blob` 分支直接返回 `Result`），
  既修复编译错误，也避免 `Ok(..?)` 触发 clippy `needless_question_mark`。

另有一处适配性变更：本地框架已删除 `framework-proc-ts` crate，Rust/TS 生成器统一挂在 `framework-proc-core`
的 `ts` / `rust` 特性下；本项目相应移除了 `framework-proc-ts` 依赖，`lib-web` 改为
`framework-proc-core = { features = ["collect", "ts", "rust"] }`。

---

## 8. 幂等性与回归

- `scripts/gen.sh` 连续执行两次，`packages/sdk-rust/src/*.rs` md5 一致。
- `cargo build` 通过，0 warning；`scripts/check.sh`（`cargo fmt --all --check` + `cargo clippy --workspace --all-targets -- -D warnings`）通过。
- 删除 `packages/` 后 `cargo build` 的失败信息明确指向缺失的生成文件，跑一次 `scripts/gen.sh` 即恢复。
- 本任务未执行任何测试（按仓库约定，未经授权不运行测试）。
