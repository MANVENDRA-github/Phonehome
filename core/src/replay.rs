//! Fixture replayer: the third first-class `Ingestor` (ARCHITECTURE §2.1).
//! All dev, CI and demo ingestion runs through this — no live network needed.

use crate::{Batch, IngestError, Ingestor, QueryEvent};
use std::path::Path;

/// Replays a JSONL fixture of [`QueryEvent`]s in fixed-size chunks.
/// Cursor = number of events already delivered, as a decimal string.
#[derive(Debug)]
pub struct FixtureReplayer {
    source_id: String,
    events: Vec<QueryEvent>,
    chunk_size: usize,
}

impl FixtureReplayer {
    /// Loads and validates the whole fixture eagerly; a malformed line is a
    /// hard error (fixtures are committed artifacts — they must be valid).
    pub fn from_path(
        source_id: impl Into<String>,
        path: &Path,
        chunk_size: usize,
    ) -> Result<Self, IngestError> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| IngestError(format!("read fixture {}: {e}", path.display())))?;
        let mut events = Vec::new();
        for (i, line) in raw.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let event: QueryEvent = serde_json::from_str(line)
                .map_err(|e| IngestError(format!("fixture line {}: {e}", i + 1)))?;
            events.push(event);
        }
        Ok(Self {
            source_id: source_id.into(),
            events,
            chunk_size: chunk_size.max(1),
        })
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

#[async_trait::async_trait]
impl Ingestor for FixtureReplayer {
    fn source_id(&self) -> &str {
        &self.source_id
    }

    fn kind(&self) -> &'static str {
        "fixture"
    }

    async fn poll(&mut self, cursor: Option<&str>) -> Result<Batch, IngestError> {
        let start: usize = match cursor {
            None => 0,
            Some(c) => c
                .parse()
                .map_err(|_| IngestError(format!("bad replayer cursor {c:?}")))?,
        };
        let start = start.min(self.events.len());
        let end = (start + self.chunk_size).min(self.events.len());
        Ok(Batch {
            events: self.events[start..end].to_vec(),
            next_cursor: Some(end.to_string()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn fixture_file(n: usize) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        for i in 0..n {
            writeln!(
                f,
                r#"{{"ts":{},"client_ip":"192.168.1.10","client_mac":null,"domain":"d{}.example","qtype":"A","blocked":false,"source":"fixture"}}"#,
                1_000_000 + i as i64,
                i
            )
            .unwrap();
        }
        f
    }

    #[tokio::test]
    async fn replays_everything_exactly_once_in_chunks() {
        let f = fixture_file(25);
        let mut r = FixtureReplayer::from_path("fixture", f.path(), 10).unwrap();
        assert_eq!(r.len(), 25);

        let mut seen = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let batch = r.poll(cursor.as_deref()).await.unwrap();
            if batch.events.is_empty() {
                break;
            }
            seen.extend(batch.events);
            cursor = batch.next_cursor;
        }
        assert_eq!(seen.len(), 25);
        let domains: std::collections::HashSet<_> = seen.iter().map(|e| &e.domain).collect();
        assert_eq!(domains.len(), 25, "no duplicates");
    }

    #[tokio::test]
    async fn resumes_from_cursor_without_duplicates() {
        let f = fixture_file(10);
        let mut r = FixtureReplayer::from_path("fixture", f.path(), 4).unwrap();
        let first = r.poll(None).await.unwrap();
        assert_eq!(first.events.len(), 4);

        // "restart": a brand-new replayer continuing from the persisted cursor
        let mut r2 = FixtureReplayer::from_path("fixture", f.path(), 4).unwrap();
        let second = r2.poll(first.next_cursor.as_deref()).await.unwrap();
        assert_eq!(second.events[0].domain, "d4.example");
    }

    #[tokio::test]
    async fn malformed_line_is_a_hard_error() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(f, "not json").unwrap();
        let err = FixtureReplayer::from_path("fixture", f.path(), 10).unwrap_err();
        assert!(err.to_string().contains("line 1"));
    }
}
