use syn::{Expr, ExprField, ExprMethodCall, ExprPath, Member, Stmt};

/// Walk a method body and detect which `self.<field>` paths are modified or read.
pub fn analyze_field_access(stmts: &[Stmt]) -> (Vec<String>, Vec<String>) {
    let mut modifies = Vec::new();
    let mut reads = Vec::new();

    for stmt in stmts {
        collect_from_stmt(stmt, &mut modifies, &mut reads);
    }

    modifies.sort();
    modifies.dedup();
    reads.sort();
    reads.dedup();
    (modifies, reads)
}

fn collect_from_stmt(stmt: &Stmt, modifies: &mut Vec<String>, reads: &mut Vec<String>) {
    match stmt {
        Stmt::Local(local) => {
            if let Some(init) = &local.init {
                collect_from_expr(&init.expr, modifies, reads, false);
            }
        }
        Stmt::Expr(expr, _) => {
            collect_from_expr(expr, modifies, reads, false);
        }
        _ => {}
    }
}

fn collect_from_expr(
    expr: &Expr,
    modifies: &mut Vec<String>,
    reads: &mut Vec<String>,
    is_lhs: bool,
) {
    match expr {
        // Assignment: self.field = ...
        Expr::Assign(assign) => {
            collect_from_expr(&assign.left, modifies, reads, true);
            collect_from_expr(&assign.right, modifies, reads, false);
        }

        // Field access: self.field
        Expr::Field(ExprField { base, member, .. }) => {
            if let Some(field_name) = extract_self_field(base, member) {
                if is_lhs {
                    modifies.push(field_name);
                } else {
                    reads.push(field_name);
                }
            } else {
                // Nested: self.field.something — extract the root
                if let Some(root_field) = extract_self_root_field(expr) {
                    if is_lhs {
                        modifies.push(root_field);
                    } else {
                        reads.push(root_field);
                    }
                }
                collect_from_expr(base, modifies, reads, is_lhs);
            }
        }

        // Method call: self.field.insert(...), self.field.push(...)
        Expr::MethodCall(ExprMethodCall {
            receiver, method, args, ..
        }) => {
            let method_name = method.to_string();
            let is_mutating = matches!(
                method_name.as_str(),
                "insert"
                    | "upsert"
                    | "push"
                    | "pop"
                    | "remove"
                    | "get_mut"
                    | "entry"
                    | "extend"
                    | "clear"
                    | "retain"
                    | "sort"
                    | "sort_by"
                    | "truncate"
                    | "drain"
                    | "append"
            );

            if let Some(root_field) = extract_self_root_field(receiver) {
                if is_mutating || is_lhs {
                    modifies.push(root_field);
                } else {
                    reads.push(root_field.clone());
                }
            }

            collect_from_expr(receiver, modifies, reads, false);
            for arg in args {
                collect_from_expr(arg, modifies, reads, false);
            }
        }

        // Binary op: a + b, a < b, etc.
        Expr::Binary(binary) => {
            collect_from_expr(&binary.left, modifies, reads, false);
            collect_from_expr(&binary.right, modifies, reads, false);
        }

        // Unary: !expr, -expr
        Expr::Unary(unary) => {
            collect_from_expr(&unary.expr, modifies, reads, false);
        }

        // Block: { stmts }
        Expr::Block(block) => {
            for stmt in &block.block.stmts {
                collect_from_stmt(stmt, modifies, reads);
            }
        }

        // If: if cond { ... } else { ... }
        Expr::If(expr_if) => {
            collect_from_expr(&expr_if.cond, modifies, reads, false);
            for stmt in &expr_if.then_branch.stmts {
                collect_from_stmt(stmt, modifies, reads);
            }
            if let Some((_, else_expr)) = &expr_if.else_branch {
                collect_from_expr(else_expr, modifies, reads, false);
            }
        }

        // Match
        Expr::Match(expr_match) => {
            collect_from_expr(&expr_match.expr, modifies, reads, false);
            for arm in &expr_match.arms {
                collect_from_expr(&arm.body, modifies, reads, false);
            }
        }

        // Call: func(args)
        Expr::Call(call) => {
            collect_from_expr(&call.func, modifies, reads, false);
            for arg in &call.args {
                collect_from_expr(arg, modifies, reads, false);
            }
        }

        // Let: let x = ...
        Expr::Let(expr_let) => {
            collect_from_expr(&expr_let.expr, modifies, reads, false);
        }

        // Return
        Expr::Return(ret) => {
            if let Some(expr) = &ret.expr {
                collect_from_expr(expr, modifies, reads, false);
            }
        }

        // Reference: &expr, &mut expr
        Expr::Reference(reference) => {
            collect_from_expr(&reference.expr, modifies, reads, reference.mutability.is_some());
        }

        // Index: self.field[i]
        Expr::Index(index) => {
            if let Some(root) = extract_self_root_field(&index.expr) {
                if is_lhs {
                    modifies.push(root);
                } else {
                    reads.push(root);
                }
            }
            collect_from_expr(&index.expr, modifies, reads, is_lhs);
            collect_from_expr(&index.index, modifies, reads, false);
        }

        // Paren: (expr)
        Expr::Paren(paren) => {
            collect_from_expr(&paren.expr, modifies, reads, is_lhs);
        }

        // Closure
        Expr::Closure(closure) => {
            collect_from_expr(&closure.body, modifies, reads, false);
        }

        // For loop
        Expr::ForLoop(for_loop) => {
            collect_from_expr(&for_loop.expr, modifies, reads, false);
            for stmt in &for_loop.body.stmts {
                collect_from_stmt(stmt, modifies, reads);
            }
        }

        // While loop
        Expr::While(while_loop) => {
            collect_from_expr(&while_loop.cond, modifies, reads, false);
            for stmt in &while_loop.body.stmts {
                collect_from_stmt(stmt, modifies, reads);
            }
        }

        // Loop
        Expr::Loop(expr_loop) => {
            for stmt in &expr_loop.body.stmts {
                collect_from_stmt(stmt, modifies, reads);
            }
        }

        // Tuple: (a, b)
        Expr::Tuple(tuple) => {
            for elem in &tuple.elems {
                collect_from_expr(elem, modifies, reads, false);
            }
        }

        // Try: expr?
        Expr::Try(expr_try) => {
            collect_from_expr(&expr_try.expr, modifies, reads, false);
        }

        // Await: expr.await
        Expr::Await(expr_await) => {
            collect_from_expr(&expr_await.base, modifies, reads, is_lhs);
        }

        _ => {}
    }
}

/// Check if an expression is `self.field` and return the field name.
fn extract_self_field(base: &Expr, member: &Member) -> Option<String> {
    if let Expr::Path(ExprPath { path, .. }) = base {
        if path.is_ident("self") {
            if let Member::Named(ident) = member {
                return Some(ident.to_string());
            }
        }
    }
    None
}

/// Walk up from an expression to find the root `self.field` name.
/// Handles chains like `self.accounts.get_mut(&from).unwrap().balance`.
fn extract_self_root_field(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Field(ExprField { base, member, .. }) => {
            if let Some(name) = extract_self_field(base, member) {
                Some(name)
            } else {
                extract_self_root_field(base)
            }
        }
        Expr::MethodCall(ExprMethodCall { receiver, .. }) => extract_self_root_field(receiver),
        Expr::Index(index) => extract_self_root_field(&index.expr),
        Expr::Reference(reference) => extract_self_root_field(&reference.expr),
        Expr::Paren(paren) => extract_self_root_field(&paren.expr),
        Expr::Await(expr_await) => extract_self_root_field(&expr_await.base),
        _ => None,
    }
}
