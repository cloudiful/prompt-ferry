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

- 兼容 OpenAI Chat Completions 和 Responses，并支持通过 Responses 转发到 Anthropic Messages 上游。
- 支持对转发内容、日志和用量详情进行配置化脱敏。
- 支持用户、客户端 API Key、上游端点、模型路由和多 relay 管理。
- 支持 HTTP/stdio MCP 聚合、请求用量与重放、保留策略、审批和计费。
- 支持 relay-worker 之间的 TLS、双向 TLS 和应用层加密。

托管模式默认保留请求元数据 90 天、标准化内容 3 天、原始报文 3 天、已处理审批历史
90 天。审批保留策略不会删除 pending 状态的审批。

## 兼容边界

- Responses 的 `input_image` 转发到 Chat 上游时会转换为标准的
  `image_url` 内容块。调用方选择的 provider 和模型必须支持 Chat 多模态输入。
- 远程 URL 和 data URL 会直接透传，不会由中继下载图片。远程 URL 必须能被上游
  provider 访问，data URL 也会计入请求体大小限制。
- Chat 转换不支持 Responses file ID、工具返回图片或上游模型输出图片。使用原生
  Responses 上游时，输入图片内容不会经过 Chat 转换。

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

打开管理控制台：<http://127.0.0.1:8789>。登录后配置上游端点、模型路由、
用户和客户端 API Key，再将 OpenAI 兼容客户端指向 relay：

```dotenv
OPENAI_BASE_URL=http://127.0.0.1:8787/v1
OPENAI_API_KEY=<生成的客户端密钥>
```

如需将原始报文放在 PostgreSQL 之外，请在 `.env` 中配置
`PROMPT_FERRY_WORKER__RAW_OBJECT_STORE_*` 变量，并在 usage-retention 设置中选择
`object_store`。对象桶应配置 3 天生命周期、服务端加密和私有访问；对象存储凭据
只属于部署配置，不会通过管理 API 暴露。

请保持 `8789` 端口只对本机或受保护的内网开放。Compose 配置项见
[.env.example](.env.example)。

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

## 文档

- [安全策略](SECURITY.md)
- [贡献指南](CONTRIBUTING.md)
- [Apache License 2.0](LICENSE)

## 许可证

本项目采用 Apache License 2.0 许可证。
