use crate::usage::UsageCapture;

pub(super) fn observe_usage_chunk(
    capture: &mut UsageCapture,
    ttft_ms: &mut Option<i64>,
    chunk: &[u8],
    elapsed_ms: i64,
) {
    let output_started = capture.observe_chunk(chunk);
    if ttft_ms.is_none() && output_started {
        *ttft_ms = Some(elapsed_ms);
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
}
