use std::future::Future;

// ── StateStore trait ───────────────────────────────────────────────

/// Trait for database backends used in state machine simulation.
///
/// Implementations handle two simulation-critical operations:
/// - **reset**: Clear all state between simulation traces
/// - **dump**: Snapshot all state for violation reports
///
/// Any database can participate in simulation by implementing this trait.
/// The simulation engine calls `reset()` before each trace and `dump()`
/// when a constraint violation is detected.
pub trait StateStore: Send + Sync {
    fn reset(&self) -> impl Future<Output = ()> + Send;
    fn dump(&self) -> impl Future<Output = serde_json::Value> + Send;
}

// ── Stores ─────────────────────────────────────────────────────────

/// Container for all state stores backing a state machine.
///
/// Wraps a `Vec<Box<dyn StateStoreDyn>>` and provides aggregate
/// `reset()` and `dump()` operations across all stores.
pub struct Stores(Vec<Box<dyn StateStoreDyn>>);

impl Stores {
    pub fn new(stores: Vec<Box<dyn StateStoreDyn>>) -> Self {
        Self(stores)
    }

    /// Reset all stores (clear state between simulation traces).
    pub async fn reset(&self) {
        for store in &self.0 {
            store.reset_dyn().await;
        }
    }

    /// Dump all store state as a JSON string (for violation reports).
    pub async fn dump(&self) -> String {
        let mut result = serde_json::Map::new();
        for (i, store) in self.0.iter().enumerate() {
            let dump = store.dump_dyn().await;
            // Use the store's self-reported name if available, else index
            if let serde_json::Value::Object(map) = &dump {
                for (k, v) in map {
                    result.insert(k.clone(), v.clone());
                }
            } else {
                result.insert(format!("store_{i}"), dump);
            }
        }
        serde_json::to_string_pretty(&serde_json::Value::Object(result)).unwrap_or_default()
    }
}

// ── Object-safe wrapper ────────────────────────────────────────────

/// Object-safe version of `StateStore` for use in `Vec<Box<dyn ...>>`.
///
/// `StateStore` uses `impl Future` return types which are not object-safe.
/// This trait provides the same operations as boxed futures, and is
/// auto-implemented for all `StateStore` types.
pub trait StateStoreDyn: Send + Sync {
    fn reset_dyn(&self) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + '_>>;
    fn dump_dyn(&self) -> std::pin::Pin<Box<dyn Future<Output = serde_json::Value> + Send + '_>>;
}

impl<T: StateStore> StateStoreDyn for T {
    fn reset_dyn(&self) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(self.reset())
    }

    fn dump_dyn(&self) -> std::pin::Pin<Box<dyn Future<Output = serde_json::Value> + Send + '_>> {
        Box::pin(self.dump())
    }
}

#[cfg(feature = "couchbase")]
pub mod couchbase;
#[cfg(feature = "redpanda")]
pub mod redpanda;
