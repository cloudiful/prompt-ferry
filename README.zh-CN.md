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
- 支持 HTTP/stdio MCP 聚合；SQLite 支持 MCP 配置、目录和运行时执行，MCP 配额及用量账本需要 PostgreSQL。
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
`ghcr.io/cloudiful/prompt-ferry:latest`，并替换其余密钥占位符。worker token、
加密密钥、上游 API Key 和初始管理员密码均为可选项；未设置的密钥会自动生成或
延后到 Admin 配置中完成，详见 [Worker 存储](#worker-存储)。然后启动：

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

### Worker 存储

Worker 使用同一套 Admin API 和配置模型，可选择 PostgreSQL 或 SQLite。
`PROMPT_FERRY_WORKER__DATABASE_URL` 非空时使用 PostgreSQL，适合共享和持久化
存储；为空时使用本地 SQLite 持久化配置。已配置但不可用的 PostgreSQL 不会自动
降级为 SQLite。两种后端都支持用户、加密密钥、端点、路由、relay、设置、客户端
密钥以及 MCP 配置、目录和运行时。SQLite 同样提供 Admin API 和认证，但不提供
持久化请求记录、原始报文保留、审批、计费、重放历史或 MCP 配额/用量账本。

SQLite 适合单个 worker；需要多 worker 或完整高级持久化能力时使用 PostgreSQL。
Valkey 是可选的，可用于共享协调和缓存加速；未配置时，SQLite 使用 SQLite 协调，
PostgreSQL 按状态语义使用现有后端或有限的本地内存降级。

relay 和 worker 可以部署在不同机器上，分别运行
`prompt-ferry relay` 和 `prompt-ferry worker`。relay-worker 桥接协议不变：worker
必须能访问 relay 的 worker bind，客户端必须能访问 relay 的 public bind。relay URL
可通过可重复的 `--relay-url` 参数或 `relay_urls` 配置列表设置；使用环境变量覆盖时，
`PROMPT_FERRY_WORKER__RELAY_URLS` 的值应为 JSON 数组。

首次启动时，空的 SQLite 数据库会从静态 worker 设置引导，包括 relay URL、上游基础地址、
TLS 以及桥接加密设置；也可以稍后通过 Admin 引导流程创建第一个上游端点。引导完成后以
SQLite 配置为准；重新加载轮询会在不重启 worker 的情况下应用支持的直接 SQLite 修改。
静态加密使用 base64 编码的 32 字节 worker 配置加密密钥
（`PROMPT_FERRY_WORKER__WORKER_CONFIG_ENCRYPTION_KEY`，旧名称
`PROMPT_FERRY_WORKER__RELAY_SECRET_MASTER_KEY` 仍可使用）。未设置时会自动生成随机密钥，
并保存到 `<data-root>/prompt-ferry/worker-config.key`（Unix 下权限为 `0600`）。
SQLite 不会以明文保存上游 API Key，也不提供明文降级路径。

生成文件位于 SQLite 数据库同级的 `prompt-ferry/` 目录下。当不存在任何活跃用户且未配置
初始管理员密码时，会生成强随机密码并一次性写入
`<data-root>/prompt-ferry/bootstrap-admin.txt`（Unix 下权限为 `0600`）；日志只输出文件
路径和登录名。已配置的初始凭据优先，已有用户永远不会被覆盖。

relay 的 `/ws/worker` 端点在 `WORKER_TOKEN` 非空时要求
`Authorization: Bearer <token>` 认证。token 为空时将完全关闭 worker 认证——任何能访问
worker bind 的客户端都可以作为 worker 连接——此时必须依靠 TLS 和网络隔离保护该端口，
relay 启动时会输出警告日志。

默认 SQLite 数据库路径为：Linux 使用 `$XDG_DATA_HOME/prompt-ferry/worker.sqlite3`，或
`$HOME/.local/share/prompt-ferry/worker.sqlite3`；macOS 使用
`$HOME/Library/Application Support/prompt-ferry/worker.sqlite3`；Windows 使用
`%LOCALAPPDATA%\\prompt-ferry\\worker.sqlite3`。可通过
`PROMPT_FERRY_WORKER__STANDALONE_DATABASE_PATH` 或
`--standalone-database-path` 覆盖。备份或恢复 SQLite 文件时应先停止 worker，并同时
保管 worker 配置加密密钥（`worker-config.key`）。

SQLite 请求和用量摘要最多保留 256 条内存记录，重启后会清空。脱敏规则会持久化，但
按会话的脱敏状态会在重启后重置。直接修改 SQLite 只有在符合受支持的 schema/配置变更
以及正常密钥加密约束时，才会由轮询重新加载。

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
PROMPT_FERRY_WORKER__UPSTREAM_API_KEY=<上游 API 密钥>
PROMPT_FERRY_WORKER__WORKER_TOKEN=<worker-token>
# 可选；未设置时自动生成到 <data-root>/prompt-ferry/worker-config.key。
PROMPT_FERRY_WORKER__WORKER_CONFIG_ENCRYPTION_KEY=<base64-32-byte-key>
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

`serve` 会将内部 worker 桥接绑定到本机回环地址，并且首次启动无需任何必填密钥：
空的 `PROMPT_FERRY_WORKER_TOKEN` 表示该回环端口不启用 worker 认证，加密密钥会在
首次启动时自动生成，需要的初始管理员密码会写入
`<data-root>/prompt-ferry/bootstrap-admin.txt`。随后在管理控制台
<http://127.0.0.1:8789> 中配置客户端令牌和上游端点：

```dotenv
PROMPT_FERRY_RELAY__CLIENT_TOKEN=<客户端令牌>
PROMPT_FERRY_WORKER__UPSTREAM_BASE_URL=https://api.example.com
PROMPT_FERRY_WORKER__UPSTREAM_API_KEY=<上游 API 密钥>
```
