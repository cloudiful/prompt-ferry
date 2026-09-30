use super::super::context::{RequestExecutionContext, RuntimeServices};
use crate::{db, usage::UsageCapture};
use tracing::warn;

/// Issue #657 Phase P1: observe one upstream chunk and report the request's
/// first meaningful output instant the moment it starts. The returned value is
/// `Some` exactly once per request — the chunk that moved `ttft_ms` from unset
/// to set — so the caller can persist the instant without re-writing it on
/// every later chunk.
pub(super) fn observe_usage_chunk(
    capture: &mut UsageCapture,
    ttft_ms: &mut Option<i64>,
    chunk: &[u8],
    elapsed_ms: i64,
) -> Option<i64> {
    let output_started = capture.observe_chunk(chunk);
    if ttft_ms.is_none() && output_started {
        *ttft_ms = Some(elapsed_ms);
        return Some(elapsed_ms);
    }
    None
}

/// Issue #657 Phase P1: persist the first output instant on the running request
/// row so a list refresh can show `流式输出中` instead of `等待响应` before the
/// request finishes. Best-effort: the terminal record still carries its own
/// timing, and the write itself never overwrites a recorded value.
pub(super) async fn record_first_output(
    services: &RuntimeServices,
    request_ctx: &RequestExecutionContext,
    ttft_ms: i64,
) {
    let Some(state) = services.admin_state() else {
        return;
    };
    if let Err(err) = db::record_request_first_output(
        &state.pool,
        request_ctx.request_id,
        Some(request_ctx.created_at),
        ttft_ms,
    )
    .await
    {
        warn!(
            error = %err,
            request_id = %request_ctx.request_id,
            ttft_ms,
            "failed to persist the first output instant on the running request record"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::observe_usage_chunk;
    use crate::usage::UsageCapture;

    #[test]
    fn captures_completed_usage_after_output_started() {
        let mut capture = UsageCapture::new(true, None);
        let mut ttft_ms = None;

        observe_usage_chunk(
            &mut capture,
            &mut ttft_ms,
            b"data: {\"type\":\"response.created\"}\n\n",
            10,
        );
        assert_eq!(ttft_ms, None);

        observe_usage_chunk(
            &mut capture,
            &mut ttft_ms,
            b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n",
            20,
        );
        assert_eq!(ttft_ms, Some(20));

        observe_usage_chunk(
            &mut capture,
            &mut ttft_ms,
            b"data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":193032,\"output_tokens\":5668,\"total_tokens\":198700,\"input_tokens_details\":{\"cached_tokens\":184064,\"cache_read_tokens\":184064,\"cache_write_tokens\":0}}}}\n\n",
            30,
        );

        assert_eq!(ttft_ms, Some(20));
        assert_eq!(capture.usage.input_tokens, Some(193032));
        assert_eq!(capture.usage.output_tokens, Some(5668));
        assert_eq!(capture.usage.total_tokens, Some(198700));
        assert_eq!(capture.usage.cached_tokens, Some(184064));
        assert_eq!(capture.usage.cache_read_tokens, Some(184064));
        assert_eq!(capture.usage.cache_write_tokens, Some(0));
    }

    #[test]
    fn reports_the_first_output_instant_exactly_once() {
        let mut capture = UsageCapture::new(true, None);
        let mut ttft_ms = None;

        assert_eq!(
            observe_usage_chunk(
                &mut capture,
                &mut ttft_ms,
                b"data: {\"type\":\"response.created\"}\n\n",
                5,
            ),
            None
        );
        assert_eq!(
            observe_usage_chunk(
                &mut capture,
                &mut ttft_ms,
                b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n",
                20,
            ),
            Some(20)
        );

        // Later output, usage, and completion chunks never re-report the
        // instant, so the running record is stamped once per request.
        assert_eq!(
            observe_usage_chunk(
                &mut capture,
                &mut ttft_ms,
                b"data: {\"type\":\"response.output_text.delta\",\"delta\":\" world\"}\n\n",
                35,
            ),
            None
        );
        assert_eq!(
            observe_usage_chunk(
                &mut capture,
                &mut ttft_ms,
                b"data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"output_tokens\":2}}}\n\n",
                40,
            ),
            None
        );
        assert_eq!(ttft_ms, Some(20));
    }

    #[test]
    fn lifecycle_only_stream_never_reports_a_first_output() {
        let mut capture = UsageCapture::new(true, None);
        let mut ttft_ms = None;

        assert_eq!(
            observe_usage_chunk(
                &mut capture,
                &mut ttft_ms,
                b"data: {\"type\":\"response.created\"}\n\n",
                5,
            ),
            None
        );
        assert_eq!(
            observe_usage_chunk(
                &mut capture,
                &mut ttft_ms,
                b"data: {\"type\":\"response.in_progress\"}\n\n",
                9,
            ),
            None
        );
        assert_eq!(ttft_ms, None);
    }
}
