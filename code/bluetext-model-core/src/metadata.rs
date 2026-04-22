use serde::{Deserialize, Serialize};

// ── State Machine Metadata ──────────────────────────────────────────

/// Top-level metadata for a state machine model.
/// Serialized to `model/model.json` for UI consumption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateMachineMeta {
    pub module: String,
    /// Sub-modules for sidebar navigation (each maps to a source file/module).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub modules: Vec<ModuleEntry>,
    pub types: Vec<TypeMeta>,
    pub aliases: Vec<AliasMeta>,
    pub state_vars: Vec<StateVarMeta>,

    // ── Business logic layer ───────────────────────────────────────
    pub mutations: Vec<MutationMeta>,
    pub getters: Vec<GetterMeta>,
    pub commands: Vec<CommandMethodMeta>,
    pub requests: Vec<RequestMeta>,
    pub controllers: Vec<ControllerMeta>,
    pub access: Vec<AccessMeta>,

    // ── Constraints ────────────────────────────────────────────────
    /// State invariants — checked after every mutation.
    pub state_invariants: Vec<StateInvariantMeta>,
    /// Eventual consistency constraints — checked after commands complete.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub eventual_consistency_constraints: Vec<EventualConsistencyConstraintMeta>,

    // ── Simulation ─────────────────────────────────────────────────
    pub simulation_inits: Vec<SimulationInitMeta>,
    pub simulation_steps: Vec<SimulationStepMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleEntry {
    pub name: String,
    /// Path to the source file, relative to project root (e.g. "code/model/src/account.rs").
    pub file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeMeta {
    pub name: String,
    pub kind: TypeKind,
    pub fields: Vec<FieldMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_module: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_end_line: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TypeKind {
    Record,
    Enum,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldMeta {
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
    #[serde(default)]
    pub fields: Vec<FieldMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub references: Option<String>,
    /// State variable collection this param is a key for (from `#[key(collection)]`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_for: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AliasMeta {
    pub name: String,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateVarMeta {
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
    pub type_refs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_module: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_line: Option<u32>,
}

// ── Mutation Metadata ───────────────────────────────────────────────
/// A mutation atomically transitions state machine data.
/// Idempotent. Checked against state invariants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationMeta {
    pub name: String,
    pub params: Vec<FieldMeta>,
    pub modifies: Vec<String>,
    pub reads: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_end_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_module: Option<String>,
}

// ── Getter Metadata ────────────────────────────────────────────────
/// A getter reads state machine data without mutations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetterMeta {
    pub name: String,
    pub params: Vec<FieldMeta>,
    pub reads: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_end_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_module: Option<String>,
}

// ── Command Metadata ───────────────────────────────────────────────
/// A command method orchestrates mutations, getters, and requests.
/// Guarantees eventual consistency via durable execution (Resonate).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandMethodMeta {
    pub name: String,
    pub params: Vec<FieldMeta>,
    /// Which mutations/getters/requests this command calls, in execution order.
    pub calls: Vec<CallMeta>,
    /// Full control flow step tree for flowchart rendering.
    /// Contains if/else, try/catch, parallel, await structures.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub body: Vec<serde_json::Value>,
    /// Whether this command is private (called only by other commands).
    #[serde(default)]
    pub is_private: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_end_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_module: Option<String>,
    /// Constraint groups this command belongs to (for eventual consistency checking).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub constraints: Vec<String>,
}

/// A call to a mutation, getter, or request within a command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallMeta {
    pub name: String,
    pub line: u32,
}

// ── Request Metadata ───────────────────────────────────────────────
/// A request calls a remote service through a client. Idempotent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestMeta {
    pub name: String,
    pub params: Vec<FieldMeta>,
    /// The client/service this request depends on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_dependency: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_end_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_module: Option<String>,
}

// ── Controller Metadata ────────────────────────────────────────────
/// A controller is a thin service endpoint.
/// Calls exactly one command and handles access control.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControllerMeta {
    pub name: String,
    pub method: String,
    pub path: String,
    /// The command this controller calls.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_called: Option<String>,
    /// The access control function this controller uses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_check: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_end_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_module: Option<String>,
}

// ── Access Control Metadata ────────────────────────────────────────
/// An access control function returns true/false for access rights.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessMeta {
    pub name: String,
    pub params: Vec<FieldMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_end_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_module: Option<String>,
}

// ── Constraint Metadata ────────────────────────────────────────────
/// State invariant — checked after every mutation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateInvariantMeta {
    pub name: String,
    pub reads: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_end_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_module: Option<String>,
}

/// Eventual consistency constraint — checked after commands complete.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventualConsistencyConstraintMeta {
    pub name: String,
    pub reads: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_end_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_module: Option<String>,
}

// ── Simulation Metadata ────────────────────────────────────────────
/// Simulation init — sets up initial state for randomized exploration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationInitMeta {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_end_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_module: Option<String>,
}

/// Simulation step — nondeterministic step that calls mutations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationStepMeta {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_end_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_module: Option<String>,
}

// ── Traits for metadata emission ────────────────────────────────────

/// Implemented by #[derive(ModelType)] on structs/enums.
pub trait ModelTypeMeta {
    fn type_meta() -> TypeMeta;
}

/// Implemented by #[derive(StateMachine)] on the root state struct.
pub trait StateMachineModel: Sized {
    fn metadata() -> StateMachineMeta;
    fn emit_model_json() -> String {
        serde_json::to_string_pretty(&Self::metadata()).unwrap()
    }
}

