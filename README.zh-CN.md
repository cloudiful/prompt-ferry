# 模型摆渡人

[![Release](https://github.com/cloudiful/prompt-ferry/actions/workflows/release.yml/badge.svg)](https://github.com/cloudiful/prompt-ferry/actions/workflows/release.yml)
[![Latest Release](https://img.shields.io/github/v/release/cloudiful/prompt-ferry?display_name=tag)](https://github.com/cloudiful/prompt-ferry/releases)
[![License](https://img.shields.io/github/license/cloudiful/prompt-ferry)](LICENSE)
[![GHCR](https://img.shields.io/badge/container-GHCR-2496ED?logo=docker&logoColor=white)](https://github.com/cloudiful/prompt-ferry/pkgs/container/prompt-ferry)

[English](README.md) | [简体中文](README.zh-CN.md)

`prompt-ferry`（模型摆渡人）是一个支持脱敏的 OpenAI 兼容 AI API 中继，面向
Codex 和其他 API 客户端。请求经由 relay-worker 通道转发到一个或多个兼容 OpenAI
或 Anthropic 的上游服务，并在转发过程中脱敏。单条二进制默认一体运行 relay
与 worker。

```text
客户端 -> relay /v1/* <-> worker WebSocket -> 上游 API
```

## 核心功能

- 兼容 OpenAI Chat Completions、Responses 和 Embeddings，并提供原生 Anthropic Messages 和 Models 接口。
- 内置 MiniMax、CommandCode、OpencodeGo、OpenRouter、GLM、DeepSeek 和 OpenAI Platform 上游预设；其他 OpenAI 兼容服务使用显式填写 base URL 的通用 provider。
- 按请求控制思考强度，包括 Chat `reasoning_effort`、Responses `reasoning.effort` 和 Anthropic `output_config.effort`。
- 一等公民的 Embeddings 转发：目标级向量维度覆盖、失效即拒绝的脱敏与仅输入计费（[文档](docs/embeddings.zh-CN.md)）。
- 用户、客户端 API key、上游端点、模型路由、key 分流以及多 relay。
- 可配置的转发内容、日志和用量明细脱敏。
- 管理控制台提供默认文本额度汇总与逐 key 明细；在 PostgreSQL 下还包含 OpenAI 订阅的 30 天历史快照。
- 聚合 HTTP 与 stdio MCP 服务，支持按凭据与共享配额预算。
- relay-worker 之间支持 TLS、双向 TLS 和应用层加密。

依赖：业务数据使用 PostgreSQL；协调与响应亲和缓存可选 Valkey。配置了
S3 兼容存储桶后，原始请求与响应载荷会写入该存储桶。

## 使用 Docker Compose 快速开始（推荐）

```bash
cp .env.example .env
```

在 `.env` 中替换所有 `REPLACE...` 占位值：数据库密码（`POSTGRES_PASSWORD`
与 `DATABASE_URL` 两处）、`PROMPT_FERRY_RELAY__CLIENT_TOKEN`，以及两个 S3
凭据。该文件按数据读取、不会展开变量，因此两处数据库密码必须一起修改。

先启动依赖服务，因为应用启动前存储桶必须已经存在：

```bash
docker compose up -d postgres valkey rustfs
```

打开 RustFS 控制台 <http://127.0.0.1:9001>，使用 `.env` 中的 S3 access key
与 secret key 登录，创建与 `PROMPT_FERRY_WORKER__RAW_OBJECT_STORE_BUCKET`
完全同名的存储桶（默认 `prompt-ferry-raw`）。RustFS 只提供 S3 服务，不会
自动创建存储桶。随后启动应用：

```bash
docker compose up -d prompt-ferry
```

- 管理控制台：<http://127.0.0.1:8789>，使用 `admin` 登录。
- OpenAI 兼容 base URL：`http://127.0.0.1:8787/v1`。
- 关闭：SIGTERM 停止接收新请求，已接受的推理轮次最多在
  `PROMPT_FERRY_WORKER__SHUTDOWN_DRAIN_SECONDS`（120 秒）内完成。relay 等待
  `PROMPT_FERRY_RELAY__SHUTDOWN_DRAIN_SECONDS`（130 秒），Compose 在 SIGKILL
  前允许 150 秒。修改时保持 worker < relay < `stop_grace_period`。集成模式会
  等待两边排空完成；仅 worker 的进程依赖其远程 relay 的预算，停止宽限到期或
  SIGKILL 不是优雅关闭。

如果 `PROMPT_FERRY_WORKER__BOOTSTRAP_ADMIN_PASSWORD` 留空且尚无管理员，
系统会一次性生成强密码并写入数据卷：

```bash
docker compose exec prompt-ferry cat /var/lib/prompt-ferry/prompt-ferry/bootstrap-admin.txt
```

## 直接运行二进制

从 [GitHub Releases](https://github.com/cloudiful/prompt-ferry/releases) 下载
对应平台的二进制。它需要同样的 PostgreSQL、Valkey 和存储桶，并读取同一份
`.env`：

```bash
PROMPT_FERRY_DOTENV=.env prompt-ferry
```

不带子命令启动时，二进制以一体模式运行 relay 与 worker。它使用自己的
状态目录 `$XDG_DATA_HOME/prompt-ferry` 和配置目录
`$XDG_CONFIG_HOME/prompt-ferry`。请先停止 Compose 中的应用，避免
两者争抢同一组已发布端口。

全新安装与复用已有数据库不同。全新启动且
`PROMPT_FERRY_WORKER__WORKER_CONFIG_ENCRYPTION_KEY` 留空时，会在
`<data-root>/prompt-ferry/worker-config.key` 下一次性生成密钥。如需复用
Compose 已写入数据的数据库，请先把 Compose 卷中的
`/var/lib/prompt-ferry/prompt-ferry/worker-config.key` 安全复制到宿主机
`<data-root>/prompt-ferry/worker-config.key`
（默认 `~/.local/share/prompt-ferry/worker-config.key`），保持仅属主可读
（`0600`）权限，之后切勿替换/轮换该密钥；或在首次初始化前给两边配置
同一个显式 base64 32 字节密钥。已有数据库沿用已有管理员账号——只有在
尚无管理员的首次创建时才会写入 bootstrap 密码文件。

## 对外暴露前请注意

- 备份 PostgreSQL 与 `<data-root>/prompt-ferry/worker-config.key`；缺少该密钥则已存储的密文无法解密。
- PostgreSQL 17 升级到 18 只能通过 `pg_dump`/`pg_restore` 或 `pg_upgrade`；本示例新建 PostgreSQL 18 数据卷，不会迁移任何旧数据。
- 所有端口仅绑定回环地址。需要从其他机器访问时，请显式绑定端口并配置 TLS 与反向代理。

## 相关文档

[Embeddings](docs/embeddings.zh-CN.md) · [配额反馈](docs/quota-feedback.zh-CN.md) · [配额快照](docs/quota-snapshots.zh-CN.md) · [端点 key 持久化](docs/endpoint-key-persistence.zh-CN.md) · [前端异步操作](docs/frontend-async-actions.zh-CN.md) · [安全策略](SECURITY.md) · [贡献指南](CONTRIBUTING.md) · [许可证](LICENSE)
