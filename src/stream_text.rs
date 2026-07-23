#[derive(Debug, Default)]
pub struct Utf8LineDecoder {
    pending_bytes: Vec<u8>,
    text_buffer: String,
    line_start: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Utf8LineDecoderError;

impl Utf8LineDecoder {
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<String>, Utf8LineDecoderError> {
        self.pending_bytes.extend_from_slice(chunk);
        self.decode_pending(false)?;
        Ok(self.drain_lines())
    }

    pub fn finish(&mut self) -> Result<Option<String>, Utf8LineDecoderError> {
        self.decode_pending(true)?;
        if self.line_start >= self.text_buffer.len() {
            self.text_buffer.clear();
            self.line_start = 0;
            Ok(None)
        } else {
            let remaining = self.text_buffer[self.line_start..].to_string();
            self.text_buffer.clear();
            self.line_start = 0;
            Ok(Some(remaining))
        }
    }

    fn decode_pending(&mut self, require_complete: bool) -> Result<(), Utf8LineDecoderError> {
        if self.pending_bytes.is_empty() {
            return Ok(());
        }

        let decoded_len = match std::str::from_utf8(&self.pending_bytes) {
            Ok(text) => {
                self.text_buffer.push_str(text);
                self.pending_bytes.len()
            }
            Err(err) if err.error_len().is_none() && !require_complete => {
                let valid_up_to = err.valid_up_to();
                if valid_up_to == 0 {
                    return Ok(());
                }
                let text = std::str::from_utf8(&self.pending_bytes[..valid_up_to])
                    .map_err(|_| Utf8LineDecoderError)?;
                self.text_buffer.push_str(text);
                valid_up_to
            }
            Err(_) => return Err(Utf8LineDecoderError),
        };

        self.pending_bytes.drain(..decoded_len);
        Ok(())
    }

    fn drain_lines(&mut self) -> Vec<String> {
        let mut lines = Vec::new();
        while let Some(relative_index) = self.text_buffer[self.line_start..].find('\n') {
            let index = self.line_start + relative_index;
            let line = self.text_buffer[self.line_start..index]
                .trim_end_matches('\r')
                .to_string();
            self.line_start = index + 1;
            lines.push(line);
        }
        self.compact_if_needed();
        lines
    }

    fn compact_if_needed(&mut self) {
        if self.line_start == 0 {
            return;
        }
        if self.line_start >= self.text_buffer.len() {
            self.text_buffer.clear();
            self.line_start = 0;
            return;
        }
        if self.line_start >= self.text_buffer.capacity() / 2
            || self.line_start >= self.text_buffer.len() / 2
        {
            self.text_buffer.drain(..self.line_start);
            self.line_start = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Utf8LineDecoder;

    #[test]
    fn preserves_split_utf8_until_codepoint_completes() {
        let mut decoder = Utf8LineDecoder::default();
        let chunk = "data: 你\n".as_bytes();
        let split = "data: ".len() + 1;

        let lines = decoder.push(&chunk[..split]).unwrap();
        assert!(lines.is_empty());

        let lines = decoder.push(&chunk[split..]).unwrap();
        assert_eq!(lines, vec!["data: 你".to_string()]);
    }

    #[test]
    fn drains_multiple_lines_without_losing_tail_text() {
        let mut decoder = Utf8LineDecoder::default();

        let lines = decoder.push(b"data: one\r\ndata: two\npartial").unwrap();
        assert_eq!(
            lines,
            vec!["data: one".to_string(), "data: two".to_string()]
        );

        let remaining = decoder.finish().unwrap();
        assert_eq!(remaining.as_deref(), Some("partial"));
    }

    #[test]
    fn continues_after_compacting_processed_prefix() {
        let mut decoder = Utf8LineDecoder::default();

        for _ in 0..16 {
            let lines = decoder.push(b"data: x\n").unwrap();
            assert_eq!(lines, vec!["data: x".to_string()]);
        }

        let lines = decoder.push(b"data: tail\n").unwrap();
        assert_eq!(lines, vec!["data: tail".to_string()]);
        assert_eq!(decoder.finish().unwrap(), None);
    }
}
