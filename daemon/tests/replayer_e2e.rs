//! SPEC M1 acceptance test: the committed fixture replays into the store with
//! ZERO loss and ZERO duplication across a mid-run restart, where the restart
//! recreates both the store handle (from the same db file) and the ingestor
//! (resuming purely from the persisted cursor).
//!
//! Expected values are computed independently by re-reading the fixture file,
//! so a regression in replayer OR store shows up as a count mismatch.

use phonehome_core::{FixtureReplayer, Ingestor, QueryEvent};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

// Store lives in the daemon binary; include it directly for integration use.
#[path = "../src/store.rs"]
mod store;
use store::Store;

fn fixture_path() -> PathBuf {
    // CARGO_MANIFEST_DIR = daemon/; the fixture lives at the workspace root.
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/household-01.jsonl")
}

struct Expected {
    total: i64,
    blocked: i64,
    domains: i64,
    clients: i64,
}

fn expected_from_fixture() -> Expected {
    let raw = std::fs::read_to_string(fixture_path()).unwrap();
    let events: Vec<QueryEvent> = raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let domains: HashSet<_> = events.iter().map(|e| e.domain.clone()).collect();
    let clients: HashSet<_> = events.iter().map(|e| e.client_key()).collect();
    Expected {
        total: events.len() as i64,
        blocked: events.iter().filter(|e| e.blocked).count() as i64,
        domains: domains.len() as i64,
        clients: clients.len() as i64,
    }
}

/// Drives an ingestor against a store until it reports no new events.
async fn drain(store: &Store, ingestor: &mut FixtureReplayer, max_batches: usize) -> usize {
    let mut applied = 0;
    for _ in 0..max_batches {
        let cursor = store.cursor(ingestor.source_id()).unwrap();
        let batch = ingestor.poll(cursor.as_deref()).await.unwrap();
        if batch.events.is_empty() && batch.next_cursor == cursor {
            break;
        }
        store
            .apply_batch(
                ingestor.source_id(),
                ingestor.kind(),
                &batch.events,
                batch.next_cursor.as_deref(),
                0,
            )
            .unwrap();
        applied += 1;
    }
    applied
}

#[tokio::test]
async fn full_fixture_ingests_exactly_once_across_a_restart() {
    let expected = expected_from_fixture();
    assert!(expected.total > 5_000, "fixture should be non-trivial");

    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("e2e.db");

    // Run 1: ingest exactly 3 batches (3000 events), then "crash".
    {
        let store = Store::open(&db).unwrap();
        let mut replayer = FixtureReplayer::from_path("fixture", &fixture_path(), 1000).unwrap();
        let applied = drain(&store, &mut replayer, 3).await;
        assert_eq!(applied, 3);
        let mid = store.stats().unwrap();
        assert_eq!(mid.total_queries, 3000, "3 full batches before the crash");
    } // both store handle and ingestor dropped here — a real process death

    // Run 2: fresh store handle on the same file, fresh replayer; resume from
    // the persisted cursor only.
    let store = Store::open(&db).unwrap();
    let mut replayer = FixtureReplayer::from_path("fixture", &fixture_path(), 1000).unwrap();
    drain(&store, &mut replayer, usize::MAX >> 1).await;

    let stats = store.stats().unwrap();
    assert_eq!(stats.total_queries, expected.total, "zero loss, zero dup");
    assert_eq!(stats.total_blocked, expected.blocked);
    assert_eq!(stats.distinct_domains, expected.domains);
    assert_eq!(stats.distinct_clients, expected.clients);
    assert_eq!(
        store.cursor("fixture").unwrap().unwrap(),
        expected.total.to_string(),
        "cursor ends exactly at the fixture length"
    );
}
