// TODO: Complete Redpanda store implementation.
//
// This module provides `RedpandaCluster` (implements `StateStore`) and
// `RedpandaTopic<V>` (typed topic handle). The exact rskafka API needs
// to be verified with a running Redpanda instance.
//
// Architecture:
// - RedpandaCluster: wraps rskafka::client::Client + tracked topic names
//   - StateStore::reset() → delete and recreate all tracked topics
//   - StateStore::dump() → consume all messages from all topics → JSON
//   - topic(name) → RedpandaTopic<V>
//
// - RedpandaTopic<V>: typed handle to a specific topic
//   - produce(&V) → publish a JSON-serialized message
//   - consume_all() → Vec<V> (read all messages from offset 0)

use super::StateStore;

/// Placeholder — requires `redpanda` feature and a running Redpanda instance to develop against.
#[derive(Clone)]
pub struct RedpandaCluster {
    _brokers: String,
}

impl StateStore for RedpandaCluster {
    async fn reset(&self) {
        todo!("Implement Redpanda topic delete/recreate")
    }

    async fn dump(&self) -> serde_json::Value {
        todo!("Implement Redpanda consume-all dump")
    }
}

/// Placeholder for typed Redpanda topic handle.
#[derive(Clone)]
pub struct RedpandaTopic<V> {
    _phantom: std::marker::PhantomData<V>,
}
