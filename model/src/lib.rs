use bluetext_model::prelude::*;

#[derive(Clone, Debug, Serialize, Deserialize, StateMachine, Default)]
#[state_machine(name = "demo")]
pub struct AppState {}

#[state_machine_impl]
impl AppState {
    #[simulation_init]
    pub fn init(&mut self) {}

    #[simulation_step]
    pub fn step(&mut self) -> bool { false }
}

bluetext_model::model! {
    state_machine: AppState,
    source_dir: "model/src",
    modules: [],
    types: [],
}
