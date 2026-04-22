extern crate proc_macro;

mod state_machine;
mod commands;
mod analysis;
mod metadata;
mod model_macro;
pub(crate) mod flow_walker;

use proc_macro::TokenStream;

/// Derive macro for model types (structs/enums used in state machines).
/// Generates metadata for UI diagram rendering.
#[proc_macro_derive(ModelType, attributes(references))]
pub fn derive_model_type(input: TokenStream) -> TokenStream {
    state_machine::derive_model_type_impl(input)
}

/// Attribute macro for type aliases (e.g., `type AccountId = String`).
/// Captures the alias mapping for metadata.
#[proc_macro_attribute]
pub fn model_alias(_attr: TokenStream, input: TokenStream) -> TokenStream {
    state_machine::model_alias_impl(input)
}

/// Derive macro for the root state machine struct.
/// Fields become state variables in the model metadata.
#[proc_macro_derive(StateMachine, attributes(state_machine))]
pub fn derive_state_machine(input: TokenStream) -> TokenStream {
    state_machine::derive_state_machine_impl(input)
}

/// Attribute macro for impl blocks on a StateMachine.
/// Parses #[mutation], #[getter], #[simulation_init], #[simulation_step], #[state_invariant] on methods.
/// Accepts optional extra_state_invariants, extra_meta for cross-module dispatch.
#[proc_macro_attribute]
pub fn state_machine_impl(attr: TokenStream, input: TokenStream) -> TokenStream {
    state_machine::state_machine_impl_impl(attr, input)
}

/// Attribute macro for per-module action/state invariant impl blocks.
/// Generates method implementations and a metadata registration function.
/// Does NOT generate trait impls (use state_machine_impl for that).
#[proc_macro_attribute]
pub fn state_machine_actions(_attr: TokenStream, input: TokenStream) -> TokenStream {
    state_machine::state_machine_actions_impl(input)
}

/// Attribute macro for command impl blocks on a StateMachine.
/// Commands orchestrate mutations, getters, and requests.
/// Guarantees eventual consistency via durable execution (Resonate).
///
/// Methods annotated with `#[command]` become public data model operations.
/// Methods annotated with `#[command(private)]` are internal to the data model.
///
/// ```ignore
/// #[commands]
/// impl BankState {
///     #[command]
///     pub async fn process_transfer(&mut self, from: String, to: String, amount: i64) -> Result<(), CommandError> {
///         let ok = self.transfer(from, to, amount);
///         if !ok { return Err(CommandError::from("insufficient funds")); }
///         self.send_transfer_notification(&from, &to, amount).await?;
///         Ok(())
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn commands(_attr: TokenStream, input: TokenStream) -> TokenStream {
    commands::commands_impl(input)
}

/// Declarative macro for registering a complete model.
/// Generates metadata assembly, command collection, and a `main()` entry point.
///
/// ```ignore
/// bluetext_model::model! {
///     state_machine: bank::BankState,
///     source_dir: "code/model/src",
///     modules: [customer, account, loan],
///     types: [customer::Customer, account::Account],
///     commands: true,
/// }
/// ```
#[proc_macro]
pub fn model(input: TokenStream) -> TokenStream {
    model_macro::model_impl(input)
}
