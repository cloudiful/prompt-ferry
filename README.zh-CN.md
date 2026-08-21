# 模型摆渡人

[![Release](https://github.com/cloudiful/prompt-ferry/actions/workflows/release.yml/badge.svg)](https://github.com/cloudiful/prompt-ferry/actions/workflows/release.yml)
[![Latest Release](https://img.shields.io/github/v/release/cloudiful/prompt-ferry?display_name=tag)](https://github.com/cloudiful/prompt-ferry/releases)
[![License](https://img.shields.io/github/license/cloudiful/prompt-ferry)](LICENSE)
[![GHCR](https://img.shields.io/badge/container-GHCR-2496ED?logo=docker&logoColor=white)](https://github.com/cloudiful/prompt-ferry/pkgs/container/prompt-ferry)

[English](README.md) | [简体中文](README.zh-CN.md)

`prompt-ferry`（模型摆渡人）是一个支持脱敏的 OpenAI 兼容 AI API 中继，
面向 Codex 和其他 API 客户端。请求会经过 relay-worker 通道转发到一个或多个
兼容 OpenAI 或 Anthropic 的上游服务。

```text
客户端 -> relay /v1/* <-> worker WebSocket -> 上游 API
```

## 核心功能

- 兼容 OpenAI Chat Completions 和 Responses，并提供原生 Anthropic Messages 和 Models 接口。
- Anthropic SDK 客户端可使用 `x-api-key` 调用 `POST /v1/messages` 和 `GET /v1/models`；原生
  Messages 请求只会透明转发到配置为 `AnthropicMessages` 的上游端点。
- 支持按请求调整思考强度：Chat 使用 `reasoning_effort`，Responses 使用 `reasoning.effort`，包括 DeepSeek 的 `max`；Chat 兼容层会将上游不接受的 `developer` 角色转换为 `system`。
- 支持对转发内容、日志和用量详情进行配置化脱敏。
- 支持用户、客户端 API Key、上游端点、模型路由和多 relay 管理。
- 支持 HTTP/stdio MCP 聚合、请求用量与重放、保留策略、审批和计费。
- 支持 MCP 凭据配额：按凭据或共享配额组设置请求/credits 预算，原子预占、按使用率均衡多个 API key、
  认证/限流失败自动冷却，并支持 Firecrawl 等按 credits 计费的 `creditsUsed` 校准。
- 支持 relay-worker 之间的 TLS、双向 TLS 和应用层加密。
- 支持原生 Responses 透传，包括 DeepSeek v4 flash；endpoint 使用 `Responses` 或
  `Auto`，模型路由使用 `force_passthrough`。
- 支持有界的 relay 响应缓冲，并可配置队列和字节上限。

## 部署

### Docker Compose

Compose 示例会启动 PostgreSQL、relay、worker 和管理控制台，并使用 GHCR
中的预构建镜像。

```bash
cp .env.example .env
```

编辑 `.env`，将 `PROMPT_FERRY_IMAGE` 设置为
`ghcr.io/cloudiful/prompt-ferry:latest`，并替换所有密钥占位符。然后启动：

```bash
docker compose pull
docker compose up -d
```

stdio MCP 的 worker 镜像包含 `uv`/`uvx`。配置 MCP 时，命令可以填写为
`["uvx", "minimax-coding-plan-mcp", "-y"]`；未在 MCP 表单中配置的环境变量会自动
继承 worker 环境。敏感变量建议设置在 `.env` 的 `MINIMAX_API_KEY` 中，Compose 会将其
传入 worker；也可以在 MCP 表单中直接填写变量值。

打开管理控制台：<http://127.0.0.1:8789>。登录后配置上游端点、模型路由、
用户和客户端 API Key，再将 OpenAI 兼容客户端指向 relay：

```dotenv
OPENAI_BASE_URL=http://127.0.0.1:8787/v1
OPENAI_API_KEY=<生成的客户端密钥>
```

Anthropic SDK 客户端可使用同一个 relay 地址和客户端密钥：

```dotenv
ANTHROPIC_BASE_URL=http://127.0.0.1:8787
ANTHROPIC_API_KEY=<生成的客户端密钥>
```

对于 OpenCode 或其他 `@ai-sdk/anthropic` 客户端，provider 基础地址应使用
relay 的 `/v1` 前缀，这样 SDK 会请求 `/v1/messages`。MiniMax 的 Anthropic
上游应配置为 `https://api.minimaxi.com/anthropic` 或
`https://api.minimax.io/anthropic`，协议选择 `AnthropicMessages`。

首期 Anthropic 兼容接口只支持 Messages 和 Models，不会将 Anthropic Messages 请求转换到
OpenAI Chat 或 Responses 上游，也暂不提供 Anthropic Files、Batches 和 Token Counting 接口。

如需将原始报文放在 PostgreSQL 之外，请在 `.env` 中配置
`PROMPT_FERRY_WORKER__RAW_OBJECT_STORE_*` 变量，并在 usage-retention 设置中选择
`object_store`。对象桶应配置 3 天生命周期、服务端加密和私有访问；对象存储凭据
只属于部署配置，不会通过管理 API 暴露。

如需适配读取较慢的下游客户端，可配置
`PROMPT_FERRY_RELAY__RESPONSE_STREAM_BUFFER` 和
`PROMPT_FERRY_RELAY__RESPONSE_STREAM_MAX_BYTES`。还可通过
`PROMPT_FERRY_RELAY__RESPONSE_STREAM_BACKPRESSURE_TIMEOUT_MS` 配置每个响应
forwarding pump 等待慢客户端的时长，默认 5000 毫秒。默认值分别为 256 个缓冲块和
16 MiB；三项配置都必须大于零。

请保持 `8789` 端口只对本机或受保护的内网开放。Compose 配置项见
[.env.example](.env.example)。

### Worker 存储模式

Worker 有两种存储模式。`PROMPT_FERRY_WORKER__DATABASE_URL` 非空时使用现有的
PostgreSQL 共享托管模式；为空时使用本地 SQLite 配置数据库的独立托管模式。
已配置但不可用的 PostgreSQL 不会自动降级为 SQLite。

独立模式下，relay 和 worker 可以部署在不同机器上，分别运行
`prompt-ferry relay` 和 `prompt-ferry worker`。relay-worker 桥接协议不变：worker
必须能访问 relay 的 worker bind，客户端必须能访问 relay 的 public bind。relay URL
可通过可重复的 `--relay-url` 参数或 `relay_urls` 配置列表设置；使用环境变量覆盖时，
`PROMPT_FERRY_WORKER__RELAY_URLS` 的值应为 JSON 数组。

首次启动时，空的 SQLite 数据库会从静态 worker 设置引导，包括 relay URL、上游基础地址
和 API Key、worker token、TLS 以及桥接加密设置。引导完成后以 SQLite 配置为准；重新加载
轮询会在不重启 worker 的情况下应用支持的直接 SQLite 修改。必须设置
`PROMPT_FERRY_WORKER__RELAY_SECRET_MASTER_KEY`，其值为 base64 编码的 32 字节密钥，用于
静态加密保存密钥；SQLite 不会以明文保存上游 API Key。

默认数据库路径为：Linux 使用 `$XDG_DATA_HOME/prompt-ferry/worker.sqlite3`，或
`$HOME/.local/share/prompt-ferry/worker.sqlite3`；macOS 使用
`$HOME/Library/Application Support/prompt-ferry/worker.sqlite3`；Windows 使用
`%LOCALAPPDATA%\\prompt-ferry\\worker.sqlite3`。可通过
`PROMPT_FERRY_WORKER__STANDALONE_DATABASE_PATH` 或
`--standalone-database-path` 覆盖。备份或恢复 SQLite 文件时应先停止 worker，并同时
保管 master key。

独立模式的运行时状态有明确上限：请求和用量只保留在最多 256 条记录的内存摘要环中。
不会持久化请求或原始报文、计费、审批、配额或重放历史；重启后内存环会清空。MCP
不可用。脱敏规则会持久化，但按会话的脱敏状态会在重启后重置。Valkey 是可选的：独立
模式不要求 Valkey；如果配置了可访问的 Valkey URL，仍可使用现有 Valkey 后端。Valkey
缺失或不可用时，有限的本地 affinity、session 和 replay 内存状态仅限单进程，不会在多个
worker 之间共享；持久化的请求和用量历史仍不可用。

独立模式暂未提供配置变更 CLI 或 API。静态 worker 设置会引导空的 SQLite 数据库；直接
修改 SQLite 只有在符合受支持的 schema/配置变更以及正常密钥加密约束时，才会由轮询重新
加载。需要管理控制面时请使用 PostgreSQL 模式；独立模式不是完整的管理控制台。

不同主机部署时可使用以下占位符示例，并确保 worker 主机能够访问桥接端口：

```dotenv
# Relay 主机
PROMPT_FERRY_RELAY__BIND=0.0.0.0:8787
PROMPT_FERRY_RELAY__WORKER_BIND=0.0.0.0:8788
PROMPT_FERRY_RELAY__CLIENT_TOKEN=<client-token>
PROMPT_FERRY_RELAY__WORKER_TOKEN=<worker-token>
```

```bash
prompt-ferry relay
```

```dotenv
# Worker 主机
PROMPT_FERRY_WORKER__DATABASE_URL=
PROMPT_FERRY_WORKER__RELAY_URLS=["wss://relay.example.invalid:8788/ws/worker"]
PROMPT_FERRY_WORKER__UPSTREAM_BASE_URL=https://upstream.example.invalid
PROMPT_FERRY_WORKER__UPSTREAM_API_KEY=<upstream-api-key>
PROMPT_FERRY_WORKER__WORKER_TOKEN=<worker-token>
PROMPT_FERRY_WORKER__RELAY_SECRET_MASTER_KEY=<base64-32-byte-key>
PROMPT_FERRY_WORKER__TLS_MODE=<configured-tls-mode>
PROMPT_FERRY_WORKER__BRIDGE_ENCRYPTION_MODE=<configured-bridge-mode>
```

```bash
prompt-ferry worker
```

### 单机二进制

从 [GitHub Releases](https://github.com/cloudiful/prompt-ferry/releases) 下载对应平台的
二进制文件，然后让 relay 和 worker 在同一个进程中运行：

```bash
./prompt-ferry serve
```

非托管模式启动前需要配置 relay 客户端令牌和上游 worker：

```dotenv
PROMPT_FERRY_RELAY__CLIENT_TOKEN=<客户端令牌>
PROMPT_FERRY_WORKER__UPSTREAM_BASE_URL=https://api.example.com
PROMPT_FERRY_WORKER__UPSTREAM_API_KEY=<上游 API 密钥>
```
