# Symphony (Rust)

Coding agent 编排服务 — Rust 实现。

通过 Obsidian vault 跟踪 issue，自动分配 Gemini CLI agent 执行编码任务，并提供 HTTP API + React 仪表盘进行实时监控。

## 系统概览

```
┌─────────────────────────────────────────────────────────┐
│                    Symphony 服务                         │
│                                                          │
│  ┌──────────┐    ┌─────────────┐    ┌────────────────┐  │
│  │ Obsidian │───▶│ Orchestrator│───▶│  Agent Runner   │  │
│  │ Tracker  │    │  (Poll Loop)│    │  (ACP / stdio)  │  │
│  └──────────┘    └──────┬──────┘    └───────┬────────┘  │
│                         │                    │           │
│                  ┌──────▼──────┐    ┌───────▼────────┐  │
│                  │  Workspace  │    │   Gemini CLI    │  │
│                  │  Manager    │    │   (子进程)       │  │
│                  └─────────────┘    └────────────────┘  │
│                                                          │
│  ┌──────────────────────────────────────────────────┐   │
│  │  Axum HTTP Server                                 │   │
│  │  /api/v1/state  /api/v1/:id  /api/v1/refresh     │   │
│  │  + React Dashboard (SPA) / Fallback HTML          │   │
│  └──────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────┘
```

**工作流程**: 周期性轮询 Obsidian vault → 发现 active issue → 创建隔离 workspace → 渲染 Liquid prompt → 通过 ACP (JSON-RPC over stdio) 启动 Gemini CLI agent → 流式处理事件/token → 自动重试/回退 → 状态变更时清理。

## 前置要求

- [Rust](https://rustup.rs) (cargo, 1.75+)
- [Node.js](https://nodejs.org) + npm（前端，可选）
- [Gemini CLI](https://github.com/google-gemini/gemini-cli)（Agent 执行，运行时需要）

## 快速启动

```bash
# 同时启动后端 + 前端 dev server
./start.sh

# 仅启动后端
./start.sh backend

# 仅启动前端 dev server
./start.sh frontend

# 生产构建（编译后端 + 前端静态文件）
./start.sh build

# 仅检查编译，不运行
./start.sh check
```

### 环境变量

| 变量 | 说明 | 默认值 |
|------|------|--------|
| `WORKFLOW_PATH` | WORKFLOW.md 路径 | `./WORKFLOW.md` |
| `PORT` | HTTP 服务端口 | WORKFLOW.md 中的 `server.port` |
| `RUST_LOG` | 日志级别 (tracing) | `info` |

### 示例

```bash
# 指定端口启动后端
PORT=8080 ./start.sh backend

# 开启 debug 日志
RUST_LOG=debug ./start.sh

# 使用自定义 workflow 文件
WORKFLOW_PATH=~/my/WORKFLOW.md ./start.sh
```

> 启动后按 `Ctrl+C` 可优雅退出所有进程。

---

## 项目架构

```
rust/
├── Cargo.toml              # 依赖管理 (tokio, axum, serde, liquid, notify, clap...)
├── start.sh                # 启动脚本 (支持 backend/frontend/build/check)
├── WORKFLOW.md             # 编排配置 + Agent prompt 模板
│
├── src/
│   ├── main.rs             # 入口: CLI 解析 + 结构化 JSON 日志初始化
│   ├── models.rs           # 核心领域模型
│   ├── workflow.rs         # WORKFLOW.md 解析器
│   ├── config.rs           # 类型化配置层
│   ├── prompt.rs           # Liquid 模板渲染
│   ├── tracker/
│   │   ├── mod.rs          # Tracker trait 定义
│   │   └── obsidian.rs     # Obsidian vault 适配器
│   ├── workspace.rs        # 工作空间管理
│   ├── acp.rs              # ACP 协议类型
│   ├── agent_runner.rs     # Agent 会话管理
│   ├── orchestrator.rs     # 核心编排逻辑 ⭐
│   └── server.rs           # HTTP API + Dashboard
│
└── frontend/               # React 前端 (Vite + TypeScript)
    ├── src/
    │   ├── api/
    │   │   ├── types.ts    # TypeScript 类型 (映射 Rust API)
    │   │   └── client.ts   # API 客户端 (fetch)
    │   ├── App.tsx         # Dashboard 主组件
    │   └── index.css       # 暗色主题样式
    └── dist/               # 生产构建输出 (由 Axum 静态服务)
```

## 模块说明

### `models.rs` — 核心领域模型 (SPEC §4.1)

定义所有领域类型：

| 类型 | 说明 |
|------|------|
| `Issue` | 从 Obsidian 解析的 issue，含 id/title/state/priority/labels/blocked_by |
| `WorkflowDefinition` | WORKFLOW.md 解析结果：YAML config + prompt template |
| `ServiceConfig` | 完整服务配置 (tracker/polling/workspace/hooks/agent/gemini/server) |
| `RunningEntry` | 运行中会话的状态快照 (tokens/pid/events/cancel_token) |
| `RetryEntry` | 重试队列条目 (attempt/due_at/error) |
| `GeminiTotals` | 全局 token 消耗统计 |

辅助函数：`sanitize_workspace_key()` (路径安全清洗) 和 `normalize_state()` (状态归一化)。

### `workflow.rs` — WORKFLOW.md 解析 (SPEC §5.1–§5.5)

解析 YAML front matter + Markdown body：

```markdown
---
tracker:
  kind: obsidian
  vault_dir: /path/to/vault
polling:
  interval_ms: 30000
---
你是一个编码 agent，正在处理 issue: {{ issue.title }}
请在 workspace 中完成以下任务...
```

- `---` 分隔的 YAML front matter 解析为配置
- 剩余部分作为 Liquid prompt 模板
- 缺少 front matter 时整个文件作为 prompt body

### `config.rs` — 类型化配置层 (SPEC §5.3, §6)

| 功能 | 说明 |
|------|------|
| 默认值 | `interval_ms: 30000`, `max_concurrent_agents: 10`, `command: "gemini"` 等 |
| `$VAR` 解析 | `vault_dir: $OBSIDIAN_VAULT` → 环境变量展开 |
| `~` 展开 | `root: ~/workspaces` → 绝对路径 |
| 配置验证 | tracker.kind 必填、vault_dir 必填、command 非空 |
| 逗号列表 | `active_states: "Todo, In Progress"` → 数组 |

### `tracker/` — Issue 跟踪器 (SPEC §11)

- **`mod.rs`**: `Tracker` trait — `fetch_candidate_issues()`, `fetch_issues_by_states()`, `fetch_issue_states_by_ids()`
- **`obsidian.rs`**: Obsidian vault 适配器，扫描 `.md` 文件，解析 YAML frontmatter 中的 status/title/priority/labels/blocked_by

### `workspace.rs` — 工作空间管理 (SPEC §9)

- 按 issue identifier 创建隔离目录，key 经过 `sanitize_workspace_key()` 清洗
- 路径安全校验：workspace 必须在 root 目录内
- 生命周期 hooks：`after_create` → `before_run` → `after_run` → `before_remove`
- Hook 通过 `bash -lc` 执行，带超时控制

### `acp.rs` — Agent Communication Protocol (SPEC §10)

- JSON-RPC 2.0 消息类型 (request/response/notification)
- Token 使用量提取 (`thread.tokenUsage`, `total_token_usage`, `usage`)
- Rate limit 信息解析
- `obsidian_markdown_updater` 内置工具实现 (修改 issue 状态/追加内容)

### `agent_runner.rs` — Agent 会话 (SPEC §10.7, §16.5)

1. 通过 `bash -lc` 启动 Gemini CLI 子进程
2. ACP 初始化握手 (`initialize` → `notifications/initialized`)
3. 发送 prompt 作为 `tasks/send` turn
4. 流式读取 stdout，处理 JSON-RPC 消息
5. 支持工具调用 (`tools/call`) 代理
6. Turn timeout / 取消 / 异常退出处理

### `prompt.rs` — 模板渲染 (SPEC §12)

使用 [Liquid](https://shopify.github.io/liquid/) 模板引擎：

```liquid
Issue: {{ issue.title }}
Priority: {{ issue.priority }}
Labels: {{ issue.labels | join: ", " }}
{% if attempt %}Retry attempt #{{ attempt }}{% endif %}
```

变量自动注入：`issue` (Issue 完整对象)、`attempt` (重试次数)。

### `orchestrator.rs` — 核心编排 (SPEC §7, §8, §16) ⭐

最核心的模块，实现完整的状态机：

```
                        Poll Tick (每 interval_ms)
                              │
              ┌───────────────▼───────────────┐
              │    1. 存活检测 & 状态刷新        │
              │    2. 验证 dispatch 配置         │
              │    3. 拉取 candidate issues     │
              │    4. 优先级排序                 │
              │    5. 分发符合条件的 issue        │
              └───────────────────────────────┘
                              │
            ┌─────────────────▼─────────────────┐
            │         Worker (tokio::spawn)       │
            │  workspace → hook → prompt → agent  │
            └──────────┬──────────┬──────────────┘
                       │          │
              正常退出  │          │ 异常退出
                       ▼          ▼
              continuation    exponential
              retry (1s)    backoff retry
```

**关键特性**：
- **并发控制**: 全局 `max_concurrent_agents` + 按状态 `max_concurrent_agents_by_state`
- **调度优先级**: priority ASC → created_at ASC → identifier ASC
- **Blocker 检查**: Todo 状态的 issue 检查所有 blocker 是否已 terminal
- **Stall 检测**: 超过 `stall_timeout_ms` 无 ACP 活动则取消并重试
- **动态重载**: 文件监听 WORKFLOW.md 变更，热重载配置 (保持运行中会话)
- **启动清理**: 启动时清除 terminal 状态 issue 的 workspace
- **Token 统计**: 累计 input/output/total tokens，支持活跃会话的实时秒数

### `server.rs` — HTTP 服务 (SPEC §13.7)

基于 Axum 的 HTTP 服务器：

| 端点 | 方法 | 说明 |
|------|------|------|
| `/api/v1/state` | GET | 系统状态快照 (running/retrying/tokens) |
| `/api/v1/{identifier}` | GET | 单个 issue 详情 |
| `/api/v1/refresh` | POST | 触发立即轮询 |
| `/` | GET | React SPA 或内置 Fallback Dashboard |

内置 Fallback Dashboard 是纯 HTML/JS 实现，无需构建前端即可使用。

### React Frontend

Vite + React + TypeScript SPA：

- **`@tanstack/react-query`** 管理 API 状态，2 秒自动刷新
- **暗色主题** (GitHub-dark 风格)
- **Stats Grid**: Running / Retrying / Total Tokens / Runtime
- **Running Sessions Table**: Issue / State / Turns / Tokens / Last Event / Activity
- **Retry Queue Table**: Issue / Attempt / Due At / Error

---

## 技术栈

### 后端 (Rust)

| 类别 | Crate | 用途 |
|------|-------|------|
| 异步运行时 | `tokio` | 全特性异步运行时 |
| HTTP 服务 | `axum` + `tower-http` | 路由、中间件、静态文件服务 |
| 序列化 | `serde` + `serde_json` + `serde_yaml` | JSON/YAML 序列化 |
| 模板引擎 | `liquid` | Liquid 模板渲染 |
| 文件监听 | `notify` + `notify-debouncer-mini` | WORKFLOW.md 变更检测 |
| CLI | `clap` | 命令行参数解析 |
| 日志 | `tracing` + `tracing-subscriber` | 结构化 JSON 日志 |
| 时间 | `chrono` | 时间处理和 RFC3339 格式化 |
| 错误处理 | `thiserror` | derive Error 宏 |
| 其他 | `uuid`, `regex`, `dirs`, `async-trait`, `tokio-util` | 工具类 |

### 前端 (React)

| 类别 | 技术 | 用途 |
|------|------|------|
| 框架 | React + TypeScript | UI 组件 |
| 构建 | Vite | 开发服务器 + 生产构建 |
| 数据管理 | `@tanstack/react-query` | API 轮询和缓存 |
| 样式 | Vanilla CSS + CSS Variables | 暗色主题 |

---

## 开发

### 编译 & 测试

```bash
# 编译检查
cargo check

# 完整构建
cargo build

# 运行测试 (22 个单元测试)
cargo test

# Lint 检查
cargo clippy

# 前端类型检查
cd frontend && npx tsc --noEmit

# 前端生产构建
cd frontend && npm run build
```

### 测试覆盖

| 模块 | 测试数量 | 覆盖内容 |
|------|---------|---------|
| `models` | 2 | workspace key 清洗、状态归一化 |
| `workflow` | 4 | front matter 解析、空/无效 front matter |
| `config` | 5 | 配置解析、默认值、验证、逗号列表 |
| `prompt` | 4 | 模板渲染、attempt 参数、空模板默认值、continuation |
| `tracker/obsidian` | 5 | 候选 issue 扫描、按状态查询、ID 查询、字段解析 |
| `orchestrator` | 2 | 退避计算、调度排序 |
| **合计** | **22** | |

### 日志

使用 `tracing` + `tracing-subscriber`，输出结构化 JSON 日志：

```bash
# 设置日志级别
RUST_LOG=debug cargo run -- WORKFLOW.md

# 按模块过滤
RUST_LOG=symphony::orchestrator=debug,symphony::tracker=info cargo run -- WORKFLOW.md
```

---

## SPEC 实现对照

本项目实现了 `SPEC.md` 中定义的核心功能：

| SPEC 章节 | 功能 | 实现状态 |
|-----------|------|---------|
| §4.1 | 领域模型 | ✅ |
| §5.1–5.5 | Workflow 解析 | ✅ |
| §5.3, §6 | 配置层 (默认值/环境变量/$VAR) | ✅ |
| §6.2 | 动态配置重载 (file watcher) | ✅ |
| §6.3 | Dispatch 验证 | ✅ |
| §7 | 编排状态机 | ✅ |
| §8.2 | 调度优先级 + Blocker 检查 | ✅ |
| §8.3 | 并发控制 (全局 + 按状态) | ✅ |
| §8.4 | 重试 + 指数退避 | ✅ |
| §8.5 | 存活检测 + Stall 检测 | ✅ |
| §8.6 | 启动清理 | ✅ |
| §9 | Workspace 管理 + 路径安全 | ✅ |
| §9.4 | 生命周期 Hooks | ✅ |
| §10 | ACP (JSON-RPC over stdio) | ✅ |
| §10.5 | Tool 调用代理 | ✅ |
| §11 | Obsidian Tracker 适配器 | ✅ |
| §12 | Prompt 模板 (Liquid) | ✅ |
| §13.1 | 结构化 JSON 日志 | ✅ |
| §13.5 | Token 统计 (增量累计) | ✅ |
| §13.7 | HTTP API + Dashboard | ✅ |

---

## License

MIT
