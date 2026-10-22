# Embeddings 向量接口

`prompt-ferry` 将 `POST /v1/embeddings` 作为与 Chat、Responses、Anthropic
Messages 并列的一等协议转发。除目标的模型与可选维度覆盖外，请求按原样转发；
响应只返回给调用端。

[English](embeddings.md) | [简体中文](embeddings.zh-CN.md)

## 配置

1. 创建上游端点。OpenRouter 使用 `openrouter` 预设并选择 `Auto` 协议，可同时
   服务 chat 与 embeddings；其他 OpenAI 兼容服务使用通用 provider 并显式填写
   base URL。
2. 创建模型路由（例如 `qwen/*`）。在目标上选择 `Embeddings` 端口类型，并填写
   上游模型，例如 `qwen/qwen3-embedding-8b`。
3. 可选地在目标上设置**向量维度覆盖**：
   - 留空沿用客户端的 `dimensions`；
   - 正整数强制该目标使用该维度。

   覆盖是强制值，不是上限；仅对 embeddings 目标显示。chat/responses/anthropic
   目标保留各自选项且不显示维度字段；`auto` 目标继承端点协议。

## 调用示例

```bash
curl https://your-relay/v1/embeddings \
  -H "Authorization: Bearer $PROMPT_FERRY_CLIENT_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"qwen/qwen3-embedding-8b","input":"hello world"}'
```

使用 `auto` 端点向 `/v1/embeddings` 发请求时按路径解析协议。向 embeddings 目标发送
chat 请求（或反向）会被直接拒绝，而不是被转换。

## 请求校验

请求在任何上游访问之前完成校验：`model` 缺失或为空、`input` 缺失/为空或类型不
一致、token id 非整数或为负、`dimensions` 非正整数（0、负数、小数、超范围——绝不
截断）、`encoding_format` 不支持、`stream: true` 都会被拒绝。未知的 OpenAI 兼容
字段会原样保留。

## 维度行为

维度不计入 token。若上游拒绝所请求的维度，其错误原样返回。

## 脱敏

Embeddings 的上游脱敏采用「失效即拒绝」。当生效的脱敏策略会替换任一文本输入时，
请求在任何上游访问前以 HTTP 400 `embedding_input_redaction_conflict` 拒绝：被替换
的输入会静默改变向量，且无法还原。无命中的输入按字节原样转发；纯 token 输入不会
冲突。该协议不会全局关闭脱敏。

## 计费与日志

用量仅含输入：`input_tokens` 取自上游 prompt tokens，输出为 0，total 取上游上报值
（否则等于输入）。上游未上报用量时保持未知，不会用向量长度猜测。

向量只返回给调用端。无论请求内容日志模式如何，embeddings 响应的规范化文本与原始
响应体都不会写入请求记录或原始载荷存储，因此不会留存向量。请求体（输入文本）遵循
既有的日志与脱敏规则。

## 发现与测试

端点模型目录跟随生效协议：OpenRouter 的 embeddings 列出
`/v1/embeddings/models`，其他情况列出 `/v1/models`；`auto` OpenRouter 端点返回两者
去重并集。管理端目录接口可用可选参数 `?native_api=` 限定协议。

模型路由测试向目标实际模型发送一个极小的 embedding 请求（配置了维度时带上强制
维度），并汇总结果——向量数量、维度数量与上报用量——不会回显任何向量。

## 生产运维说明

现有部署需先应用本 issue 的数据库迁移对，embeddings 协议与维度覆盖才能生效：迁移
新增协议取值与可空的逐目标维度列。自动化测试不访问任何真实 provider 账号，也不
代表已验证真实 provider 的向量用量。
