use syn::{
    parse::{Parse, ParseStream},
    Expr, ExprAwait, ExprIf, ExprMacro, ExprMatch,
    ExprMethodCall, Pat, Stmt, Token,
    punctuated::Punctuated,
    spanned::Spanned,
};
use quote::quote;

// ── IR types ────────────────────────────────────────────────────────

#[derive(Clone)]
pub(crate) struct Step {
    pub kind: StepKind,
    pub line: usize,
    pub end_line: usize,
}

#[derive(Clone)]
pub(crate) enum StepKind {
    Do { action: String, args: Vec<String> },
    Parallel { steps: Vec<Step> },
    Sleep { duration: String, steps: Vec<Step> },
    If { name: Option<String>, condition: String, condition_args: Vec<String>, then_steps: Vec<Step>, else_steps: Option<Vec<Step>> },
    Try { body: Vec<Step>, on_failure: Vec<Step> },
}


impl Step {
    pub fn new(kind: StepKind, line: usize, end_line: usize) -> Self {
        Self { kind, line, end_line }
    }
}

impl From<StepKind> for Step {
    fn from(kind: StepKind) -> Self {
        Self { kind, line: 0, end_line: 0 }
    }
}


// ── JSON serialization ───────────────────────────────────────────────

pub(crate) fn step_to_json(step: &Step) -> serde_json::Value {
    let line = step.line;
    let end_line = step.end_line;
    match &step.kind {
        StepKind::Do { action, args } => serde_json::json!({
            "kind": "do",
            "action": action,
            "args": args,
            "line": line, "endLine": end_line,
        }),
        StepKind::Parallel { steps } => serde_json::json!({
            "kind": "parallel",
            "steps": steps.iter().map(step_to_json).collect::<Vec<_>>(),
            "line": line, "endLine": end_line,
        }),
        StepKind::Sleep { duration, steps } => {
            let children: Vec<serde_json::Value> = steps.iter().map(step_to_json).collect();
            serde_json::json!({
                "kind": "sleep",
                "duration": duration,
                "steps": children,
                "line": line, "endLine": end_line,
            })
        }
        StepKind::If { name, condition, condition_args, then_steps, else_steps } => {
            let cond_str = if condition_args.is_empty() {
                condition.clone()
            } else {
                format!("{}({})", condition, condition_args.join(", "))
            };
            let mut v = serde_json::json!({
                "kind": "if",
                "condition": cond_str,
                "then": then_steps.iter().map(step_to_json).collect::<Vec<_>>(),
                "line": line, "endLine": end_line,
            });
            if let Some(n) = name {
                v["name"] = serde_json::Value::String(n.clone());
            }
            if let Some(els) = else_steps {
                v["else"] = serde_json::Value::Array(els.iter().map(step_to_json).collect());
            }
            v
        }
        StepKind::Try { body, on_failure } => serde_json::json!({
            "kind": "try",
            "body": body.iter().map(step_to_json).collect::<Vec<_>>(),
            "onFailure": on_failure.iter().map(step_to_json).collect::<Vec<_>>(),
            "onFailureLine": 0,
            "line": line, "endLine": end_line,
        }),
    }
}

// ── Flow body walker (for #[commands]) ──────────────────────────────
//
// Walks a Rust method body AST and extracts control flow steps.
// Every `self.xxx()` call is treated as a `Do` step.
// Recognises if/else, match (try pattern), tokio::join!/try_join! (parallel),
// and .await expressions.

pub(crate) fn walk_stmts(stmts: &[Stmt]) -> Vec<Step> {
    let mut steps = Vec::new();
    let mut pending_label: Option<String> = None;
    for stmt in stmts {
        // Check for label!("...") macro call — applies to the next step
        if let Some(lbl) = extract_label_macro(stmt) {
            pending_label = Some(lbl);
            continue;
        }
        let mut new_steps = walk_stmt(stmt);
        // Apply pending label to the first If step found
        if pending_label.is_some() {
            if let Some(step) = new_steps.iter_mut().find(|s| matches!(s.kind, StepKind::If { .. })) {
                if let StepKind::If { name, .. } = &mut step.kind {
                    *name = pending_label.take();
                }
            }
        }
        steps.extend(new_steps);
    }
    steps
}

/// Extract label from `label!("text")` which expands to `let _ = "text";`.
fn extract_label_macro(stmt: &Stmt) -> Option<String> {
    // Detect expanded label!(): `let _ = "text";`
    if let Stmt::Local(local) = stmt {
        if let Pat::Wild(_) = &local.pat {
            if let Some(init) = &local.init {
                if let Expr::Lit(lit) = &*init.expr {
                    if let syn::Lit::Str(s) = &lit.lit {
                        return Some(s.value());
                    }
                }
            }
        }
    }
    // Detect unexpanded label!() macro call
    let mac = match stmt {
        Stmt::Expr(Expr::Macro(mac), _) => Some(&mac.mac),
        Stmt::Macro(stmt_mac) => Some(&stmt_mac.mac),
        _ => None,
    };
    if let Some(mac) = mac {
        if mac.path.is_ident("label") {
            let tokens = mac.tokens.to_string();
            let trimmed = tokens.trim().trim_matches('"');
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// Extract a self.xxx() call from an expression (for if-let patterns).
fn extract_self_call_from_expr(expr: &Expr) -> Option<Step> {
    match expr {
        Expr::MethodCall(mc) => {
            if is_self_receiver(&mc.receiver) {
                let method = mc.method.to_string();
                let args = extract_call_args(&mc.args);
                Some(step_at(StepKind::Do { action: method, args }, mc.method.span()))
            } else if let Expr::MethodCall(inner) = &*mc.receiver {
                extract_self_call_from_expr(&Expr::MethodCall(inner.clone()))
            } else {
                None
            }
        }
        Expr::Await(aw) => extract_self_call_from_expr(&aw.base),
        Expr::Try(tr) => extract_self_call_from_expr(&tr.expr),
        _ => None,
    }
}

fn step_at(kind: StepKind, span: proc_macro2::Span) -> Step {
    Step::new(kind, span.start().line, span.end().line)
}

fn walk_stmt(stmt: &Stmt) -> Vec<Step> {
    match stmt {
        Stmt::Expr(expr, _) => {
            if let Expr::Return(ret) = expr {
                let span = expr.span();
                let action = if is_err_return(ret) { "return_err" } else { "return_ok" };
                vec![Step::new(StepKind::Do { action: action.to_string(), args: vec![] }, span.start().line, span.end().line)]
            } else if let Expr::If(if_expr) = expr {
                // For `if let Some(x) = self.xxx(...)`, extract the self call as a Do step
                let mut steps = Vec::new();
                if let Expr::Let(let_expr) = &*if_expr.cond {
                    if let Some(do_step) = extract_self_call_from_expr(&let_expr.expr) {
                        steps.push(do_step);
                    }
                }
                steps.extend(walk_expr(expr));
                steps
            } else {
                walk_expr_multi(expr)
            }
        }
        Stmt::Local(local) => {
            if let Some(init) = &local.init {
                walk_expr_multi(&init.expr)
            } else {
                vec![]
            }
        }
        _ => vec![],
    }
}

fn walk_expr_multi(expr: &Expr) -> Vec<Step> {
    match expr {
        Expr::Block(block) => walk_stmts(&block.block.stmts),
        _ => walk_expr(expr).into_iter().collect(),
    }
}

fn walk_expr(expr: &Expr) -> Option<Step> {
    let span = expr.span();
    let mut step = match expr {
        Expr::Try(e) => walk_expr(&e.expr),
        Expr::Await(e) => walk_await(e),
        Expr::Macro(e) => walk_macro(e),
        Expr::If(e) => walk_if(e),
        Expr::Match(e) => walk_match(e),
        Expr::Paren(e) => walk_expr(&e.expr),
        Expr::MethodCall(mc) => walk_method_call(mc),
        _ => None,
    }?;
    if step.line == 0 {
        step.line = span.start().line;
        step.end_line = span.end().line;
    }
    Some(step)
}

/// Handle `self.method(args)` calls, including chained calls like `self.get_event(&id).map(...)`.
fn walk_method_call(mc: &ExprMethodCall) -> Option<Step> {
    if is_self_receiver(&mc.receiver) {
        let method = mc.method.to_string();
        let args = extract_call_args(&mc.args);
        Some(step_at(StepKind::Do { action: method, args }, mc.method.span()))
    } else if let Expr::MethodCall(inner) = &*mc.receiver {
        // Chained: self.get_event(&id).map(...) — extract the self.xxx() call
        walk_method_call(inner)
    } else {
        None
    }
}

fn walk_await(await_expr: &ExprAwait) -> Option<Step> {
    match &*await_expr.base {
        // self.method(args).await
        Expr::MethodCall(mc) => {
            if is_self_receiver(&mc.receiver) {
                let method = mc.method.to_string();
                let args = extract_call_args(&mc.args);
                Some(StepKind::Do { action: method, args }.into())
            } else {
                None
            }
        }
        // Plain function call: fn(args).await
        Expr::Call(call) => {
            if let Expr::Path(path) = &*call.func {
                let segments: Vec<_> = path.path.segments.iter().collect();
                if segments.len() == 1 {
                    let name = segments[0].ident.to_string();
                    if name == "sleep" {
                        let duration = extract_string_arg_from_call(&call.args).unwrap_or_default();
                        return Some(StepKind::Sleep { duration, steps: vec![] }.into());
                    }
                }
            }
            None
        }
        _ => None,
    }
}

fn walk_macro(mac: &ExprMacro) -> Option<Step> {
    let path_str = mac.mac.path.segments.iter()
        .map(|s| s.ident.to_string())
        .collect::<Vec<_>>()
        .join("::");

    let is_join = matches!(
        path_str.as_str(),
        "tokio::join" | "tokio::try_join" | "join" | "try_join"
        | "bluetext_model::tokio::join" | "bluetext_model::tokio::try_join"
    );
    if !is_join {
        return None;
    }

    let exprs: Vec<Expr> = syn::parse2::<CommaSepExprs>(mac.mac.tokens.clone())
        .map(|c| c.0)
        .unwrap_or_default();

    let steps: Vec<Step> = exprs
        .iter()
        .filter_map(|expr| walk_parallel_branch(expr))
        .collect();

    if steps.is_empty() { return None; }
    Some(StepKind::Parallel { steps }.into())
}

fn walk_parallel_branch(expr: &Expr) -> Option<Step> {
    match expr {
        Expr::MethodCall(mc) => {
            if is_self_receiver(&mc.receiver) {
                let method = mc.method.to_string();
                let args = extract_call_args(&mc.args);
                Some(StepKind::Do { action: method, args }.into())
            } else {
                None
            }
        }
        Expr::Try(e) => walk_parallel_branch(&e.expr),
        Expr::Await(e) => walk_parallel_branch(&e.base),
        _ => None,
    }
}

/// Render the full condition expression as a string for the diagram label.
/// Strips `self.` prefix for readability.
fn extract_condition(expr: &Expr) -> (Option<String>, String, Vec<String>) {
    let raw = quote::quote!(#expr).to_string();
    // Clean up for readability: strip "self.", normalize spacing
    let condition = raw
        .replace("self . ", "")
        .replace("self.", "")
        .replace("& ", "&")
        .replace(" . ", ".");
    (None, condition, vec![])
}

/// Check if a block contains a return or early exit.
fn has_return(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| {
        if let Stmt::Expr(expr, _) = s {
            matches!(expr, Expr::Return(_))
        } else {
            false
        }
    })
}

fn walk_if(if_expr: &ExprIf) -> Option<Step> {
    let (name, condition_str, args) = extract_condition(&if_expr.cond);

    let then_steps = walk_stmts(&if_expr.then_branch.stmts);
    let then_has_return = has_return(&if_expr.then_branch.stmts);
    let else_steps = if_expr.else_branch.as_ref().and_then(|(_, else_expr)| {
        match &**else_expr {
            Expr::Block(block) => {
                let steps = walk_stmts(&block.block.stmts);
                let has_ret = has_return(&block.block.stmts);
                if steps.is_empty() && !has_ret { None } else { Some(steps) }
            }
            Expr::If(nested) => walk_if(nested).map(|s| vec![s]),
            _ => None,
        }
    });

    // Emit the If step if there are steps, returns, or a meaningful condition with self calls
    if then_steps.is_empty() && !then_has_return && else_steps.is_none() {
        return None;
    }

    Some(StepKind::If {
        name,
        condition: condition_str,
        condition_args: args,
        then_steps,
        else_steps,
    }.into())
}

fn walk_match(match_expr: &ExprMatch) -> Option<Step> {
    // match async { ... }.await — try/catch pattern
    let async_block = match &*match_expr.expr {
        Expr::Await(await_expr) => match &*await_expr.base {
            Expr::Async(async_expr) => Some(&async_expr.block),
            _ => None,
        },
        _ => None,
    }?;

    let body_steps = walk_stmts(&async_block.stmts);

    let mut on_failure_steps = Vec::new();
    for arm in &match_expr.arms {
        if is_err_pattern(&arm.pat) {
            match &*arm.body {
                Expr::Block(block) => {
                    on_failure_steps = walk_stmts(&block.block.stmts);
                }
                _ => {
                    on_failure_steps.extend(walk_expr(&arm.body));
                }
            }
        }
    }

    Some(StepKind::Try { body: body_steps, on_failure: on_failure_steps }.into())
}

// ── Expression helpers ──────────────────────────────────────────────

/// Check if a return expression returns Err(...).
fn is_err_return(ret: &syn::ExprReturn) -> bool {
    if let Some(expr) = &ret.expr {
        if let Expr::Call(call) = &**expr {
            if let Expr::Path(path) = &*call.func {
                if let Some(ident) = path.path.get_ident() {
                    return ident == "Err";
                }
            }
        }
    }
    false
}

fn is_self_receiver(expr: &Expr) -> bool {
    if let Expr::Path(path) = expr {
        return path.path.is_ident("self");
    }
    false
}

fn extract_call_args(args: &Punctuated<Expr, Token![,]>) -> Vec<String> {
    args.iter().map(extract_arg_name).collect()
}

fn extract_arg_name(expr: &Expr) -> String {
    match expr {
        Expr::Reference(r) => extract_arg_name(&r.expr),
        Expr::Path(p) => p
            .path
            .segments
            .last()
            .map(|s| s.ident.to_string())
            .unwrap_or_else(|| quote!(#expr).to_string()),
        Expr::Field(f) => {
            let base = extract_arg_name(&f.base);
            match &f.member {
                syn::Member::Named(ident) => format!("{}.{}", base, ident),
                syn::Member::Unnamed(index) => format!("{}.{}", base, index.index),
            }
        }
        _ => quote!(#expr).to_string(),
    }
}

fn extract_string_arg_from_call(args: &Punctuated<Expr, Token![,]>) -> Option<String> {
    args.first().and_then(extract_str_lit)
}

fn extract_str_lit(expr: &Expr) -> Option<String> {
    if let Expr::Lit(lit) = expr {
        if let syn::Lit::Str(s) = &lit.lit {
            return Some(s.value());
        }
    }
    None
}

fn is_err_pattern(pat: &Pat) -> bool {
    match pat {
        Pat::TupleStruct(pts) => pts
            .path
            .segments
            .last()
            .map(|s| s.ident == "Err")
            .unwrap_or(false),
        _ => false,
    }
}

struct CommaSepExprs(Vec<Expr>);

impl Parse for CommaSepExprs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let p = Punctuated::<Expr, Token![,]>::parse_terminated(input)?;
        Ok(CommaSepExprs(p.into_iter().collect()))
    }
}

