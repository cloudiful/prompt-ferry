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
- 内置 MiniMax、CommandCode、OpencodeGo、OpenRouter、GLM、DeepSeek 与 OpenAI Platform 一等上游预设；
  预设基础地址由服务端推导（OpenAI 为 `https://api.openai.com`），只需配置推理 API Key。其他
  OpenAI-compatible 上游继续使用通用 provider 并显式填写基础地址。
- 支持 HTTP/stdio MCP 聚合；SQLite 支持 MCP 配置、目录和运行时执行，MCP 配额及用量账本需要 PostgreSQL。
- 支持 MCP 凭据配额：按凭据或共享配额组设置请求/credits 预算，原子预占、按使用率均衡多个 API key、
  认证/限流失败自动冷却，并支持 Firecrawl 等按 credits 计费的 `creditsUsed` 校准。
- 支持 relay-worker 之间的 TLS、双向 TLS 和应用层加密。
- 支持原生 Responses 透传，包括 DeepSeek v4 flash；Responses 请求要求 Responses
  原生目标，不做跨协议转换直接透传。
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

MiniMax Coding Plan 请优先使用 MiniMax 上游端点的「暴露 MiniMax MCP 工具」开关
（推荐）。开启后会创建托管的 `builtin_minimax` 服务，提供 `web_search` 和
`understand_image`，无需启动 `uvx` 子进程。托管服务复用该端点已配置的
token-plan API key（开启 endpoint key 分流时多 key 轮换），无需 Basic Auth；
端点区域决定请求 `https://api.minimaxi.com`（中国区）或
`https://api.minimax.io`（国际区）。已有自定义 `minimax-coding-plan-mcp`
stdio 行不会被自动转换，行为保持不变。

worker 镜像同样内置 `uv`/`uvx`，可用于通用的第三方 stdio MCP 服务。配置时命令
可填写为 `["uvx", "example-mcp-server", "-y"]`；未在 MCP 表单中配置的环境变量
会自动继承 worker 环境。敏感变量建议设置在 `.env` 的 `MINIMAX_API_KEY` 中，
Compose 会将其传入 worker；也可以在 MCP 表单中直接填写变量值。

Compose 已通过命名卷持久化 worker 的 uv 缓存（`/root/.cache/uv` 与
`/root/.local/share/uv`），重建容器时无需重新下载 Python 运行时与依赖。容器
`restart` 会保留文件系统；若没有这些卷，`down`/重建会丢失缓存。

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

### 出站代理

LLM 上游使用端点 `proxy_url` 与按路由 `proxy_url_override`
（`http/https/socks5/socks5h`）；未设置表示直连。模型列表拉取、token plan
用量、端点协议检查与 `me` 可用模型复用同一端点代理走池化 client；非法代理
直接失败，不会静默回落直连。

MCP HTTP 服务按 `mcp_servers.proxy_url` 优先，再取 worker 进程环境变量
（`HTTPS_PROXY`/`HTTP_PROXY`，小写可用，`ALL_PROXY` 兜底），`NO_PROXY` 绕行，
最后直连。内置 MiniMax MCP 继承其绑定端点的代理（`端点 → 环境 → 直连`）；
其行代理被忽略，图片 URL 拉取保持直连并做 pinned DNS。MCP `stdio` 继承
worker 环境变量给子进程。代理非法时请求直接失败，不会静默直连；凭据不会
写入日志。

以下保持直连：审批 webhook（用户内网设施）、上游 Realtime websocket
（需手写 CONNECT 隧道，已确认暂缓）、relay-worker 桥接（内部控制面）。

```dotenv
HTTPS_PROXY=http://proxy.example.invalid:8080
NO_PROXY=127.0.0.1,localhost
```

### 路由目标排期

每个模型路由目标可配生效时间段（`[{start,end,days?}]` `HH:MM`，`days` 1=周一..7=周日，缺省/空即每天）；留空即始终生效。
窗口外的目标不参与路由。时间段按 Worker 本机时间解释：`end` 早于 `start`
视为跨天（如 `22:00–06:00` 按起始日判定星期），开始分钟含在内、结束分钟不含在内，
命中任一段即生效。`enabled=false` 的目标即使在窗口内也不参选。

排期生效时请让所有 Worker 使用同一时区；多机时区不一致会对同一窗口得出
不同结论。过滤为 fail-closed：无可用目标时请求直接报错，不会回落到窗口外
目标；已存排期非法时也不会静默当作始终生效。

```text
no route target is active for route 'summarizer' at 03:12 (worker-local time; windows: primary(enabled): 06:30–14:00, 18:00–20:00; night(disabled): unrestricted)
```

### 端点默认排期与目标归一

每个上游端点可配默认时间段（`[{start,end,days?}]` `HH:MM`）；留空即始终生效。
目标排期留空时继承所属端点默认；目标非空则覆盖端点；两者都空即始终生效。
两级复用同一套 `HH:MM` 校验。

目标归一（`dev_system_normalize`，默认关闭）控制 Chat 透传：关闭（默认）时
`developer` 原样透传；开启后 `developer` 重写为 `system`。这是相对此前无条件
重写的行为变更：严格校验 `developer` 的上游需按目标手动开启。开关位于目标行
齿轮 popover，与代理、排期同处，恒以 `true`/`false` 全量下发。

### 连续会话缓存率报警

PostgreSQL 部署可以对持续重读上下文的会话报警（SQLite 单机模式不做聚合与
报警，请使用 PostgreSQL）。

监控任务按 `conversation_id` 统计最近 `window_minutes` 内 `completed` 的 AI
轮次：去重轮次数达到 `min_turns` 且 fold-aware 缓存读取率低于 `threshold`
时，发送一条钉钉机器人消息。该比率与用量总览一致
（`SUM(cache_read) / SUM(fold-aware 完整输入)`，截断到 `0..1`），失败和进行中
的行不计入；每个会话还有独立的 `cooldown_minutes` 冷却时钟，冷却期内不重复
报警。

在管理控制台通过 `GET`/`PUT /api/v1/settings/cache-alert` 配置 `enabled`、
`window_minutes`（5–1440，默认 30）、`min_turns`（2–100，默认 5）、
`threshold`（0–1，默认 0.2）、`cooldown_minutes`（5–1440，默认 60），以及钉钉
机器人 `dingtalk_webhook_url` 和可选 `dingtalk_secret`（加签机器人）。密钥只写
不回显，留空表示保留已存值。监控任务每 `min(window_minutes, 60)` 分钟执行
一次检测，同一时刻只有一个 worker 参与。

消息只包含会话元数据——`conversation_id`、`model`、`window`、`turns`、
`cache_rate`、`threshold`——不含请求正文与用户信息。

### Responses compact

- `POST /v1/responses/compact` 对 Responses 原生上游逐字节透传；把返回的
  `output` 原样作为下一次 `POST /v1/responses` 的 `input` 即可继续对话。
- 无原生 compact 支持的目标，可在目标上设置 `compact_mode=self_summarize`
  启用 ferry 侧摘要兜底（丢弃 `encrypted_content`、裁剪旧 tool 输出、返回
  明文 handoff）。默认 `passthrough` 对非 Responses 目标返回 `400`；
  `off` 则按目标禁用 compact。

### Responses 无状态降级

Chat 请求里的 `tool_calls` 回合若丢失 `tool` 输出（历史被 prune 或截断），
ferry 会在该 call 后紧邻插入 `function_call_output` 占位
（`[Missing tool output: pruned or truncated, call_id=<id>]`），上游 Responses
不再以 `No tool output found for function call` 拒绝请求。

上游仍拒绝续写（`No tool output found ...` 或
`Referenced reasoning item ... was not found or has expired`）时，ferry 返回
`code=retryable_invalid_continuation` 与重试提示：丢掉
`previous_response_id`，从孤儿 `call_*` 之前截断，重试一次。两处降级均输出
结构化日志（`event=chat_to_responses_missing_tool_output`、
`event=upstream_invalid_continuation`）。

### 思考降级

思考模式的上游在“父轮 assistant tool-call 消息没有可回传的 reasoning”（
thinking 模式下 `reasoning_content` / `reasoning_text` 必须回传）时，会拒绝带
tools 的一轮请求。ferry 让这类轮次继续可用：

- 事前降级（仅对需要回传思考的上游，当前为 DeepSeek）：已落盘的父轮 artifact 证明
  父轮没有产生 reasoning，且本轮请求思考、带 tools、也没有可回填内容时，ferry 只对
  这一轮关掉思考。Chat 请求体写入 `thinking: {"type":"disabled"}` 并删除
  `reasoning_effort`（覆盖按目标的思考强度覆盖值）；Responses 请求体写入
  `reasoning.effort: "none"`。其余上游（例如 opencode go）保持请求的思考（含按目标的
  强度覆盖值），出站请求体不会被改写为 `reasoning.effort: "none"`。
- 指纹重试：上游返回 `400` 且响应体包含 `must be passed back` 时，同一轮会关闭
  思考重发一次；重发仍被拒绝则返回原始上游错误。该路径不区分上游，对所有上游
  始终生效。

两条路径都不会伪造 reasoning、不新增落盘字段、不改动目标配置；能回传 reasoning
的轮次按原字节转发。观测事件：`event=thinking_downgrade`、
`event=thinking_echo_retry`、`event=thinking_echo_retry_sent`、
`event=thinking_echo_retry_rejected`，均带 `conversation_id`、`provider`、
`native_api`、`disposition`、`attempt`。设置
`PROMPT_FERRY_DISABLE_THINKING_DOWNGRADE=1` 可同时旁路两条路径。

### ChatGPT Plus/Pro 订阅（OAuth）

OpenAI 端点支持两种「上游计划」（Admin → 上游端点 → OpenAI）：

- 官方 API Key（Platform）：沿用 `https://api.openai.com`，用量按 Platform API 口径。
- ChatGPT Plus/Pro 订阅：使用 ChatGPT 账号 OAuth 登录，推理走 Codex 后端
  `https://chatgpt.com/backend-api/codex/responses`，额度按订阅的 5 小时/周窗口。

配置步骤：

1. 新建或编辑 OpenAI 端点并先保存（订阅计划需要端点已存在）。
2. 在「ChatGPT 登录」区完成登录：Headless 用设备码（界面会显示设备码与验证地址，
   在浏览器打开并输入设备码），或浏览器登录（打开授权链接完成登录后，把跳转到
   `http://localhost:1455/auth/callback` 的完整地址粘贴回输入框）。
3. 登录成功后把「上游计划」切换为 ChatGPT Plus/Pro 订阅并保存。凭据到期会自动
   刷新；上游返回 401 时自动刷新并重试一次；refresh token 失效会清除凭据并提示
   重新登录。
4. 端点端口类型需为 `responses`。客户端模型名会归一为 Codex 后端模型：Codex 系列
   （`gpt-6-sol`、`gpt-6-astra`、`gpt-6-luna`、`gpt-5.6-sol`、`gpt-5.6-terra`、
   `gpt-5.6-luna`、`gpt-5.2-codex`、`gpt-5.1-codex` 等）原样保留并去掉思考
   强度后缀；未知模型原样透传，由上游返回真实的模型错误；模型为空时回退到 `gpt-6-sol`。

订阅额度仅在端点对话框展示（5 小时/周窗口），不参与路由权重，也不计入 Platform
API 用量；两套凭据相互独立：切换到官方 API Key 计划会清除已存的 OAuth 凭据，切离
OpenAI 提供方同样清理。内网前置或测试可用 `PROMPT_FERRY_CHATGPT_OAUTH_ISSUER` 与
`PROMPT_FERRY_CHATGPT_BACKEND_URL` 覆盖 ChatGPT 认证与后端域名。

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
