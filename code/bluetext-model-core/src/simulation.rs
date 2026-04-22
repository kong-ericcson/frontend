use serde::{Deserialize, Serialize};
use rand::Rng;
use rand::rngs::SmallRng;
use rand::SeedableRng;
use std::cell::RefCell;

// ── Configuration ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CheckFrequency {
    EveryStep,
    EndOfRun,
}

impl Default for CheckFrequency {
    fn default() -> Self { CheckFrequency::EndOfRun }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationConfig {
    pub max_samples: u64,
    pub max_steps: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    pub init_name: String,
    pub step_name: String,
    pub state_invariants: Vec<String>,
    #[serde(default)]
    pub check_frequency: CheckFrequency,
}

// ── Results ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationResult {
    pub status: SimulationStatus,
    pub output: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub violation: Option<ViolationInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SimulationStatus {
    Ok,
    Violation,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViolationInfo {
    pub constraint_name: String,
    pub step: u64,
    pub sample: u64,
    pub sample_seed: u64,
    pub state: String,
    pub trace: Vec<TraceEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEntry {
    pub step: u64,
    pub action: Option<String>,
    pub state: String,
}

// ── Simulation trait ────────────────────────────────────────────────

/// Implemented by the #[state_machine_impl] macro.
/// Provides the hooks needed to run simulation against real databases.
///
/// Unlike the previous sync trait, all methods are async — actions and
/// constraints can perform real database operations. State is reset
/// between traces via `reset()` and dumped on violations via `dump()`.
pub trait Simulatable {
    /// Available simulation_init method names.
    fn available_simulation_inits() -> Vec<&'static str>;
    /// Available simulation_step method names.
    fn available_simulation_steps() -> Vec<&'static str>;
    /// Available state invariant method names.
    fn available_state_invariants() -> Vec<&'static str>;

    /// Reset all state stores to empty (between simulation traces).
    fn reset(&mut self) -> impl std::future::Future<Output = ()> + Send;
    /// Dump all state as a JSON string (for violation reports).
    fn dump(&self) -> impl std::future::Future<Output = String> + Send;

    /// Call the named simulation_init method.
    fn call_simulation_init(&mut self, name: &str) -> impl std::future::Future<Output = bool> + Send;
    /// Call the named simulation_step method. Returns true if the step made progress.
    fn call_simulation_step(&mut self, name: &str) -> impl std::future::Future<Output = bool> + Send;
    /// Check the named state invariant. Returns true if it holds.
    fn check_state_invariant(&self, name: &str) -> impl std::future::Future<Output = bool> + Send;

    /// Call a mutation by name with JSON arguments. Used by auto-step.
    fn call_mutation(&mut self, name: &str, args: &serde_json::Value) -> impl std::future::Future<Output = bool> + Send;
    /// Get all keys from a named state variable collection. Used by auto-step for ID sampling.
    fn collection_keys(&self, name: &str) -> impl std::future::Future<Output = Vec<String>> + Send;
}

/// Run simulation against a Simulatable state machine.
///
/// Takes a mutable reference to an already-wired state machine (with DB
/// connections established). Calls `reset()` between traces instead of
/// creating new instances via `Default`.
pub async fn simulate<S: Simulatable>(state: &mut S, config: &SimulationConfig) -> SimulationResult {
    // Test store connectivity before running traces
    eprintln!("progress:connecting");
    state.reset().await;
    eprintln!("progress:connected");

    let seed = config.seed.unwrap_or_else(|| {
        rand::rng().random()
    });

    let mut total_steps: u64 = 0;
    let mut max_trace_len: u64 = 0;
    let mut min_trace_len: u64 = u64::MAX;
    let mut last_trace_entries: Vec<TraceEntry> = Vec::new();

    for sample in 0..config.max_samples {
        let run_start = std::time::Instant::now();
        let run_reset_ms: u128;
        let mut run_action_ms: u128 = 0;
        let mut run_constraint_ms: u128 = 0;

        // Report progress to stderr (stdout reserved for JSON result)
        eprintln!("progress:{}/{}", sample + 1, config.max_samples);

        let sample_seed = seed.wrapping_add(sample);
        THREAD_RNG.with(|cell| {
            *cell.borrow_mut() = Some(SmallRng::seed_from_u64(sample_seed));
        });

        // Reset state stores to empty for this trace
        let t_reset = std::time::Instant::now();
        state.reset().await;
        run_reset_ms = t_reset.elapsed().as_millis();

        if !state.call_simulation_init(&config.init_name).await {
            return SimulationResult {
                status: SimulationStatus::Error,
                output: format!("Init '{}' failed or not found", config.init_name),
                seed: Some(seed),
                stats: None,
                violation: None,
            };
        }

        // Track actions for trace (state dumped only on violation)
        let mut trace_actions: Vec<(u64, Option<String>)> = vec![(0, Some("init".to_string()))];

        // Check state invariants after init (parallel)
        {
            let constraint_futs: Vec<_> = config.state_invariants.iter()
                .map(|inv| state.check_state_invariant(inv))
                .collect();
            let constraint_results: Vec<bool> = futures::future::join_all(constraint_futs).await;
            for (inv, holds) in config.state_invariants.iter().zip(constraint_results) {
                if !holds {
                    // Dump full trace on violation
                    let state_dump = state.dump().await;
                    let trace = build_violation_trace(state, &trace_actions).await;
                    return SimulationResult {
                        status: SimulationStatus::Violation,
                        output: format!(
                            "[violation] Constraint '{}' failed after init (run {}, seed {})",
                            inv, sample + 1, sample_seed
                        ),
                        seed: Some(seed),
                        stats: None,
                        violation: Some(ViolationInfo {
                            constraint_name: inv.clone(),
                            step: 0,
                            sample: sample + 1,
                            sample_seed,
                            state: state_dump,
                            trace,
                        }),
                    };
                }
            }
        }

        let mut trace_len: u64 = 0;
        for step_num in 1..=config.max_steps {
            let step_start = std::time::Instant::now();

            // Retry the step up to 100 times (nondeterministic choice may fail)
            let mut progressed = false;
            let mut last_tried_action = None;
            for _ in 0..100 {
                let _ = take_last_action(); // clear before step
                let ok = state.call_simulation_step(&config.step_name).await;
                if let Some(action) = take_last_action() {
                    last_tried_action = Some(action);
                }
                if ok {
                    progressed = true;
                    break;
                }
            }
            let action_ms = step_start.elapsed().as_millis();

            trace_len = step_num;

            if !progressed {
                let step_ms = step_start.elapsed().as_millis();
                eprintln!("progress:{}/{} step {}/{} (no progress) {}ms", sample + 1, config.max_samples, step_num, config.max_steps, step_ms);
                break;
            }

            // Record action (no state dump — only on violation)
            trace_actions.push((step_num, last_tried_action.clone()));

            // Check state invariants after each step (if configured)
            let constraint_ms = if config.check_frequency == CheckFrequency::EveryStep {
                let constraint_start = std::time::Instant::now();
                let constraint_futs: Vec<_> = config.state_invariants.iter()
                    .map(|inv| state.check_state_invariant(inv))
                    .collect();
                let constraint_results: Vec<bool> = futures::future::join_all(constraint_futs).await;
                for (inv, holds) in config.state_invariants.iter().zip(constraint_results) {
                    if !holds {
                        let violation_state = state.dump().await;
                        let trace = build_violation_trace(state, &trace_actions).await;
                        return SimulationResult {
                            status: SimulationStatus::Violation,
                            output: format!(
                                "[violation] Constraint '{}' failed at step {} (run {}, seed {})",
                                inv, step_num, sample + 1, sample_seed
                            ),
                            seed: Some(seed),
                            stats: None,
                            violation: Some(ViolationInfo {
                                constraint_name: inv.clone(),
                                step: step_num,
                                sample: sample + 1,
                                sample_seed,
                                state: violation_state,
                                trace,
                            }),
                        };
                    }
                }
                constraint_start.elapsed().as_millis()
            } else {
                0
            };

            // Report step time
            let step_ms = step_start.elapsed().as_millis();
            run_action_ms += action_ms;
            run_constraint_ms += constraint_ms;
            if let Some(ref action) = last_tried_action {
                eprintln!("progress:{}/{} step {}/{} action:{} {}ms (action {}ms, constraints {}ms)",
                    sample + 1, config.max_samples, step_num, config.max_steps, action, step_ms, action_ms, constraint_ms);
            } else {
                eprintln!("progress:{}/{} step {}/{} {}ms",
                    sample + 1, config.max_samples, step_num, config.max_steps, step_ms);
            }
        }

        // Check state invariants at end of run (if end_of_run mode or always after init)
        if config.check_frequency == CheckFrequency::EndOfRun && trace_len > 0 {
            let constraint_start = std::time::Instant::now();
            let constraint_futs: Vec<_> = config.state_invariants.iter()
                .map(|inv| state.check_state_invariant(inv))
                .collect();
            let constraint_results: Vec<bool> = futures::future::join_all(constraint_futs).await;
            let check_ms = constraint_start.elapsed().as_millis();
            run_constraint_ms += check_ms;
            for (inv, holds) in config.state_invariants.iter().zip(constraint_results) {
                if !holds {
                    let violation_state = state.dump().await;
                    let trace = build_violation_trace(state, &trace_actions).await;
                    return SimulationResult {
                        status: SimulationStatus::Violation,
                        output: format!(
                            "[violation] Constraint '{}' failed at end of run {} (seed {})",
                            inv, sample + 1, sample_seed
                        ),
                        seed: Some(seed),
                        stats: None,
                        violation: Some(ViolationInfo {
                            constraint_name: inv.clone(),
                            step: trace_len,
                            sample: sample + 1,
                            sample_seed,
                            state: violation_state,
                            trace,
                        }),
                    };
                }
            }
        }

        // Dump full state at end of run (compact single-line JSON)
        let state_dump = state.dump().await;
        let compact_dump = match serde_json::from_str::<serde_json::Value>(&state_dump) {
            Ok(v) => serde_json::to_string(&v).unwrap_or(state_dump),
            Err(_) => state_dump,
        };
        eprintln!("progress:{}/{} state:{}", sample + 1, config.max_samples, compact_dump);

        let run_ms = run_start.elapsed().as_millis();
        let other_ms = run_ms.saturating_sub(run_reset_ms + run_action_ms + run_constraint_ms);
        eprintln!("progress:{}/{} done {}ms (reset {}ms, actions {}ms, constraints {}ms, other {}ms)",
            sample + 1, config.max_samples, run_ms, run_reset_ms, run_action_ms, run_constraint_ms, other_ms);
        #[cfg(feature = "couchbase")]
        crate::stores::couchbase::print_and_reset_stats();

        total_steps += trace_len;
        max_trace_len = max_trace_len.max(trace_len);
        if trace_len > 0 {
            min_trace_len = min_trace_len.min(trace_len);
        }
        if sample == config.max_samples - 1 {
            last_trace_entries = trace_actions.iter().map(|(step, action)| {
                TraceEntry { step: *step, action: action.clone(), state: String::new() }
            }).collect();
        }
    }

    THREAD_RNG.with(|cell| {
        *cell.borrow_mut() = None;
    });

    if min_trace_len == u64::MAX {
        min_trace_len = 0;
    }
    let avg = if config.max_samples > 0 {
        total_steps / config.max_samples
    } else {
        0
    };

    // Format last trace for output
    let trace_text: String = last_trace_entries.iter().map(|e| {
        let action = e.action.as_deref().unwrap_or("?");
        format!("[Step {}] {}\n{}", e.step, action, e.state)
    }).collect::<Vec<_>>().join("\n");

    SimulationResult {
        status: SimulationStatus::Ok,
        output: format!(
            "[ok] No violation found ({} samples, seed {})\n\n{}",
            config.max_samples, seed, trace_text
        ),
        seed: Some(seed),
        stats: Some(format!(
            "Trace length statistics: min={}, max={}, avg={}",
            min_trace_len, max_trace_len, avg
        )),
        violation: None,
    }
}

/// Replay the trace to build full state dumps for violation reporting.
/// Only called on violation — not on every step.
async fn build_violation_trace<S: Simulatable>(
    _state: &S,
    trace_actions: &[(u64, Option<String>)],
) -> Vec<TraceEntry> {
    // We can't replay the trace (actions are nondeterministic), so we
    // record the action names without state. The violation's own state
    // dump captures the final state.
    trace_actions.iter().map(|(step, action)| {
        TraceEntry {
            step: *step,
            action: action.clone(),
            state: String::new(),
        }
    }).collect()
}

// ── Auto-step: random mutation selection ──────────────────────────────

use crate::metadata::{StateMachineMeta, StateMachineModel, TypeKind};

/// Automatically pick a random mutation and generate valid arguments.
/// Used when no `#[simulation_step]` is defined.
pub async fn auto_step<S: Simulatable + StateMachineModel>(state: &mut S) -> bool {
    let meta = S::metadata();
    if meta.mutations.is_empty() { return false; }

    let mutation = &meta.mutations[rand_usize(meta.mutations.len())];
    let args = generate_random_args(&mutation.params, &meta, state).await;
    trace_action(&mutation.name);
    state.call_mutation(&mutation.name, &args).await
}

/// Generate random JSON arguments for a mutation's parameters.
async fn generate_random_args<S: Simulatable>(
    params: &[crate::metadata::FieldMeta],
    meta: &StateMachineMeta,
    state: &S,
) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    for param in params {
        let val = generate_random_value(&param.type_name, &param.references, &param.key_for, meta, state).await;
        obj.insert(param.name.clone(), val);
    }
    serde_json::Value::Object(obj)
}

/// Generate a random JSON value for a given type.
async fn generate_random_value<S: Simulatable>(
    type_name: &str,
    references: &Option<String>,
    key_for: &Option<String>,
    meta: &StateMachineMeta,
    state: &S,
) -> serde_json::Value {
    // If this param is a key for a specific collection, sample from it
    if let Some(collection) = key_for {
        let keys = state.collection_keys(collection).await;
        if let Some(key) = one_of(&keys) {
            return serde_json::Value::String(key);
        }
        return serde_json::Value::String(uuid().to_string());
    }

    // If this field references another type, sample an existing ID from the collection
    if let Some(ref_type) = references {
        if let Some(collection) = find_collection_for_type(ref_type, meta) {
            let keys = state.collection_keys(&collection).await;
            if let Some(key) = one_of(&keys) {
                return serde_json::Value::String(key);
            }
        }
        return serde_json::Value::String(uuid().to_string());
    }

    match type_name {
        "String" => serde_json::Value::String(uuid().to_string()),
        "i64" => serde_json::json!(rand_range(1, 100)),
        "u64" => serde_json::json!(rand_range(1, 100)),
        "i32" => serde_json::json!(rand_range(1, 100)),
        "u32" => serde_json::json!(rand_range(1, 100)),
        "f64" => serde_json::json!(rand_range(1, 10000) as f64 / 100.0),
        "bool" => serde_json::json!(rand_usize(2) == 0),
        t if t.starts_with("Option<") => {
            if rand_usize(3) == 0 {
                serde_json::Value::Null
            } else {
                let inner = &t[7..t.len() - 1];
                Box::pin(generate_random_value(inner, &None, &None, meta, state)).await
            }
        }
        t if t.starts_with("Vec<") => {
            let inner = &t[4..t.len() - 1];
            let len = rand_usize(3);
            let mut arr = Vec::new();
            for _ in 0..len {
                arr.push(Box::pin(generate_random_value(inner, &None, &None, meta, state)).await);
            }
            serde_json::Value::Array(arr)
        }
        _ => {
            // Look up in type definitions
            if let Some(type_def) = meta.types.iter().find(|t| t.name == type_name) {
                match type_def.kind {
                    TypeKind::Enum => {
                        if type_def.fields.is_empty() {
                            serde_json::Value::Null
                        } else {
                            let variant = &type_def.fields[rand_usize(type_def.fields.len())];
                            serde_json::Value::String(variant.name.clone())
                        }
                    }
                    TypeKind::Record => {
                        let mut obj = serde_json::Map::new();
                        for field in &type_def.fields {
                            let val = Box::pin(generate_random_value(
                                &field.type_name, &field.references, &field.key_for, meta, state,
                            )).await;
                            obj.insert(field.name.clone(), val);
                        }
                        serde_json::Value::Object(obj)
                    }
                }
            } else {
                serde_json::Value::Null
            }
        }
    }
}

/// Find the state variable (collection) name that holds values of the given type.
fn find_collection_for_type(type_name: &str, meta: &StateMachineMeta) -> Option<String> {
    for sv in &meta.state_vars {
        // Match CouchbaseCollection<Type> or HashMap<String, Type>
        if sv.type_refs.iter().any(|r| r == type_name) {
            return Some(sv.name.clone());
        }
    }
    None
}

// ── Thread-local RNG for nondeterministic choice in actions ─────────

thread_local! {
    static THREAD_RNG: RefCell<Option<SmallRng>> = const { RefCell::new(None) };
    static LAST_ACTION: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Record the name of the action being executed.
/// Call this from within `#[step]` methods to report which action was chosen.
pub fn trace_action(name: &str) {
    LAST_ACTION.with(|cell| {
        *cell.borrow_mut() = Some(name.to_string());
    });
}

fn take_last_action() -> Option<String> {
    LAST_ACTION.with(|cell| cell.borrow_mut().take())
}

/// Pick a random usize in [0, max). For use in #[step] methods.
pub fn rand_usize(max: usize) -> usize {
    if max == 0 {
        return 0;
    }
    THREAD_RNG.with(|cell| {
        let mut borrow = cell.borrow_mut();
        match borrow.as_mut() {
            Some(rng) => rng.random_range(0..max),
            None => rand::rng().random_range(0..max),
        }
    })
}

/// Pick a random i64 in [min, max]. For use in #[step] methods.
pub fn rand_range(min: i64, max: i64) -> i64 {
    if min >= max {
        return min;
    }
    THREAD_RNG.with(|cell| {
        let mut borrow = cell.borrow_mut();
        match borrow.as_mut() {
            Some(rng) => rng.random_range(min..=max),
            None => rand::rng().random_range(min..=max),
        }
    })
}

/// Pick a random element from a slice. Returns None if empty.
pub fn one_of<T: Clone>(items: &[T]) -> Option<T> {
    if items.is_empty() {
        None
    } else {
        Some(items[rand_usize(items.len())].clone())
    }
}

// ── Controlled nondeterminism: now() and uuid() ─────────────────────

thread_local! {
    static THREAD_TIME: RefCell<Option<chrono::DateTime<chrono::Utc>>> = const { RefCell::new(None) };
}

/// Get current timestamp. In simulation, returns a seeded/controlled value.
/// In production (no simulation context), returns real wall clock time.
///
/// The simulation engine sets a base time before each trace. Each call
/// to `now()` within a trace advances the time by 1 millisecond to
/// ensure unique, ordered timestamps.
pub fn now() -> chrono::DateTime<chrono::Utc> {
    THREAD_TIME.with(|cell| {
        let mut borrow = cell.borrow_mut();
        match borrow.as_mut() {
            Some(time) => {
                let current = *time;
                *time = current + chrono::TimeDelta::milliseconds(1);
                current
            }
            None => chrono::Utc::now(),
        }
    })
}

/// Generate a UUID. In simulation, generates from the seeded RNG for
/// reproducibility. In production, generates a real v4 UUID.
pub fn uuid() -> uuid::Uuid {
    THREAD_RNG.with(|cell| {
        let mut borrow = cell.borrow_mut();
        match borrow.as_mut() {
            Some(rng) => {
                let mut bytes = [0u8; 16];
                rng.fill(&mut bytes);
                uuid::Uuid::from_bytes(bytes)
            }
            None => uuid::Uuid::new_v4(),
        }
    })
}
