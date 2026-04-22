use bluetext_model::prelude::*;

// ── Simple workflow using #[workflow_impl] ──────────────────────────

pub struct Transfer;

#[workflow_impl]
impl Transfer {
    #[step]
    async fn validate(&self, _from: &str, _to: &str) -> Result<(), WorkflowError> {
        Ok(())
    }

    #[request(ledger)]
    async fn debit(&self, _from: &str, _amount: &u64) -> Result<(), WorkflowError> {
        Ok(())
    }

    #[request(ledger)]
    async fn credit(&self, _to: &str, _amount: &u64) -> Result<(), WorkflowError> {
        Ok(())
    }

    #[condition(name = "insufficient")]
    fn insufficient_funds(&self, _from: &str) -> bool {
        false
    }

    #[step]
    async fn rollback(&self, _from: &str, _to: &str, _amount: &u64) -> Result<(), WorkflowError> {
        Ok(())
    }

    #[step]
    async fn confirm(&self, _from: &str, _to: &str) -> Result<(), WorkflowError> {
        Ok(())
    }

    #[step]
    async fn send_receipt(&self, _to: &str) -> Result<(), WorkflowError> {
        Ok(())
    }

    #[request(ledger)]
    async fn finalize(&self, _from: &str) -> Result<(), WorkflowError> {
        Ok(())
    }

    #[step]
    async fn notify_admin(&self, _from: &str) -> Result<(), WorkflowError> {
        Ok(())
    }

    #[flow]
    async fn run(&self, from: String, to: String, amount: u64) -> Result<(), WorkflowError> {
        self.validate(&from, &to).await?;
        self.debit(&from, &amount).await?;
        self.credit(&to, &amount).await?;

        if self.insufficient_funds(&from) {
            self.rollback(&from, &to, &amount).await?;
        } else {
            self.confirm(&from, &to).await?;
        }

        tokio::try_join!(
            self.send_receipt(&from),
            self.send_receipt(&to),
        )?;

        match async {
            self.finalize(&from).await?;
            Ok::<(), WorkflowError>(())
        }.await {
            Ok(()) => {},
            Err(_) => {
                self.notify_admin(&from).await?;
            }
        }

        Ok(())
    }

    #[constraint]
    fn ordering() {
        before("validate", "ledger.debit");
        exclusive("rollback", "confirm");
        eventually("ledger.debit");
        max_occurrences("notify_admin", 1);
    }
}

#[test]
fn test_workflow_impl_metadata() {
    let meta = Transfer::workflow_metadata();
    assert_eq!(meta.workflows.len(), 1);
    assert_eq!(meta.workflows[0].name, "transfer");
    assert_eq!(meta.workflows[0].params.len(), 3);
    assert_eq!(meta.workflows[0].params[0].name, "from");
    assert_eq!(meta.workflows[0].params[1].name, "to");
    assert_eq!(meta.workflows[0].params[2].name, "amount");
}

#[test]
fn test_workflow_impl_name() {
    assert_eq!(Transfer::name(), "transfer");
}

#[test]
fn test_workflow_impl_step_kinds() {
    let meta = Transfer::workflow_metadata();
    let body = &meta.workflows[0].body;
    let json = serde_json::to_value(body).unwrap();
    let steps = json.as_array().unwrap();

    // do validate
    assert_eq!(steps[0]["kind"], "do");
    assert_eq!(steps[0]["action"], "validate");

    // request ledger.debit
    assert_eq!(steps[1]["kind"], "request");
    assert_eq!(steps[1]["service"], "ledger");
    assert_eq!(steps[1]["method"], "debit");

    // request ledger.credit
    assert_eq!(steps[2]["kind"], "request");
    assert_eq!(steps[2]["service"], "ledger");
    assert_eq!(steps[2]["method"], "credit");

    // if insufficient_funds
    assert_eq!(steps[3]["kind"], "if");
    assert_eq!(steps[3]["name"], "insufficient");
    assert_eq!(steps[3]["condition"], "insufficient_funds(from)");

    // Check then/else branches
    let then_steps = steps[3]["then"].as_array().unwrap();
    assert_eq!(then_steps.len(), 1);
    assert_eq!(then_steps[0]["kind"], "do");
    assert_eq!(then_steps[0]["action"], "rollback");

    let else_steps = steps[3]["else"].as_array().unwrap();
    assert_eq!(else_steps.len(), 1);
    assert_eq!(else_steps[0]["kind"], "do");
    assert_eq!(else_steps[0]["action"], "confirm");

    // parallel
    assert_eq!(steps[4]["kind"], "parallel");
    let par_steps = steps[4]["steps"].as_array().unwrap();
    assert_eq!(par_steps.len(), 2);
    assert_eq!(par_steps[0]["kind"], "do");
    assert_eq!(par_steps[0]["action"], "send_receipt");

    // try
    assert_eq!(steps[5]["kind"], "try");
    let try_body = steps[5]["body"].as_array().unwrap();
    assert_eq!(try_body.len(), 1);
    assert_eq!(try_body[0]["kind"], "request");
    assert_eq!(try_body[0]["method"], "finalize");

    let on_failure = steps[5]["onFailure"].as_array().unwrap();
    assert_eq!(on_failure.len(), 1);
    assert_eq!(on_failure[0]["kind"], "do");
    assert_eq!(on_failure[0]["action"], "notify_admin");
}

#[test]
fn test_workflow_impl_constraints() {
    let meta = Transfer::workflow_metadata();
    assert_eq!(meta.constraints.len(), 1);

    let constraints = &meta.constraints[0].constraints;
    assert_eq!(constraints.len(), 4);

    let json = serde_json::to_value(constraints).unwrap();
    let arr = json.as_array().unwrap();

    assert_eq!(arr[0]["kind"], "before");
    assert_eq!(arr[0]["stepA"], "validate");
    assert_eq!(arr[0]["stepB"], "ledger.debit");

    assert_eq!(arr[1]["kind"], "exclusive");
    assert_eq!(arr[1]["stepA"], "rollback");
    assert_eq!(arr[1]["stepB"], "confirm");

    assert_eq!(arr[2]["kind"], "eventually");
    assert_eq!(arr[2]["step"], "ledger.debit");

    assert_eq!(arr[3]["kind"], "max_occurrences");
    assert_eq!(arr[3]["step"], "notify_admin");
    assert_eq!(arr[3]["count"], 1);
}

#[test]
fn test_workflow_impl_json_format() {
    let json = Transfer::workflow_metadata_json();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

    // Verify top-level structure matches WorkflowFile format
    assert!(parsed["imports"].is_array());
    assert!(parsed["workflows"].is_array());
    assert!(parsed["constraints"].is_array());
    assert_eq!(parsed["workflows"][0]["name"], "transfer");
}

// ── Test with explicit name override ────────────────────────────────

pub struct MyAccountSetup;

#[workflow_impl(name = "account_setup")]
impl MyAccountSetup {
    #[step]
    async fn create_account(&self, _id: &str) -> Result<(), WorkflowError> {
        Ok(())
    }

    #[flow]
    async fn run(&self, customer_id: String) -> Result<(), WorkflowError> {
        self.create_account(&customer_id).await?;
        Ok(())
    }
}

#[test]
fn test_workflow_impl_name_override() {
    assert_eq!(MyAccountSetup::name(), "account_setup");
    let meta = MyAccountSetup::workflow_metadata();
    assert_eq!(meta.workflows[0].name, "account_setup");
}

// ── Test with client fields (self.client.method pattern) ────────────

#[derive(Default)]
pub struct Ledger;
impl Ledger {
    pub async fn debit(&self, _from: &str, _amount: &u64) -> Result<(), WorkflowError> { Ok(()) }
    pub async fn credit(&self, _to: &str, _amount: &u64) -> Result<(), WorkflowError> { Ok(()) }
}

#[derive(Default)]
pub struct ClientTransfer {
    ledger: Ledger,
}

#[workflow_impl(name = "client_transfer")]
impl ClientTransfer {
    #[step]
    async fn validate(&self, _from: &str, _to: &str) -> Result<(), WorkflowError> {
        Ok(())
    }

    #[flow]
    async fn run(&self, from: String, to: String, amount: u64) -> Result<(), WorkflowError> {
        self.validate(&from, &to).await?;
        self.ledger.debit(&from, &amount).await?;
        self.ledger.credit(&to, &amount).await?;
        Ok(())
    }
}

#[test]
fn test_workflow_impl_client_calls() {
    let meta = ClientTransfer::workflow_metadata();
    let body = &meta.workflows[0].body;
    let json = serde_json::to_value(body).unwrap();
    let steps = json.as_array().unwrap();

    assert_eq!(steps.len(), 3);

    assert_eq!(steps[0]["kind"], "do");
    assert_eq!(steps[0]["action"], "validate");

    assert_eq!(steps[1]["kind"], "request");
    assert_eq!(steps[1]["service"], "ledger");
    assert_eq!(steps[1]["method"], "debit");
    assert_eq!(steps[1]["args"], serde_json::json!(["from", "amount"]));

    assert_eq!(steps[2]["kind"], "request");
    assert_eq!(steps[2]["service"], "ledger");
    assert_eq!(steps[2]["method"], "credit");
    assert_eq!(steps[2]["args"], serde_json::json!(["to", "amount"]));
}
