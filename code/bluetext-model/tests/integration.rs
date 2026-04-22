use bluetext_model::prelude::*;

// ── State Machine Integration Test ──────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize, ModelType)]
struct Account {
    id: String,
    balance: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, StateMachine, Default)]
#[state_machine(name = "Bank")]
struct BankState {
    accounts: HashMap<String, Account>,
    total_supply: i64,
}

#[state_machine_impl]
impl BankState {
    #[simulation_init]
    pub fn init(&mut self) {
        self.accounts.insert(
            "alice".into(),
            Account { id: "alice".into(), balance: 100 },
        );
        self.accounts.insert(
            "bob".into(),
            Account { id: "bob".into(), balance: 100 },
        );
        self.total_supply = 200;
    }

    #[mutation]
    pub fn transfer(&mut self, from: String, to: String, amount: i64) -> bool {
        let sender = match self.accounts.get(&from) {
            Some(a) => a.balance,
            None => return false,
        };
        if sender < amount || amount <= 0 {
            return false;
        }
        self.accounts.get_mut(&from).unwrap().balance -= amount;
        self.accounts.get_mut(&to).unwrap().balance += amount;
        true
    }

    #[simulation_step]
    pub fn step(&mut self) -> bool {
        let ids: Vec<_> = self.accounts.keys().cloned().collect();
        if ids.len() < 2 {
            return false;
        }
        let from = bluetext_model_core::simulation::one_of(&ids).unwrap();
        let to = bluetext_model_core::simulation::one_of(&ids).unwrap();
        if from == to {
            return false;
        }
        let max = self.accounts[&from].balance;
        if max <= 0 {
            return false;
        }
        let amount = bluetext_model_core::simulation::rand_range(1, max);
        self.transfer(from, to, amount)
    }

    #[state_invariant]
    pub fn no_negative_balances(&self) -> bool {
        self.accounts.values().all(|a| a.balance >= 0)
    }

    #[state_invariant]
    pub fn total_supply_constant(&self) -> bool {
        let total: i64 = self.accounts.values().map(|a| a.balance).sum();
        total == self.total_supply
    }
}

#[test]
fn test_model_type_meta() {
    let meta = Account::type_meta();
    assert_eq!(meta.name, "Account");
    assert_eq!(meta.fields.len(), 2);
    assert_eq!(meta.fields[0].name, "id");
    assert_eq!(meta.fields[1].name, "balance");
}

#[test]
fn test_state_machine_metadata() {
    let meta = BankState::metadata();
    assert_eq!(meta.module, "Bank");
    assert_eq!(meta.state_vars.len(), 2);
    assert_eq!(meta.state_vars[0].name, "accounts");
    assert_eq!(meta.state_vars[1].name, "total_supply");

    // Mutations
    assert!(meta.mutations.iter().any(|m| m.name == "transfer"));
    assert_eq!(meta.mutations.len(), 1);

    // Transfer should modify accounts
    let transfer = meta.mutations.iter().find(|m| m.name == "transfer").unwrap();
    assert!(transfer.modifies.contains(&"accounts".to_string()));

    // Simulation
    assert_eq!(meta.simulation_inits.len(), 1);
    assert_eq!(meta.simulation_inits[0].name, "init");
    assert_eq!(meta.simulation_steps.len(), 1);
    assert_eq!(meta.simulation_steps[0].name, "step");

    // State invariants
    assert_eq!(meta.state_invariants.len(), 2);
    assert!(meta.state_invariants.iter().any(|i| i.name == "no_negative_balances"));
    assert!(meta.state_invariants.iter().any(|i| i.name == "total_supply_constant"));
}

#[test]
fn test_emit_model_json() {
    let json = BankState::emit_model_json();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["module"], "Bank");
    assert!(parsed["stateVars"].as_array().unwrap().len() >= 2);
    assert!(parsed["mutations"].as_array().unwrap().len() >= 1);
    assert!(parsed["stateInvariants"].as_array().unwrap().len() >= 2);
    assert!(parsed["simulationInits"].as_array().unwrap().len() >= 1);
    assert!(parsed["simulationSteps"].as_array().unwrap().len() >= 1);
}

#[tokio::test]
async fn test_simulation_ok() {
    let config = SimulationConfig {
        max_samples: 10,
        max_steps: 50,
        seed: Some(42),
        init_name: "init".to_string(),
        step_name: "step".to_string(),
        state_invariants: vec![
            "no_negative_balances".to_string(),
            "total_supply_constant".to_string(),
        ],
    };
    let mut state = BankState::default();
    let result = bluetext_model_core::simulation::simulate(&mut state, &config).await;
    assert_eq!(result.status, SimulationStatus::Ok);
    assert!(result.output.contains("[ok]"));
    assert!(result.seed.is_some());
}

#[test]
fn test_simulatable_trait() {
    assert_eq!(BankState::available_simulation_inits(), vec!["init"]);
    assert_eq!(BankState::available_simulation_steps(), vec!["step"]);
    assert_eq!(
        BankState::available_state_invariants(),
        vec!["no_negative_balances", "total_supply_constant"]
    );
}

#[test]
fn test_mutation_directly() {
    let mut state = BankState::default();
    state.init();
    assert_eq!(state.accounts["alice"].balance, 100);
    assert_eq!(state.accounts["bob"].balance, 100);

    let ok = state.transfer("alice".into(), "bob".into(), 30);
    assert!(ok);
    assert_eq!(state.accounts["alice"].balance, 70);
    assert_eq!(state.accounts["bob"].balance, 130);

    // Invariants still hold
    assert!(state.no_negative_balances());
    assert!(state.total_supply_constant());
}

#[test]
fn test_mutation_rejects_overdraft() {
    let mut state = BankState::default();
    state.init();
    let ok = state.transfer("alice".into(), "bob".into(), 200);
    assert!(!ok);
    assert_eq!(state.accounts["alice"].balance, 100); // unchanged
}

// ── Commands Integration Test ───────────────────────────────────────

#[commands]
impl BankState {
    /// Public command: process a transfer with validation.
    #[command]
    pub async fn process_transfer(
        &mut self,
        from: String,
        to: String,
        amount: i64,
    ) -> Result<bool, CommandError> {
        // Getter: check balance
        let sender_balance = self.accounts.get(&from)
            .map(|a| a.balance)
            .ok_or(CommandError::from("sender not found"))?;

        if sender_balance < amount {
            return Err(CommandError::from("insufficient funds"));
        }

        // Mutation: execute transfer
        let ok = self.transfer(from, to, amount);
        Ok(ok)
    }

    /// Private command: internal reconciliation.
    #[command(private)]
    async fn reconcile(&mut self) -> Result<(), CommandError> {
        let total: i64 = self.accounts.values().map(|a| a.balance).sum();
        if total != self.total_supply {
            return Err(CommandError::from("supply mismatch"));
        }
        Ok(())
    }
}

#[test]
fn test_commands_metadata() {
    let commands = BankState::__commands_meta();
    assert_eq!(commands.len(), 2);

    let process_transfer = commands.iter().find(|c| c.name == "process_transfer").unwrap();
    assert!(!process_transfer.is_private);
    assert_eq!(process_transfer.params.len(), 3); // from, to, amount
    assert!(process_transfer.calls.contains(&"transfer".to_string()));

    let reconcile = commands.iter().find(|c| c.name == "reconcile").unwrap();
    assert!(reconcile.is_private);
}

#[test]
fn test_public_command_names() {
    let names = BankState::__public_command_names();
    assert_eq!(names, vec!["process_transfer"]);
}

#[tokio::test]
async fn test_command_execution() {
    let mut state = BankState::default();
    state.init();

    // Successful transfer
    let result = state.process_transfer("alice".into(), "bob".into(), 30).await;
    assert!(result.is_ok());
    assert!(result.unwrap());
    assert_eq!(state.accounts["alice"].balance, 70);
    assert_eq!(state.accounts["bob"].balance, 130);

    // Insufficient funds
    let result = state.process_transfer("alice".into(), "bob".into(), 999).await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().message, "insufficient funds");
}

#[tokio::test]
async fn test_private_command_execution() {
    let mut state = BankState::default();
    state.init();

    // Reconcile should succeed when supply is correct
    let result = state.reconcile().await;
    assert!(result.is_ok());
}
