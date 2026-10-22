# Embeddings

`prompt-ferry` relays `POST /v1/embeddings` as a first-class protocol, separate
from Chat, Responses and Anthropic Messages. A request is forwarded verbatim
except for the target's model and optional dimension override; the response
reaches only the caller.

[English](embeddings.md) | [简体中文](embeddings.zh-CN.md)

## Setup

1. Create an endpoint. For OpenRouter use the `openrouter` preset with the
   protocol `Auto`, which serves both chat and embeddings. Other
   OpenAI-compatible hosts use the Generic provider with an explicit base URL.
2. Create a model route (for example `qwen/*`). On its target choose the
   `Embeddings` protocol and an upstream model such as `qwen/qwen3-embedding-8b`.
3. Optionally set **Vector dimensions override** on the target:
   - empty inherits the client's `dimensions`,
   - a positive integer forces that dimension count for every call.

   The override is a forced value, never a cap, and it only appears for an
   embeddings target. A chat/responses/anthropic target keeps its own options
   and no dimension field; an `auto` target inherits the endpoint protocol.

## Client request

```bash
curl https://your-relay/v1/embeddings \
  -H "Authorization: Bearer $PROMPT_FERRY_CLIENT_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"qwen/qwen3-embedding-8b","input":"hello world"}'
```

A caller that posts `/v1/embeddings` with an `auto` endpoint resolves the
protocol from the path. A chat request against an embeddings target (and an
embeddings request against a chat target) is rejected instead of being adapted.

## Request validation

The request is validated before any upstream contact. A missing/empty `model`,
a missing/empty or non-homogeneous `input`, a negative token
id, a `dimensions` that is not a positive integer (zero, negative, fractional,
out of range — never clamped), an unsupported `encoding_format`, or
`stream: true` is rejected. Unknown OpenAI-compatible fields are preserved.

## Dimension behavior

Dimensions never become tokens. If the upstream rejects the requested
dimension, its error is relayed unchanged.

## Redaction

Upstream redaction is fail-closed for embeddings. When the effective redaction
policy *would* replace any text input, the request is refused with HTTP 400
`embedding_input_redaction_conflict` before any upstream contact: a redacted
input would silently change the vector and nothing could restore it. A
no-match input is forwarded byte-for-byte, and a token-only input never
conflicts. Redaction is never disabled globally for this protocol.

## Metering and logging

Usage is input-only: `input_tokens` is the upstream prompt tokens, output is
zero, and the total is the reported total (or the input). A response that
reports no usage stays unknown — no value is guessed from the vector length.

Embedding vectors reach only the caller. Whatever the request-content logging
mode, neither the normalized response text nor the raw response body of an
embeddings response is written to a request record or the raw payload store, so
no vector is retained. The request body (the input text) follows the existing
logging and redaction rules.

## Discovery and tests

The endpoint model catalog follows the effective protocol: OpenRouter lists
`/v1/embeddings/models` for embeddings, `/v1/models` otherwise, and an `auto`
OpenRouter endpoint lists the deduplicated union of both catalogs. The admin
catalog call accepts an optional `?native_api=` selector to scope the listing.

The model-route test posts a tiny embedding request to the target's actual model
(with the forced dimension when configured) and summarizes the outcome — vector
count, dimension count and reported usage — without echoing a vector.

## Production operator note

Existing deployments must apply the issue's database migration pair before the
embeddings protocol and dimension override work; the migration adds the
protocol value and the nullable per-target dimension column. No provider account
is contacted by the automated tests, and no real provider embedding volume is
certified here.
