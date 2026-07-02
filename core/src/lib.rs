//! Phonehome core: the source-agnostic data model and ingestion contract.
//!
//! Everything downstream of the ingestion adapters is backend-blind (D-003):
//! adapters normalize Pi-hole / AdGuard / fixture data into [`QueryEvent`]s and
//! nothing else in the system may know which backend produced them.

mod event;
pub mod ingest;
pub mod replay;

pub use event::QueryEvent;
pub use ingest::{Batch, IngestError, Ingestor};
pub use replay::FixtureReplayer;
