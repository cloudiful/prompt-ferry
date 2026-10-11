[English](quota-feedback.md) | [简体中文](quota-feedback.zh-CN.md)

# Quota feedback and summaries

The endpoint token-plan response includes optional per-key `quota_status` metadata. Its `scope` is `text_inference`; the state is `available`, `exhausted`, `unknown`, or `cooling`. Raw provider windows, balances, model rows, and observation history remain available in the detail view.

## Interpreting the status

- `available` requires a fresh successful observation with usable default-text capacity.
- `exhausted` means an applicable quota or balance constraint is confirmed depleted. A zero whose reset has passed is not treated as current exhaustion.
- `unknown` covers missing or unsupported quota signals, stale observations, failed refreshes, and reset-passed zero values. Unknown is not a zero and is not a confirmation of availability.
- `cooling` means the key or endpoint is temporarily excluded by an active cooldown. The response includes its deadline when available; expiry makes it eligible for a controlled recovery probe.

These states describe default text inference, not every model or modality. MiniMax's summary uses only its `general` model row; independent model windows such as video remain visible but do not increase the text summary. Balance providers keep native units: currencies are not combined, and DeepSeek availability follows the provider's `is_available` flag.

## Effective badges and detail

Endpoint badges average the same confirmed usable key set for short and long windows. Confirmed exhausted and cooling keys do not inflate the averages. When all applicable keys are confirmed exhausted, the relevant summary is zero; when only a subset is known, the summary is marked partial. If freshness or applicability is unknown, the badge says so rather than representing the missing signal as available or zero.

The detail view retains every reported key, model, window, balance, reset, and history row. A per-key status badge is additional context and does not replace or rewrite provider values. Observation time, refresh failure, and a cooldown deadline are available in its tooltip.

## Refresh and scope

Quota refreshes preserve the last successful raw observation while marking failed or expired observations unknown. A later successful refresh restores a current status. OpenAI ChatGPT subscription status uses the persisted observation timestamp and freshness rules described in [quota snapshots](quota-snapshots.md).

Cooldown feedback and token-plan cache state are shared by cloned state within one worker process. Separate workers do not share an instantaneous cooldown or feedback record, so their views can differ until their own observations and refreshes converge. No provider request is made just to serialize quota metadata.
