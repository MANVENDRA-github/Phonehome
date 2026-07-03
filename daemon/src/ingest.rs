//! The per-source ingest loop: poll → apply (atomically) → sleep → repeat.
//! Source failures are logged and retried next tick — never fatal (§2.1).

use crate::store::{Pulse, Store};
use phonehome_core::Ingestor;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub async fn run(
    store: Store,
    pulses: broadcast::Sender<Pulse>,
    mut ingestor: Box<dyn Ingestor>,
    interval: Duration,
) {
    let source_id = ingestor.source_id().to_owned();
    let kind = ingestor.kind();
    tracing::info!(source = %source_id, kind, "ingest loop started");

    loop {
        // Read the persisted cursor; on a store error skip this tick rather
        // than polling from None (which would re-ingest from the beginning).
        let cursor = {
            let store = store.clone();
            let sid = source_id.clone();
            match tokio::task::spawn_blocking(move || store.cursor(&sid)).await {
                Ok(Ok(c)) => c,
                Ok(Err(e)) => {
                    tracing::error!(source = %source_id, error = %e, "cursor read failed");
                    tokio::time::sleep(interval).await;
                    continue;
                }
                Err(e) => {
                    tracing::error!(source = %source_id, error = %e, "cursor task panicked");
                    tokio::time::sleep(interval).await;
                    continue;
                }
            }
        };

        match ingestor.poll(cursor.as_deref()).await {
            Ok(batch) => {
                let n = batch.events.len();
                if n > 0 || batch.next_cursor != cursor {
                    let store = store.clone();
                    let sid = source_id.clone();
                    let apply = tokio::task::spawn_blocking(move || {
                        store.apply_batch(
                            &sid,
                            kind,
                            &batch.events,
                            batch.next_cursor.as_deref(),
                            now_ms(),
                        )
                    })
                    .await;
                    match apply {
                        Ok(Ok(batch_pulses)) => {
                            if n > 0 {
                                tracing::info!(source = %source_id, events = n, "batch applied");
                            }
                            // Best-effort live hints; a send error just means
                            // no /api/stream subscriber right now.
                            for pulse in batch_pulses {
                                let _ = pulses.send(pulse);
                            }
                        }
                        Ok(Err(e)) => {
                            tracing::error!(source = %source_id, error = %e, "apply failed; batch will be re-polled");
                        }
                        Err(e) => {
                            tracing::error!(source = %source_id, error = %e, "apply task panicked");
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!(source = %source_id, error = %e, "poll failed; will retry");
            }
        }

        tokio::time::sleep(interval).await;
    }
}
