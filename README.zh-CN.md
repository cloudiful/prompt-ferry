# prompt-ferry

兼容 OpenAI API、支持节点互连的 AI 网关。单个节点在同一监听端口（默认 `:8787`）提供浏览器、管理 API、AI API 和节点连接。数据库节点处理请求；无数据库的边缘节点通过认证的节点连接转发浏览器及 AI 流量，不直接连接 PostgreSQL 或 Valkey。任一端可配置 `PROMPT_FERRY__PEER_URLS`，不支持多跳。

## 快速开始

已有安装请先阅读 [迁移指南](MIGRATING.md)。新 Compose 安装：复制 `.env.example` 为 `.env`，用 `openssl rand -base64 32` 生成一次 `PROMPT_FERRY__MASTER_SECRET`，安全保存并备份，填写数据库和对象存储凭据。主机数据库 URL 与 `POSTGRES_PASSWORD` 分别包含密码，修改时须同步。运行 `docker compose up -d`，需要原始数据存储时在 RustFS 创建 bucket。保留数据及配置卷，数据根目录保存持久节点 ID，旧配置卷可能有备份。

访问 `http://127.0.0.1:8787`，在可信本地连接或 HTTPS 上使用主密钥完成首次设置，再正常登录。在界面创建托管 AI 客户端密钥作为 API bearer；主密钥不是 AI bearer。

数据库节点设置 `PROMPT_FERRY__DATABASE_URL`，边缘节点留空。节点地址为 JSON 数组，例如 `["wss://node.example:8787"]` 或 `[]`。所有节点必须使用相同编译版本、协议及主密钥；同数据库节点共享状态，多个数据库节点需要共享 Valkey。单数据库节点不配置 Valkey 时会使用本地会话，重启后需重新登录；已配置的 Valkey 不可用时不会回退。生成开始后不保证跨组内存接管或透明重试。

为 MCP OAuth 设置规范的公共 HTTPS 地址 `PROMPT_FERRY__MCP_OAUTH_REDIRECT_BASE`。同源共享登录，不同域名分别登录。反向代理必须保留原始 Host 以检查设置请求的 Origin。TLS 和信任要求见 [SECURITY.md](SECURITY.md)。

开发时在环境中配置稳定主密钥，或用 `PROMPT_FERRY_DOTENV` 指定独立 dotenv，再运行 `bash scripts/dev.sh backend` 或 `bash scripts/dev.sh full`。full 模式使用默认 `127.0.0.1:8787` 和 Vite API 代理。后端单元检查为 `bash scripts/ci/run-tests.sh`，参见 [CONTRIBUTING.md](CONTRIBUTING.md)。
