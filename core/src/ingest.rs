//! The ingestion contract every source adapter implements (ARCHITECTURE §2.1).

use crate::QueryEvent;

/// One page of normalized events plus the cursor that must be persisted
/// **atomically with** the applied batch.
///
/// The exactly-once guarantee is a two-party contract:
/// - the adapter guarantees that polling again from any `next_cursor` it
///   previously returned never re-yields events already delivered before it;
/// - the caller guarantees that events and `next_cursor` are committed in a
///   single transaction, so a crash between polls replays a whole batch or
///   none of it — never a fraction.
#[derive(Debug, Clone, PartialEq)]
pub struct Batch {
    pub events: Vec<QueryEvent>,
    pub next_cursor: Option<String>,
}

/// Error from a source adapter. Polling failures are expected in normal
/// operation (source restarting, network blip) — callers log and retry; they
/// must never crash the daemon (ARCHITECTURE §2.1 "degrade, never crash-loop").
#[derive(Debug)]
pub struct IngestError(pub String);

impl std::fmt::Display for IngestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ingest error: {}", self.0)
    }
}

impl std::error::Error for IngestError {}

/// A configured ingestion source. Implementations: Pi-hole v6, AdGuard Home
/// (M3), and the fixture replayer (dev/CI/demo).
#[async_trait::async_trait]
pub trait Ingestor: Send {
    /// Stable identifier of this configured source (e.g. "pihole-main").
    fn source_id(&self) -> &str;

    /// Adapter kind, for the sources table ("pihole" | "adguard" | "fixture").
    fn kind(&self) -> &'static str;

    /// Fetch the next batch of events after `cursor` (None = from the beginning).
    /// An empty `events` with an unchanged cursor means "nothing new right now".
    async fn poll(&mut self, cursor: Option<&str>) -> Result<Batch, IngestError>;
}
