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
- 一等公民的 Embeddings 转发：目标级向量维度覆盖、失效即拒绝的脱敏与仅输入计费。
- 用户、客户端 API key、上游端点、模型路由、key 分流，以及本地配置的 relay 目标。
- 只读的 Worker 列表，展示同一 PostgreSQL 数据库中注册的所有进程。
- 可配置的转发内容、日志和用量明细脱敏。
- 管理控制台提供默认文本额度汇总与逐 key 明细；在 PostgreSQL 下还包含 OpenAI 订阅的 30 天历史快照。
- 聚合 HTTP 与 stdio MCP 服务，支持按凭据与共享配额预算。
- relay-worker 之间支持 TLS、双向 TLS 和应用层加密。

依赖：业务数据使用 PostgreSQL；协调与响应亲和缓存可选 Valkey。共享 Valkey
地址以加密形式保存在数据库中，并且数据库记录始终优先。全新部署可在首次启动时
用 `PROMPT_FERRY_WORKER__VALKEY_URL` 初始化该记录：它在 `.env` 加载完成后、
仅由 PostgreSQL 引导流程读取，而且只在尚不存在 `shared_valkey` 记录时生效，
不会覆盖已保存或被有意清空的设置。留空表示不播种；没有记录时提供无效值会直接
启动失败。播种值与其他已存密钥使用同一个 worker 配置密钥加密，因此共用同一
数据库的所有 worker 必须配置相同的显式
`PROMPT_FERRY_WORKER__WORKER_CONFIG_ENCRYPTION_KEY`。已保存的地址会先探测再使用：
配置了 Valkey 但无法连接时启动失败，不会静默回退或重新播种；请修复连通性或
“管理 → 设置 → 存储”中的值。首次启动后，在“管理 → 设置 → 存储”中编辑该设置；
修改后必须协调重启：先由仍在运行并托管管理控制台的 worker 完成编辑，再停止所有
旧 worker，最后启动所有 worker 以应用新的期望修订。不要在编辑前停止全部 worker，
因为管理编辑器由 worker 提供。准入会拒绝协调模式或已应用修订与在线注册表不一致
的 worker，因此引导模式进程与协调模式进程不会并行运行。配置了 S3 兼容存储桶后，
原始请求与响应载荷会写入该存储桶。

未配置共享 Valkey 时，PostgreSQL 运行文档说明的单 worker 降级模式。其有界管理
会话保存在进程内存中（会话 TTL 与条目上限仍然生效），因此重启会让管理员退出，
重放快照也保持不可用。数据库配置会跨重启保留，但本地会话不会。

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

随后打开“管理 → 设置 → 存储”，按需修改共享 Valkey 地址，并重启 worker 以
应用新版本。Compose 在首次启动且 `PROMPT_FERRY_WORKER__VALKEY_URL` 未设置或为空时
会自动播种 `redis://valkey:6379`，因此全新环境无需手工 SQL 或控制台编辑。记录一旦
存在（包括被清空的记录），环境变量播种就会被忽略，数据库值优先。多 worker 部署在
编辑后应先停止所有 worker，再用新修订启动全部 worker。未配置地址或地址被有意清空
时，worker 运行在文档说明的单 worker 降级模式，并使用进程内管理会话。配置了地址但
无法连接则不同：启动会直接失败，而不会进入该降级模式，请修复连通性或已存值。

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
`.env`。首次启动可在其中设置
`PROMPT_FERRY_WORKER__VALKEY_URL=redis://127.0.0.1:6379`；使用 Compose 时请留空，
它会默认使用自身的服务地址。

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

## 个人 MCP OAuth

个人 HTTP MCP 服务（例如 `https://huggingface.co/mcp?login`）可在管理控制台使用 OAuth，无需静态 Bearer 令牌。将 `PROMPT_FERRY_WORKER__MCP_OAUTH_REDIRECT_BASE` 设为公开的 HTTPS 基地址（回调后缀为 `/api/v1/admin/mcp-servers/{server_id}/oauth/callback`）；PostgreSQL 部署还需显式配置共享的 `PROMPT_FERRY_WORKER__WORKER_CONFIG_ENCRYPTION_KEY`，以便所有 Worker 解密同一份授权。个人 OAuth 仅申请只读 `read-mcp` 权限。单服务代理、明文 HTTP 服务地址、stdio/内置传输、管理员范围或缺少配置时，该服务会停用 OAuth 并给出可操作错误，其他认证方式不受影响。运行期令牌不会导出或显示在界面中，本地断开后需重新连接。

## 对外暴露前请注意

- 备份 PostgreSQL 与 `<data-root>/prompt-ferry/worker-config.key`；缺少该密钥则已存储的密文无法解密。
- PostgreSQL 17 升级到 18 只能通过 `pg_dump`/`pg_restore` 或 `pg_upgrade`；本示例新建 PostgreSQL 18 数据卷，不会迁移任何旧数据。
- 所有端口仅绑定回环地址。需要从其他机器访问时，请显式绑定端口并配置 TLS 与反向代理。
- 会话亲和负载会记录选中的上游模型。请一次性升级全部 relay worker：旧版本会忽略该模型，可能把同一端点的会话交给另一个模型处理。

## 相关文档

[安全策略](SECURITY.md) · [贡献指南](CONTRIBUTING.md) · [许可证](LICENSE)
