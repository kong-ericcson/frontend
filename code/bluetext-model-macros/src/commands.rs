use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    parse_macro_input, Attribute, ImplItem, ItemImpl, Meta,
};

use crate::state_machine::type_to_string;
use crate::flow_walker;

// ── #[commands] impl block ──────────────────────────────────────────

pub fn commands_impl(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as ItemImpl);
    let self_ty = &input.self_ty;

    let mut commands = Vec::new();

    for item in &input.items {
        if let ImplItem::Fn(method) = item {
            let kind = classify_command_method(&method.attrs);
            let method_name = method.sig.ident.to_string();

            let start_line = method.sig.ident.span().start().line;
            let end_line = method.block.brace_token.span.close().end().line;

            match kind {
                CommandKind::Command { is_private, constraints } => {
                    let params = extract_params(method);
                    let calls = extract_self_calls(&method.block.stmts);
                    let body = flow_walker::walk_stmts(&method.block.stmts);
                    commands.push(CommandInfo {
                        name: method_name,
                        params,
                        calls,
                        body,
                        is_private,
                        constraints,
                        start_line,
                        end_line,
                    });
                }
                CommandKind::None => {}
            }
        }
    }

    let command_meta_tokens = generate_command_meta_tokens(&commands);

    // Strip our attributes from the emitted code.
    let stripped_items: Vec<TokenStream2> = input.items.iter().filter_map(|item| {
        if let ImplItem::Fn(method) = item {
            let mut cleaned = method.clone();
            cleaned.attrs.retain(|attr| !is_command_attr(attr, "command"));
            Some(quote! { #cleaned })
        } else {
            Some(quote! { #item })
        }
    }).collect();

    // Generate Resonate registration helpers when feature is enabled.
    // For each public command, generate a standalone async function that
    // can be registered with Resonate via `resonate.register(...)`.
    let public_command_names: Vec<&str> = commands.iter()
        .filter(|c| !c.is_private)
        .map(|c| c.name.as_str())
        .collect();

    let expanded = quote! {
        impl #self_ty {
            #(#stripped_items)*
        }

        /// Returns command metadata and public command names for this #[commands] block.
        /// Module-level function to avoid conflicts when multiple files have #[commands] blocks.
        #[doc(hidden)]
        pub fn __commands_block_meta() -> (Vec<bluetext_model::metadata::CommandMethodMeta>, Vec<&'static str>) {
            let mod_path = module_path!();
            let root_module = mod_path.rsplit("::").next().unwrap_or(mod_path);
            let mut commands: Vec<bluetext_model::metadata::CommandMethodMeta> = vec![#(#command_meta_tokens),*];
            for c in &mut commands { c.source_module = Some(root_module.to_string()); }
            let public_names: Vec<&'static str> = vec![#(#public_command_names),*];
            (commands, public_names)
        }
    };

    TokenStream::from(expanded)
}

// ── Helper types ────────────────────────────────────────────────────

struct CallInfo {
    name: String,
    line: usize,
}

struct CommandInfo {
    name: String,
    params: Vec<(String, String)>,
    calls: Vec<CallInfo>,
    /// Full control flow step tree from the body walker.
    body: Vec<flow_walker::Step>,
    is_private: bool,
    constraints: Vec<String>,
    start_line: usize,
    end_line: usize,
}

enum CommandKind {
    Command { is_private: bool, constraints: Vec<String> },
    None,
}

fn classify_command_method(attrs: &[Attribute]) -> CommandKind {
    for attr in attrs {
        if is_command_attr(attr, "command") {
            let is_private = extract_private_flag(attr);
            let constraints = extract_constraints(attr);
            return CommandKind::Command { is_private, constraints };
        }
    }
    CommandKind::None
}

fn is_command_attr(attr: &Attribute, name: &str) -> bool {
    attr.path().is_ident(name)
}

fn extract_private_flag(attr: &Attribute) -> bool {
    if let Meta::List(meta_list) = &attr.meta {
        let tokens = meta_list.tokens.to_string();
        return tokens.contains("private");
    }
    false
}

/// Extract `constraints = ["booking", "cancellation"]` from `#[command(constraints = [...])]`.
fn extract_constraints(attr: &Attribute) -> Vec<String> {
    let Meta::List(meta_list) = &attr.meta else { return vec![] };
    let tokens = meta_list.tokens.to_string();
    // Find constraints = [...] in the token string
    let Some(start) = tokens.find("constraints") else { return vec![] };
    let after_eq = &tokens[start..];
    let Some(bracket_start) = after_eq.find('[') else { return vec![] };
    let Some(bracket_end) = after_eq.find(']') else { return vec![] };
    let inner = &after_eq[bracket_start + 1..bracket_end];
    inner.split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn extract_params(method: &syn::ImplItemFn) -> Vec<(String, String)> {
    method
        .sig
        .inputs
        .iter()
        .filter_map(|arg| {
            if let syn::FnArg::Typed(pat_type) = arg {
                if let syn::Pat::Ident(pat_ident) = pat_type.pat.as_ref() {
                    let name = pat_ident.ident.to_string();
                    if name == "self" {
                        return None;
                    }
                    // Skip the Resonate Context parameter
                    let ty = type_to_string(&pat_type.ty);
                    if ty.contains("Context") && ty.contains("resonate") {
                        return None;
                    }
                    return Some((name, ty));
                }
            }
            None
        })
        .collect()
}

/// Extract `self.xxx()` calls from the method body to detect which
/// mutations/getters/requests the command calls.
fn extract_self_calls(stmts: &[syn::Stmt]) -> Vec<CallInfo> {
    let mut calls = Vec::new();
    for stmt in stmts {
        extract_calls_from_stmt(stmt, &mut calls);
    }
    calls
}

fn extract_calls_from_stmt(stmt: &syn::Stmt, calls: &mut Vec<CallInfo>) {
    match stmt {
        syn::Stmt::Expr(expr, _) => extract_calls_from_expr(expr, calls),
        syn::Stmt::Local(local) => {
            if let Some(init) = &local.init {
                extract_calls_from_expr(&init.expr, calls);
            }
        }
        _ => {}
    }
}

fn extract_calls_from_expr(expr: &syn::Expr, calls: &mut Vec<CallInfo>) {
    match expr {
        syn::Expr::MethodCall(mc) => {
            if is_self_receiver(&mc.receiver) {
                let line = mc.method.span().start().line;
                calls.push(CallInfo { name: mc.method.to_string(), line });
            }
            extract_calls_from_expr(&mc.receiver, calls);
            for arg in &mc.args {
                extract_calls_from_expr(arg, calls);
            }
        }
        syn::Expr::Await(aw) => {
            extract_calls_from_expr(&aw.base, calls);
        }
        syn::Expr::Block(block) => {
            for stmt in &block.block.stmts {
                extract_calls_from_stmt(stmt, calls);
            }
        }
        syn::Expr::If(expr_if) => {
            extract_calls_from_expr(&expr_if.cond, calls);
            for stmt in &expr_if.then_branch.stmts {
                extract_calls_from_stmt(stmt, calls);
            }
            if let Some((_, else_expr)) = &expr_if.else_branch {
                extract_calls_from_expr(else_expr, calls);
            }
        }
        syn::Expr::Match(expr_match) => {
            extract_calls_from_expr(&expr_match.expr, calls);
            for arm in &expr_match.arms {
                extract_calls_from_expr(&arm.body, calls);
            }
        }
        syn::Expr::Try(expr_try) => {
            extract_calls_from_expr(&expr_try.expr, calls);
        }
        syn::Expr::Call(call) => {
            extract_calls_from_expr(&call.func, calls);
            for arg in &call.args {
                extract_calls_from_expr(arg, calls);
            }
        }
        _ => {}
    }
}

fn is_self_receiver(expr: &syn::Expr) -> bool {
    if let syn::Expr::Path(path) = expr {
        return path.path.is_ident("self");
    }
    false
}

fn generate_command_meta_tokens(commands: &[CommandInfo]) -> Vec<TokenStream2> {
    commands.iter().map(|c| {
        let name = &c.name;
        let params: Vec<TokenStream2> = c.params.iter().map(|(n, t)| {
            quote! { bluetext_model::metadata::FieldMeta { name: #n.to_string(), type_name: #t.to_string(), fields: vec![], references: None, key_for: None } }
        }).collect();
        let call_tokens: Vec<TokenStream2> = c.calls.iter().map(|ci| {
            let call_name = &ci.name;
            let call_line = ci.line as u32;
            quote! { bluetext_model::metadata::CallMeta { name: #call_name.to_string(), line: #call_line } }
        }).collect();
        // Serialize body steps to JSON at compile time
        let body_json: Vec<serde_json::Value> = c.body.iter().map(flow_walker::step_to_json).collect();
        let body_json_str = serde_json::to_string(&body_json).unwrap();
        let is_private = c.is_private;
        let constraints = &c.constraints;
        let start_line = c.start_line as u32;
        let end_line = c.end_line as u32;
        quote! {
            bluetext_model::metadata::CommandMethodMeta {
                name: #name.to_string(),
                params: vec![#(#params),*],
                calls: vec![#(#call_tokens),*],
                body: serde_json::from_str(#body_json_str).unwrap(),
                is_private: #is_private,
                source_file: Some(file!().to_string()),
                source_line: Some(#start_line),
                source_end_line: Some(#end_line),
                source_module: None,
                constraints: vec![#(#constraints.to_string()),*],
            }
        }
    }).collect()
}
