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
