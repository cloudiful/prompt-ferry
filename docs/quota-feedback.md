[English](quota-feedback.md) | [简体中文](quota-feedback.zh-CN.md)

# Quota feedback and summaries

The endpoint token-plan response includes optional per-key `quota_status` metadata. Its `scope` is `text_inference`; the state is `available`, `exhausted`, `unknown`, `cooling`, `rate_limited`, or `upstream_unavailable`. Raw provider windows, balances, model rows, and observation history remain available in the detail view.

## Interpreting the status

- `available` requires a fresh successful observation with usable default-text capacity.
- `exhausted` means an applicable quota or balance constraint is confirmed depleted. A zero whose reset has passed is not treated as current exhaustion.
- `unknown` covers missing or unsupported quota signals, stale observations, failed refreshes, and reset-passed zero values. Unknown is not a zero and is not a confirmation of availability.
- `cooling` means the key or endpoint is temporarily excluded because the quota or balance is confirmed spent. The response includes its deadline when available; expiry makes it eligible for a controlled recovery probe.
- `rate_limited` means the key is temporarily excluded by throttling, a concurrency limit, or an in-flight credit reservation. The account's quota is not reduced.
- `upstream_unavailable` means the upstream explicitly reported that it cannot serve the request right now.

Only a confirmed exhaustion produces `cooling` or `exhausted`. A bare mention of a quota, credit, billing or token plan, and an HTTP 429 or 402 on its own, are not exhaustion evidence; a 429 the body does not explain is reported as `rate_limited`.

These states describe default text inference, not every model or modality. MiniMax's summary uses only its `general` model row; independent model windows such as video remain visible but do not increase the text summary. Balance providers keep native units: currencies are not combined, and DeepSeek availability follows the provider's `is_available` flag.

## Effective badges and detail

Endpoint badges average the same confirmed usable key set for short and long windows. Confirmed exhausted and cooling keys do not inflate the averages. A throttled or temporarily unavailable key still contributes its reported windows, because its capacity is untouched, and its transient badge sits next to those percentages. When all applicable keys are confirmed exhausted, the relevant summary is zero; when only a subset is known, the summary is marked partial. If freshness or applicability is unknown, the badge says so rather than representing the missing signal as available or zero.

A key that can still be used keeps its channel usable, so one available key is never hidden behind a blocked one. Only when every key is blocked does the endpoint report the strongest cause present, preferring a quota cooldown over an unavailable upstream over a rate limit; it never claims every key is exhausted.

The detail view retains every reported key, model, window, balance, reset, and history row. A per-key status badge is additional context and does not replace or rewrite provider values. Observation time, refresh failure, and a retry deadline are available in its tooltip. A deadline that has already elapsed is reported as unknown until a refresh lands, so a finished backoff never still reads as a live one.

## Refresh and scope

Quota refreshes preserve the last successful raw observation while marking failed or expired observations unknown. A later successful refresh restores a current status. OpenAI ChatGPT subscription status uses the persisted observation timestamp and freshness rules described in [quota snapshots](quota-snapshots.md).

Cooldown feedback and token-plan cache state are shared by cloned state within one worker process. Separate workers do not share an instantaneous cooldown or feedback record, so their views can differ until their own observations and refreshes converge. No provider request is made just to serialize quota metadata.

## Built-in MiniMax MCP tools

The built-in `web_search` and `understand_image` tools reuse the same quota cache and the same per-key feedback store as the AI routes, so both surfaces see one quota state inside a worker process. The managed (PostgreSQL) worker publishes that shared cache to the builtin tools. A standalone (SQLite) worker has no quota snapshot source at all, so the builtin tools there run without a snapshot: they spread over the enabled keys and react to the responses they actually receive. The tools are read-only, so a quota or rate rejection may switch to another endpoint key.

`web_search` is billed against the `general` quota group, so the cached general snapshot gates its candidates: a key with a confirmed zero remaining budget is not called while a funded key is available. An unknown or stale snapshot is no signal, not exhaustion, and such a key stays selectable with a neutral weight. The image tool bills a group the provider does not document, so it never infers one: it spreads over the enabled keys and reacts to the responses it receives. Image selection never reads that shared state either: a search exhaustion, a rate cooldown or an in-flight recovery probe on a key never removes it from the image pool or changes its weight. An image outcome never writes or clears the shared general-quota feedback and never claims a shared recovery probe, so an image success or an image 429 cannot erase a search exhaustion, and the search path keeps assessing the key with its own state.

The candidates are the enabled non-blank key rows. The legacy inline endpoint key is used only when the endpoint has no key row at all: when rows exist but every one is disabled or blank, the endpoint has no usable credential and no request is sent, even though the ordinary endpoint save derives that legacy column from the first row.

One call spends at most three upstream attempts, the first attempt plus two switches, and every enabled key is tried at most once. The candidates are reassessed before each attempt, so a key that became unusable while the call was running drops out. A MiniMax business code that reports a spent quota and an HTTP 429 switch to the next key; the 429 cooldown honours the bounded `Retry-After` without sleeping through the caller's budget. An authentication or validation failure, a server failure, a malformed payload and a transport failure are returned to the caller without an automatic replay, and an endpoint with key balancing disabled keeps its single configured key. A successful HTTP status is not a successful call on its own: an unreadable or non-JSON body, and a body that is not the documented payload shape of the tool, fail the attempt instead of counting as a completed call or a success observation. When a body carries the `base_resp` envelope at all, that envelope must hold a valid integral status code; a present but invalid envelope — a non-object, a missing field or a non-integral value — is a malformed response and is never read as an absent envelope. A received non-success status also survives an unreadable or truncated body, so a 429 whose body is cut off still switches keys with its `Retry-After` instead of degrading to a generic transport failure.

A recovery probe claimed by a builtin call owns its key for the whole in-flight attempt: however long the upstream call runs, a concurrent caller sees the key as busy, and only releasing, settling or dropping that attempt frees it. The time-based recovery of an unguarded probe stays in place. When every key is exhausted, cooling or being probed, the call fails with that reason and sends no upstream request.

The whole call shares one deadline, bounded by the server timeout, that covers the payload preparation, the selection, the feedback and every attempt; it is not restarted per key. Diagnostics name the endpoint, the key label and the attempt reason only: a credential, an authorization header, an image payload and the query content are never logged. Everything the MCP client receives is a fixed diagnostic that carries only a known status or provider code: the provider's own message, a transport exception, a trace header, the query, the image source and every configured credential — enabled, disabled or the legacy inline value — are never part of it, and the original failure is dropped instead of being attached as an error source that debug formatting could reveal. The same fixed text is what the request record keeps.
