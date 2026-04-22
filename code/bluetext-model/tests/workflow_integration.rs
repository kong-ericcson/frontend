use bluetext_model::prelude::*;

// ── Workflow Integration Test ───────────────────────────────────────

// Define a simple workflow
workflow! {
    name: transfer,
    params: { from: String, to: String, amount: u64 },
    services: { ledger: dyn LedgerService },
    steps: {
        do validate(from, to);
        request ledger.debit(from, amount);
        request ledger.credit(to, amount);

        if insufficient_funds(from) {
            do rollback(from, to, amount);
        } else {
            do confirm(from, to);
        }

        parallel {
            do send_receipt(from);
            do send_receipt(to);
        }

        try {
            request ledger.finalize(from);
        } on failure {
            do notify_admin(from);
        }
    },
    constraints: {
        before(validate, ledger.debit);
        exclusive(rollback, confirm);
        eventually(ledger.debit);
        max_occurrences(notify_admin, 1);
    }
}

#[test]
fn test_workflow_metadata() {
    let meta = TransferWorkflow::workflow_metadata();
    assert_eq!(meta.workflows.len(), 1);
    assert_eq!(meta.workflows[0].name, "transfer");
    assert_eq!(meta.workflows[0].params.len(), 3);
    assert!(!meta.workflows[0].body.is_empty());

    // Check constraints
    assert_eq!(meta.constraints.len(), 1);
    assert_eq!(meta.constraints[0].constraints.len(), 4);
}

#[test]
fn test_workflow_name() {
    assert_eq!(TransferWorkflow::name(), "transfer");
}

#[test]
fn test_workflow_metadata_json() {
    let json = TransferWorkflow::workflow_metadata_json();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["workflows"][0]["name"], "transfer");
    assert!(parsed["constraints"][0]["constraints"].as_array().unwrap().len() >= 4);

    // Verify it matches the expected WorkflowFile format
    assert!(parsed["imports"].is_array());
    assert!(parsed["workflows"].is_array());
    assert!(parsed["constraints"].is_array());
}

#[test]
fn test_workflow_metadata_step_kinds() {
    let meta = TransferWorkflow::workflow_metadata();
    let body = &meta.workflows[0].body;

    // Check step kinds in order
    let json = serde_json::to_value(body).unwrap();
    let steps = json.as_array().unwrap();

    assert_eq!(steps[0]["kind"], "do");
    assert_eq!(steps[0]["action"], "validate");

    assert_eq!(steps[1]["kind"], "request");
    assert_eq!(steps[1]["service"], "ledger");
    assert_eq!(steps[1]["method"], "debit");

    assert_eq!(steps[2]["kind"], "request");
    assert_eq!(steps[2]["service"], "ledger");
    assert_eq!(steps[2]["method"], "credit");

    assert_eq!(steps[3]["kind"], "if");
    assert_eq!(steps[3]["condition"], "insufficient_funds(from)");

    assert_eq!(steps[4]["kind"], "parallel");
    assert!(steps[4]["steps"].as_array().unwrap().len() >= 2);

    assert_eq!(steps[5]["kind"], "try");
}

// Trait for the test service (not actually called in tests)
#[allow(dead_code)]
trait LedgerService: Send + Sync {
    fn debit(
        &self,
        from: &String,
        amount: &u64,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), WorkflowError>> + Send + '_>>;
    fn credit(
        &self,
        to: &String,
        amount: &u64,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), WorkflowError>> + Send + '_>>;
    fn finalize(
        &self,
        from: &String,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), WorkflowError>> + Send + '_>>;
}

// Stub async functions for the workflow steps
#[allow(dead_code)]
async fn validate(_from: &String, _to: &String) -> Result<(), WorkflowError> {
    Ok(())
}

#[allow(dead_code)]
async fn insufficient_funds(_from: &String) -> bool {
    false
}

#[allow(dead_code)]
async fn rollback(
    _from: &String,
    _to: &String,
    _amount: &u64,
) -> Result<(), WorkflowError> {
    Ok(())
}

#[allow(dead_code)]
async fn confirm(_from: &String, _to: &String) -> Result<(), WorkflowError> {
    Ok(())
}

#[allow(dead_code)]
async fn send_receipt(_to: &String) -> Result<(), WorkflowError> {
    Ok(())
}

#[allow(dead_code)]
async fn notify_admin(_from: &String) -> Result<(), WorkflowError> {
    Ok(())
}
