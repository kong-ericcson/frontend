use crate::metadata::{StateMachineMeta, StateMachineModel};
use crate::simulation::{SimulationConfig, Simulatable};

/// Entry point for model binaries.
///
/// Handles CLI flags for metadata emission and simulation.
/// The `state_factory` async closure creates a fully-wired state machine
/// instance (with DB connections) for simulation.
pub async fn model_main<S, F, Fut>(
    metadata_fn: fn() -> StateMachineMeta,
    state_factory: F,
)
where
    S: StateMachineModel + Simulatable,
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = S>,
{
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--emit-model") {
        let meta = metadata_fn();
        println!("{}", serde_json::to_string_pretty(&meta).unwrap());
        return;
    }

    if args.iter().any(|a| a == "--simulate") {
        let config: SimulationConfig = serde_json::from_reader(std::io::stdin()).unwrap();
        let mut state = state_factory().await;
        let result = crate::simulation::simulate(&mut state, &config).await;
        println!("{}", serde_json::to_string(&result).unwrap());
        return;
    }

    eprintln!("Usage: <model-binary> --emit-model | --simulate");
    std::process::exit(1);
}
