use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};

use futures::TryStreamExt;
use serde::{Serialize, de::DeserializeOwned};

use couchbase::cluster::Cluster;
use couchbase::error::{Error, ErrorKind};
use couchbase::options::query_options::{QueryOptions, ScanConsistency};

use super::StateStore;

/// Create query options with request_plus scan consistency.
/// Ensures N1QL queries see all prior KV mutations (no stale reads).
fn consistent_query_opts() -> QueryOptions {
    QueryOptions::new().scan_consistency(ScanConsistency::RequestPlus)
}

// ── Operation counters for instrumentation ─────────────────────────

static GET_COUNT: AtomicU64 = AtomicU64::new(0);
static GET_TIME_US: AtomicU64 = AtomicU64::new(0);
static UPSERT_COUNT: AtomicU64 = AtomicU64::new(0);
static UPSERT_TIME_US: AtomicU64 = AtomicU64::new(0);
static REMOVE_COUNT: AtomicU64 = AtomicU64::new(0);
static REMOVE_TIME_US: AtomicU64 = AtomicU64::new(0);
static KEYS_COUNT: AtomicU64 = AtomicU64::new(0);
static KEYS_TIME_US: AtomicU64 = AtomicU64::new(0);
static VALUES_COUNT: AtomicU64 = AtomicU64::new(0);
static VALUES_TIME_US: AtomicU64 = AtomicU64::new(0);
static QUERY_COUNT: AtomicU64 = AtomicU64::new(0);
static QUERY_TIME_US: AtomicU64 = AtomicU64::new(0);
static FLUSH_COUNT: AtomicU64 = AtomicU64::new(0);
static FLUSH_TIME_US: AtomicU64 = AtomicU64::new(0);
static DUMP_COUNT: AtomicU64 = AtomicU64::new(0);
static DUMP_TIME_US: AtomicU64 = AtomicU64::new(0);

/// Print and reset all operation counters.
pub fn print_and_reset_stats() {
    let stats = [
        ("get", GET_COUNT.swap(0, Ordering::Relaxed), GET_TIME_US.swap(0, Ordering::Relaxed)),
        ("upsert", UPSERT_COUNT.swap(0, Ordering::Relaxed), UPSERT_TIME_US.swap(0, Ordering::Relaxed)),
        ("remove", REMOVE_COUNT.swap(0, Ordering::Relaxed), REMOVE_TIME_US.swap(0, Ordering::Relaxed)),
        ("keys", KEYS_COUNT.swap(0, Ordering::Relaxed), KEYS_TIME_US.swap(0, Ordering::Relaxed)),
        ("values", VALUES_COUNT.swap(0, Ordering::Relaxed), VALUES_TIME_US.swap(0, Ordering::Relaxed)),
        ("query", QUERY_COUNT.swap(0, Ordering::Relaxed), QUERY_TIME_US.swap(0, Ordering::Relaxed)),
        ("flush", FLUSH_COUNT.swap(0, Ordering::Relaxed), FLUSH_TIME_US.swap(0, Ordering::Relaxed)),
        ("dump", DUMP_COUNT.swap(0, Ordering::Relaxed), DUMP_TIME_US.swap(0, Ordering::Relaxed)),
    ];
    let total_count: u64 = stats.iter().map(|(_, c, _)| c).sum();
    let total_us: u64 = stats.iter().map(|(_, _, t)| t).sum();
    eprintln!("progress:stats total={}ops {}ms", total_count, total_us / 1000);
    for (name, count, time_us) in &stats {
        if *count > 0 {
            let avg = if *count > 0 { time_us / count } else { 0 };
            eprintln!("progress:stats   {}: {}x, {}ms total, {}us avg", name, count, time_us / 1000, avg);
        }
    }
}

/// A Couchbase bucket connection that implements `StateStore`.
///
/// Used in `Stores` for simulation reset/dump.
/// Also serves as a factory for `CouchbaseCollection` handles.
#[derive(Clone)]
pub struct CouchbaseBucket {
    cluster: Cluster,
    bucket_name: String,
}

impl CouchbaseBucket {
    /// Connect to a Couchbase cluster and target a bucket.
    pub async fn connect(
        connection_string: &str,
        options: couchbase::options::cluster_options::ClusterOptions,
        bucket: &str,
    ) -> Result<Self, Error> {
        let cluster = Cluster::connect(connection_string, options).await?;
        Ok(Self {
            cluster,
            bucket_name: bucket.to_string(),
        })
    }

    /// Get a typed collection handle for use in state machine actions.
    pub fn collection<V>(&self, scope: &str, collection: &str) -> CouchbaseCollection<V>
    where
        V: Clone + Serialize + DeserializeOwned + Send + Sync,
    {
        CouchbaseCollection {
            cluster: self.cluster.clone(),
            bucket_name: self.bucket_name.clone(),
            scope_name: scope.to_string(),
            collection_name: collection.to_string(),
            _phantom: PhantomData,
        }
    }
}

impl StateStore for CouchbaseBucket {
    async fn reset(&self) {
        let t = std::time::Instant::now();
        // Discover all non-system collections and DELETE FROM each.
        // Much faster than bucket flush (~100ms vs ~3000ms).
        let query = format!(
            "SELECT RAW c.name FROM system:keyspaces AS c \
             WHERE c.`bucket` = '{}' AND c.`scope` = '_default' AND c.name NOT LIKE '_%%'",
            self.bucket_name
        );
        let collections: Vec<String> = match self.cluster.query(&query, None).await {
            Ok(mut result) => result.rows::<String>().try_collect().await.unwrap_or_default(),
            Err(e) => {
                eprintln!("[cb] reset: failed to discover collections: {}, falling back to flush", e);
                let manager = self.cluster.buckets();
                let _ = manager.flush_bucket(&self.bucket_name, None).await;
                FLUSH_COUNT.fetch_add(1, Ordering::Relaxed);
                FLUSH_TIME_US.fetch_add(t.elapsed().as_micros() as u64, Ordering::Relaxed);
                return;
            }
        };
        for coll in &collections {
            let del = format!("DELETE FROM `{}`.`_default`.`{}`", self.bucket_name, coll);
            if let Err(e) = self.cluster.query(&del, None).await {
                eprintln!("[cb] reset: DELETE FROM {} failed: {}", coll, e);
            }
        }
        FLUSH_COUNT.fetch_add(1, Ordering::Relaxed);
        FLUSH_TIME_US.fetch_add(t.elapsed().as_micros() as u64, Ordering::Relaxed);
        eprintln!("[cb] reset {}ms ({} collections)", t.elapsed().as_millis(), collections.len());
    }

    async fn dump(&self) -> serde_json::Value {
        let t = std::time::Instant::now();
        let mut result = serde_json::Map::new();

        // Discover all scopes and collections, then dump each
        let scopes_query = format!(
            "SELECT s.name AS scope_name, c.name AS collection_name \
             FROM system:keyspaces AS c \
             JOIN system:keyspaces AS s ON c.`namespace` = s.`namespace` AND c.`bucket` = s.`bucket` AND c.`scope` = s.name \
             WHERE c.`bucket` = '{}' AND s.`datastore_id` IS NOT MISSING AND c.`scope` IS NOT MISSING",
            self.bucket_name
        );
        // Fallback: try querying known collections via a simpler approach
        let collections = match self.cluster.query(
            format!("SELECT RAW name FROM system:keyspaces WHERE `bucket` = '{}' AND `scope` IS NOT MISSING", self.bucket_name),
            consistent_query_opts(),
        ).await {
            Ok(mut r) => r.rows::<String>().try_collect().await.unwrap_or_default(),
            Err(_) => vec![],
        };
        let _ = scopes_query; // used the simpler approach above

        for collection in &collections {
            let query = format!(
                "SELECT META().id AS `__id`, `{collection}`.* FROM `{}`.`_default`.`{collection}`",
                self.bucket_name
            );
            match self.cluster.query(query, consistent_query_opts()).await {
                Ok(mut r) => {
                    let rows: Vec<serde_json::Value> = r.rows::<serde_json::Value>().try_collect().await.unwrap_or_default();
                    if !rows.is_empty() {
                        result.insert(collection.clone(), serde_json::Value::Array(rows));
                    }
                }
                Err(_) => {}
            }
        }

        DUMP_COUNT.fetch_add(1, Ordering::Relaxed);
        DUMP_TIME_US.fetch_add(t.elapsed().as_micros() as u64, Ordering::Relaxed);
        serde_json::Value::Object(result)
    }
}

/// A typed handle to a specific Couchbase collection.
///
/// Provides typed CRUD and raw N1QL query access. All operations
/// are async and hit the real database — same code runs in
/// simulation and production.
#[derive(Clone)]
pub struct CouchbaseCollection<V> {
    cluster: Cluster,
    bucket_name: String,
    scope_name: String,
    collection_name: String,
    _phantom: PhantomData<V>,
}

impl<V> CouchbaseCollection<V>
where
    V: Clone + Serialize + DeserializeOwned + Send + Sync,
{
    fn cb_collection(&self) -> couchbase::collection::Collection {
        self.cluster
            .bucket(&self.bucket_name)
            .scope(&self.scope_name)
            .collection(&self.collection_name)
    }

    /// Get a document by key. Returns `None` if not found.
    pub async fn get(&self, key: &str) -> Option<V> {
        let t = std::time::Instant::now();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.cb_collection().get(key, None),
        ).await;
        GET_COUNT.fetch_add(1, Ordering::Relaxed);
        GET_TIME_US.fetch_add(t.elapsed().as_micros() as u64, Ordering::Relaxed);
        match result {
            Ok(Ok(result)) => result.content_as::<V>().ok(),
            Ok(Err(e)) if *e.kind() == ErrorKind::DocumentNotFound => None,
            Ok(Err(e)) => {
                eprintln!("[cb] get error on {}.{}.{} key={}: {}", self.bucket_name, self.scope_name, self.collection_name, key, e);
                None
            }
            Err(_) => {
                eprintln!("[cb] get timeout on {}.{}.{} key={}", self.bucket_name, self.scope_name, self.collection_name, key);
                None
            }
        }
    }

    /// Insert or update a document.
    pub async fn upsert(&self, key: &str, value: &V) {
        let t = std::time::Instant::now();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.cb_collection().upsert(key, value, None),
        ).await;
        UPSERT_COUNT.fetch_add(1, Ordering::Relaxed);
        UPSERT_TIME_US.fetch_add(t.elapsed().as_micros() as u64, Ordering::Relaxed);
        match result {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => eprintln!("[cb] upsert error on {}.{}.{} key={}: {}", self.bucket_name, self.scope_name, self.collection_name, key, e),
            Err(_) => eprintln!("[cb] upsert timeout on {}.{}.{} key={}", self.bucket_name, self.scope_name, self.collection_name, key),
        }
    }

    /// Remove a document by key. Returns the previous value if it existed.
    pub async fn remove(&self, key: &str) -> Option<V> {
        let existing = self.get(key).await;
        if existing.is_some() {
            let t = std::time::Instant::now();
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                self.cb_collection().remove(key, None),
            ).await;
            REMOVE_COUNT.fetch_add(1, Ordering::Relaxed);
            REMOVE_TIME_US.fetch_add(t.elapsed().as_micros() as u64, Ordering::Relaxed);
            if let Err(_) | Ok(Err(_)) = result {
                eprintln!("[cb] remove error on {}.{}.{} key={}", self.bucket_name, self.scope_name, self.collection_name, key);
            }
        }
        existing
    }

    /// Check if a document exists by attempting a get.
    pub async fn exists(&self, key: &str) -> bool {
        self.get(key).await.is_some()
    }

    /// Get all document keys via N1QL.
    pub async fn keys(&self) -> Vec<String> {
        let t = std::time::Instant::now();
        let query = format!(
            "SELECT RAW META().id FROM `{}`.`{}`.`{}`",
            self.bucket_name, self.scope_name, self.collection_name
        );
        let result = match self.cluster.query(query, consistent_query_opts()).await {
            Ok(mut result) => result.rows::<String>().try_collect().await.unwrap_or_default(),
            Err(e) => {
                eprintln!("[cb] keys error on {}.{}.{}: {}", self.bucket_name, self.scope_name, self.collection_name, e);
                vec![]
            }
        };
        KEYS_COUNT.fetch_add(1, Ordering::Relaxed);
        KEYS_TIME_US.fetch_add(t.elapsed().as_micros() as u64, Ordering::Relaxed);
        result
    }

    /// Get all document values via N1QL.
    pub async fn values(&self) -> Vec<V> {
        let t = std::time::Instant::now();
        let query = format!(
            "SELECT RAW `{}` FROM `{}`.`{}`.`{}`",
            self.collection_name, self.bucket_name, self.scope_name, self.collection_name
        );
        let result = match self.cluster.query(query, consistent_query_opts()).await {
            Ok(mut result) => result.rows::<V>().try_collect().await.unwrap_or_default(),
            Err(e) => {
                eprintln!("[cb] values error on {}.{}.{}: {}", self.bucket_name, self.scope_name, self.collection_name, e);
                vec![]
            }
        };
        VALUES_COUNT.fetch_add(1, Ordering::Relaxed);
        VALUES_TIME_US.fetch_add(t.elapsed().as_micros() as u64, Ordering::Relaxed);
        result
    }

    /// Execute a raw N1QL query returning typed results.
    pub async fn query_n1ql<T: DeserializeOwned>(&self, statement: &str, options: Option<QueryOptions>) -> Vec<T> {
        let t = std::time::Instant::now();
        let opts = options.unwrap_or_else(consistent_query_opts);
        let result = match self.cluster.query(statement, opts).await {
            Ok(mut result) => result.rows::<T>().try_collect().await.unwrap_or_default(),
            Err(e) => {
                eprintln!("[cb] query error: {}: {}", statement, e);
                vec![]
            }
        };
        QUERY_COUNT.fetch_add(1, Ordering::Relaxed);
        QUERY_TIME_US.fetch_add(t.elapsed().as_micros() as u64, Ordering::Relaxed);
        result
    }
}
