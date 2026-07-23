use anyhow::{Result, anyhow};

pub(super) struct DataLine<'a> {
    range: std::ops::Range<usize>,
    pub(super) payload: &'a str,
}

impl<'a> DataLine<'a> {
    pub(super) fn parse(event: &'a [u8]) -> Result<Option<Self>> {
        let text = std::str::from_utf8(event)
            .map_err(|err| anyhow!("invalid UTF-8 SSE event during upstream restore: {err}"))?;
        let mut offset = 0;
        let mut found = None;
        for line in text.split_inclusive('\n') {
            let content = line.trim_end_matches(['\r', '\n']);
            if let Some(payload) = content.strip_prefix("data:") {
                if found.is_some() {
                    return Err(anyhow!(
                        "multi-line SSE data is unsupported during upstream restore"
                    ));
                }
                found = Some(Self {
                    range: offset..offset + content.len(),
                    payload: payload.trim_start(),
                });
            }
            offset += line.len();
        }
        Ok(found)
    }

    pub(super) fn replace(&self, event: &[u8], json: &str) -> Result<Vec<u8>> {
        let mut output = Vec::with_capacity(event.len() + json.len());
        output.extend_from_slice(&event[..self.range.start]);
        output.extend_from_slice(b"data: ");
        output.extend_from_slice(json.as_bytes());
        output.extend_from_slice(&event[self.range.end..]);
        Ok(output)
    }
}
