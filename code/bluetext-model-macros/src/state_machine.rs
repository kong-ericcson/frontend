use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input,
    spanned::Spanned,
    Attribute, Data, DeriveInput, Fields, Ident, ImplItem, ImplItemFn,
    ItemImpl, ItemType, Meta, Path, Token, Type,
};

use crate::analysis::analyze_field_access;

// ── #[derive(ModelType)] ────────────────────────────────────────────

pub fn derive_model_type_impl(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let name_str = name.to_string();

    let (kind_str, fields_tokens) = match &input.data {
        Data::Struct(data) => {
            let fields: Vec<TokenStream2> = match &data.fields {
                Fields::Named(named) => named
                    .named
                    .iter()
                    .map(|f| {
                        let fname = f.ident.as_ref().unwrap().to_string();
                        let ftype = type_to_string(&f.ty);
                        let refs = extract_references_attr(&f.attrs);
                        quote! {
                            bluetext_model::metadata::FieldMeta {
                                name: #fname.to_string(),
                                type_name: #ftype.to_string(),
                                fields: vec![],
                                references: #refs,
                                key_for: None,
                            }
                        }
                    })
                    .collect(),
                _ => vec![],
            };
            ("record", fields)
        }
        Data::Enum(data) => {
            let variants: Vec<TokenStream2> = data
                .variants
                .iter()
                .map(|v| {
                    let vname = v.ident.to_string();
                    let sub_fields: Vec<TokenStream2> = match &v.fields {
                        Fields::Named(named) => named
                            .named
                            .iter()
                            .map(|f| {
                                let fname = f.ident.as_ref().unwrap().to_string();
                                let ftype = type_to_string(&f.ty);
                                let refs = extract_references_attr(&f.attrs);
                                quote! {
                                    bluetext_model::metadata::FieldMeta {
                                        name: #fname.to_string(),
                                        type_name: #ftype.to_string(),
                                        fields: vec![],
                                        references: #refs,
                                    }
                                }
                            })
                            .collect(),
                        _ => vec![],
                    };
                    quote! {
                        bluetext_model::metadata::FieldMeta {
                            name: #vname.to_string(),
                            type_name: String::new(),
                            fields: vec![#(#sub_fields),*],
                            references: None,
                            key_for: None,
                        }
                    }
                })
                .collect();
            ("enum", variants)
        }
        _ => ("record", vec![]),
    };

    let kind_ident = format_ident!("{}", if kind_str == "record" { "Record" } else { "Enum" });

    // Extract source location from the type definition's span
    let start_line = name.span().start().line as u32;
    let end_line = match &input.data {
        Data::Struct(data) => data.fields.span().end().line as u32,
        Data::Enum(data) => data.brace_token.span.close().end().line as u32,
        _ => start_line,
    };

    let expanded = quote! {
        impl bluetext_model::metadata::ModelTypeMeta for #name {
            fn type_meta() -> bluetext_model::metadata::TypeMeta {
                let mod_path = module_path!();
                let source_mod = mod_path.rsplit("::").next().unwrap_or(mod_path);
                bluetext_model::metadata::TypeMeta {
                    name: #name_str.to_string(),
                    kind: bluetext_model::metadata::TypeKind::#kind_ident,
                    fields: vec![#(#fields_tokens),*],
                    source_module: Some(source_mod.to_string()),
                    source_line: Some(#start_line),
                    source_end_line: Some(#end_line),
                }
            }
        }
    };

    TokenStream::from(expanded)
}

// ── #[model_alias] ──────────────────────────────────────────────────

pub fn model_alias_impl(input: TokenStream) -> TokenStream {
    let item = parse_macro_input!(input as ItemType);
    let name = &item.ident;
    let name_str = name.to_string();
    let target_str = type_to_string(&item.ty);

    let fn_name = format_ident!("__model_alias_{}", name_str.to_lowercase());

    let expanded = quote! {
        #item

        #[doc(hidden)]
        pub fn #fn_name() -> bluetext_model::metadata::AliasMeta {
            bluetext_model::metadata::AliasMeta {
                name: #name_str.to_string(),
                target: #target_str.to_string(),
            }
        }
    };

    TokenStream::from(expanded)
}

// ── #[derive(StateMachine)] ─────────────────────────────────────────

pub fn derive_state_machine_impl(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    // Parse #[state_machine(name = "...")] attribute
    let module_name = extract_state_machine_name(&input.attrs).unwrap_or_else(|| name.to_string());

    // Extract fields as state variables (skip Stores field)
    let state_vars: Vec<TokenStream2> = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(named) => named
                .named
                .iter()
                .filter(|f| type_to_string(&f.ty) != "Stores")
                .map(|f| {
                    let fname = f.ident.as_ref().unwrap().to_string();
                    let ftype = type_to_string(&f.ty);
                    let type_refs = extract_type_refs(&f.ty);
                    let line = f.ident.as_ref().unwrap().span().start().line as u32;
                    quote! {
                        bluetext_model::metadata::StateVarMeta {
                            name: #fname.to_string(),
                            type_name: #ftype.to_string(),
                            type_refs: vec![#(#type_refs.to_string()),*],
                            source_module: None,
                            source_file: Some(file!().to_string()),
                            source_line: Some(#line),
                        }
                    }
                })
                .collect(),
            _ => vec![],
        },
        _ => vec![],
    };

    // Find the Stores field name for reset/dump delegation
    let stores_field = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(named) => named
                .named
                .iter()
                .find(|f| type_to_string(&f.ty) == "Stores")
                .map(|f| f.ident.as_ref().unwrap().clone()),
            _ => None,
        },
        _ => None,
    };

    // Generate reset/dump helper methods if a #[stores] field exists
    let stores_methods = if let Some(ref field_name) = stores_field {
        quote! {
            /// Reset all state stores (generated from #[stores] field).
            pub async fn __stores_reset(&mut self) {
                self.#field_name.reset().await;
            }

            /// Dump all state stores (generated from #[stores] field).
            pub async fn __stores_dump(&self) -> String {
                self.#field_name.dump().await
            }
        }
    } else {
        quote! {
            /// No-op reset (no #[stores] field — in-memory state cleared by init).
            pub async fn __stores_reset(&mut self) {}

            /// Serialize struct fields as JSON (no #[stores] field — in-memory state).
            pub async fn __stores_dump(&self) -> String {
                serde_json::to_string_pretty(self).unwrap_or_default()
            }
        }
    };

    // Generate collection_keys match arms from struct fields
    let collection_keys_arms: Vec<TokenStream2> = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(named) => named
                .named
                .iter()
                .filter(|f| {
                    let ty = type_to_string(&f.ty);
                    ty != "Stores" && (ty.starts_with("CouchbaseCollection") || ty.starts_with("HashMap"))
                })
                .map(|f| {
                    let fname = f.ident.as_ref().unwrap();
                    let fname_str = fname.to_string();
                    let ty = type_to_string(&f.ty);
                    if ty.starts_with("CouchbaseCollection") {
                        quote! { #fname_str => self.#fname.keys().await, }
                    } else {
                        // HashMap<String, V>
                        quote! { #fname_str => self.#fname.keys().cloned().collect(), }
                    }
                })
                .collect(),
            _ => vec![],
        },
        _ => vec![],
    };

    let expanded = quote! {
        impl #name {
            /// Returns the metadata for this state machine (state vars only).
            /// Mutations, getters, state invariants added by #[state_machine_impl].
            pub fn __state_machine_base_meta() -> (String, Vec<bluetext_model::metadata::StateVarMeta>) {
                (
                    #module_name.to_string(),
                    vec![#(#state_vars),*],
                )
            }

            #stores_methods

            /// Get all keys from a named collection. Generated from struct fields.
            #[doc(hidden)]
            pub async fn __collection_keys(&self, name: &str) -> Vec<String> {
                match name {
                    #(#collection_keys_arms)*
                    _ => vec![],
                }
            }
        }
    };

    TokenStream::from(expanded)
}

// ── #[state_machine_actions] ─────────────────────────────────────────
// Per-module attribute: generates methods + metadata function. No trait impls.

pub fn state_machine_actions_impl(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as ItemImpl);
    let self_ty = &input.self_ty;

    let mut mutations = Vec::new();
    let mut getters = Vec::new();
    let mut state_invariants = Vec::new();

    for item in &input.items {
        if let ImplItem::Fn(method) = item {
            let attrs = classify_method(&method.attrs);
            let method_name = method.sig.ident.to_string();
            let start_line = method.sig.ident.span().start().line;
            let end_line = method.block.brace_token.span.close().end().line;
            match attrs {
                MethodKind::Mutation(explicit_modifies) => {
                    let (modifies, reads) = if let Some(explicit) = explicit_modifies {
                        (explicit.clone(), analyze_reads(&method.block.stmts))
                    } else {
                        analyze_field_access(&method.block.stmts)
                    };
                    let params = extract_params(method);
                    mutations.push(MutationInfo { name: method_name, params, modifies, reads, start_line, end_line });
                }
                MethodKind::Getter => {
                    let (_, reads) = analyze_field_access(&method.block.stmts);
                    let params = extract_params(method);
                    getters.push(GetterInfo { name: method_name, params, reads, start_line, end_line });
                }
                MethodKind::StateInvariant => {
                    let (_, reads) = analyze_field_access(&method.block.stmts);
                    state_invariants.push(StateInvariantInfo { name: method_name, reads, start_line, end_line });
                }
                _ => {} // simulation_init/simulation_step should only be in the primary state_machine_impl
            }
        }
    }

    let mutation_tokens = generate_mutation_meta_tokens(&mutations);
    let getter_tokens = generate_getter_meta_tokens(&getters);
    let state_invariant_tokens = generate_state_invariant_meta_tokens(&state_invariants);

    // Generate call_mutation arms for this module's mutations
    let module_mutation_call_arms: Vec<TokenStream2> = mutations
        .iter()
        .map(|m| {
            let name = &m.name;
            let ident = format_ident!("{}", name);
            let param_bindings: Vec<TokenStream2> = m.params.iter().map(|(pname, ptype, _key_for)| {
                let pident = format_ident!("{}", pname);
                let pty: syn::Type = syn::parse_str(ptype).unwrap();
                quote! {
                    let #pident: #pty = match serde_json::from_value(__args[#pname].clone()) {
                        Ok(v) => v,
                        Err(_) => return Some(false),
                    };
                }
            }).collect();
            let param_idents: Vec<proc_macro2::Ident> = m.params.iter().map(|(pname, _, _)| format_ident!("{}", pname)).collect();
            // All mutations in state_machine_actions are async (CouchbaseCollection requires it)
            quote! {
                #name => {
                    #(#param_bindings)*
                    state.#ident(#(#param_idents),*).await;
                    return Some(true);
                }
            }
        })
        .collect();

    // Generate check_invariant arms for this module's invariants
    let module_invariant_check_arms: Vec<TokenStream2> = state_invariants
        .iter()
        .map(|inv| {
            let name = &inv.name;
            let ident = format_ident!("{}", name);
            // State invariants are always async when using CouchbaseCollection
            quote! {
                #name => return Some(state.#ident().await),
            }
        })
        .collect();

    let stripped_items: Vec<TokenStream2> = strip_our_attrs(&input.items);

    let expanded = quote! {
        impl #self_ty {
            #(#stripped_items)*
        }

        /// Try to dispatch a mutation call for this module. Returns Some(result) if matched.
        #[doc(hidden)]
        pub async fn __sm_actions_call_mutation(state: &mut #self_ty, name: &str, __args: &serde_json::Value) -> Option<bool> {
            match name {
                #(#module_mutation_call_arms)*
                _ => None,
            }
        }

        /// Try to dispatch an invariant check for this module. Returns Some(result) if matched.
        #[doc(hidden)]
        pub async fn __sm_actions_check_invariant(state: & #self_ty, name: &str) -> Option<bool> {
            match name {
                #(#module_invariant_check_arms)*
                _ => None,
            }
        }

        /// Metadata registration for this module's mutations, getters, and constraints.
        #[doc(hidden)]
        pub fn __sm_actions_meta() -> (
            &'static str,
            Vec<bluetext_model::metadata::MutationMeta>,
            Vec<bluetext_model::metadata::GetterMeta>,
            Vec<bluetext_model::metadata::StateInvariantMeta>,
        ) {
            let mod_path: &'static str = module_path!();
            let module_name: &'static str = mod_path.rsplit("::").next().unwrap_or(mod_path);
            let mut mutations: Vec<bluetext_model::metadata::MutationMeta> = vec![#(#mutation_tokens),*];
            let mut getters: Vec<bluetext_model::metadata::GetterMeta> = vec![#(#getter_tokens),*];
            let mut state_invariants: Vec<bluetext_model::metadata::StateInvariantMeta> = vec![#(#state_invariant_tokens),*];
            for m in &mut mutations { m.source_module = Some(module_name.to_string()); }
            for g in &mut getters { g.source_module = Some(module_name.to_string()); }
            for i in &mut state_invariants { i.source_module = Some(module_name.to_string()); }
            (module_name, mutations, getters, state_invariants)
        }
    };

    TokenStream::from(expanded)
}

// ── #[state_machine_impl] ───────────────────────────────────────────
// Primary attribute: generates methods + trait impls. Accepts extra_actions/extra_state_invariants/extra_meta.

pub fn state_machine_impl_impl(attr: TokenStream, input: TokenStream) -> TokenStream {
    let extra = syn::parse_macro_input!(attr as ExtraConfig);
    let input = parse_macro_input!(input as ItemImpl);
    let self_ty = &input.self_ty;

    let mut mutations = Vec::new();
    let mut getters = Vec::new();
    let mut state_invariants = Vec::new();
    let mut simulation_inits = Vec::new();
    let mut simulation_steps = Vec::new();
    let mut method_is_async: Vec<(String, bool)> = Vec::new();

    for item in &input.items {
        if let ImplItem::Fn(method) = item {
            let attrs = classify_method(&method.attrs);
            let method_name = method.sig.ident.to_string();
            let is_async = method.sig.asyncness.is_some();

            let start_line = method.sig.ident.span().start().line;
            let end_line = method.block.brace_token.span.close().end().line;

            match attrs {
                MethodKind::Mutation(explicit_modifies) => {
                    let (modifies, reads) = if let Some(explicit) = explicit_modifies {
                        (explicit.clone(), analyze_reads(&method.block.stmts))
                    } else {
                        analyze_field_access(&method.block.stmts)
                    };
                    let params = extract_params(method);
                    method_is_async.push((method_name.clone(), is_async));
                    mutations.push(MutationInfo { name: method_name, params, modifies, reads, start_line, end_line });
                }
                MethodKind::Getter => {
                    let (_, reads) = analyze_field_access(&method.block.stmts);
                    let params = extract_params(method);
                    method_is_async.push((method_name.clone(), is_async));
                    getters.push(GetterInfo { name: method_name, params, reads, start_line, end_line });
                }
                MethodKind::SimulationInit => {
                    method_is_async.push((method_name.clone(), is_async));
                    simulation_inits.push(SimulationInitInfo { name: method_name, start_line, end_line });
                }
                MethodKind::SimulationStep => {
                    method_is_async.push((method_name.clone(), is_async));
                    simulation_steps.push(SimulationStepInfo { name: method_name, start_line, end_line });
                }
                MethodKind::StateInvariant => {
                    let (_, reads) = analyze_field_access(&method.block.stmts);
                    method_is_async.push((method_name.clone(), is_async));
                    state_invariants.push(StateInvariantInfo { name: method_name, reads, start_line, end_line });
                }
                MethodKind::None => {}
            }
        }
    }

    let mutation_tokens = generate_mutation_meta_tokens(&mutations);
    let getter_tokens = generate_getter_meta_tokens(&getters);
    let state_invariant_tokens = generate_state_invariant_meta_tokens(&state_invariants);
    let simulation_init_tokens = generate_simulation_init_meta_tokens(&simulation_inits);
    let simulation_step_tokens = generate_simulation_step_meta_tokens(&simulation_steps);

    // Generate async Simulatable match arms — call methods directly (no sync transformation)
    let init_arms: Vec<TokenStream2> = simulation_inits
        .iter()
        .map(|si| {
            let name = &si.name;
            let ident = format_ident!("{}", name);
            let call = if method_is_async.iter().any(|(n, is_async)| n == name && *is_async) {
                quote! { self.#ident().await; true }
            } else {
                quote! { self.#ident(); true }
            };
            quote! { #name => { #call }, }
        })
        .collect();

    let step_arms: Vec<TokenStream2> = simulation_steps
        .iter()
        .map(|ss| {
            let name = &ss.name;
            let ident = format_ident!("{}", name);
            let call = if method_is_async.iter().any(|(n, is_async)| n == name && *is_async) {
                quote! { self.#ident().await }
            } else {
                quote! { self.#ident() }
            };
            quote! { #name => #call, }
        })
        .collect();

    let mut state_invariant_arms: Vec<TokenStream2> = state_invariants
        .iter()
        .map(|inv| {
            let name = &inv.name;
            let ident = format_ident!("{}", name);
            // State invariants may be async too (querying DB)
            let is_async = method_is_async.iter().any(|(n, a)| n == name && *a);
            let call = if is_async {
                quote! { self.#ident().await }
            } else {
                quote! { self.#ident() }
            };
            quote! { #name => #call, }
        })
        .collect();

    // Add extra_state_invariants dispatch arms (constraints are parameterless, safe to dispatch)
    for name in &extra.extra_state_invariants {
        let name_str = name.to_string();
        state_invariant_arms.push(quote! { #name_str => self.#name(), });
    }

    let init_name_strs: Vec<String> = simulation_inits.iter().map(|si| si.name.clone()).collect();
    let step_name_strs: Vec<String> = simulation_steps.iter().map(|ss| ss.name.clone()).collect();
    let mut state_invariant_name_strs: Vec<String> = state_invariants.iter().map(|i| i.name.clone()).collect();

    // Include extra names in available lists
    for name in &extra.extra_state_invariants {
        state_invariant_name_strs.push(name.to_string());
    }

    // Generate call_mutation match arms: deserialize JSON args and call mutation
    let mutation_call_arms: Vec<TokenStream2> = mutations
        .iter()
        .map(|m| {
            let name = &m.name;
            let ident = format_ident!("{}", name);
            let is_async = method_is_async.iter().any(|(n, a)| n == name && *a);
            let param_bindings: Vec<TokenStream2> = m.params.iter().map(|(pname, ptype, _key_for)| {
                let pident = format_ident!("{}", pname);
                let pty: syn::Type = syn::parse_str(ptype).unwrap();
                quote! {
                    let #pident: #pty = match serde_json::from_value(__args[#pname].clone()) {
                        Ok(v) => v,
                        Err(_) => return false,
                    };
                }
            }).collect();
            let param_idents: Vec<proc_macro2::Ident> = m.params.iter().map(|(pname, _, _)| format_ident!("{}", pname)).collect();
            let call = if is_async {
                quote! { self.#ident(#(#param_idents),*).await }
            } else {
                quote! { self.#ident(#(#param_idents),*) }
            };
            quote! {
                #name => {
                    #(#param_bindings)*
                    { #call; }
                    true
                }
            }
        })
        .collect();

    // Generate extra module dispatch calls from extra_meta paths
    // For each path like `inventory::__sm_actions_meta`, derive the call_mutation and check_invariant paths
    let extra_call_mutation_dispatches: Vec<TokenStream2> = extra.extra_meta.iter().map(|path| {
        let mut segments = path.segments.clone();
        if let Some(last) = segments.last_mut() { last.ident = format_ident!("__sm_actions_call_mutation"); }
        let call_path = syn::Path { leading_colon: path.leading_colon, segments };
        quote! {
            if let Some(result) = #call_path(self, name, __args).await {
                return result;
            }
        }
    }).collect();

    let extra_check_invariant_dispatches: Vec<TokenStream2> = extra.extra_meta.iter().map(|path| {
        let mut segments = path.segments.clone();
        if let Some(last) = segments.last_mut() { last.ident = format_ident!("__sm_actions_check_invariant"); }
        let call_path = syn::Path { leading_colon: path.leading_colon, segments };
        quote! {
            if let Some(result) = #call_path(self, name).await {
                return result;
            }
        }
    }).collect();

    // Extra meta function calls for metadata()
    let extra_meta_calls: Vec<TokenStream2> = extra.extra_meta.iter().map(|path| {
        quote! {
            {
                let (_module_name, extra_mutations, extra_getters, extra_state_invariants) = #path();
                all_mutations.extend(extra_mutations);
                all_getters.extend(extra_getters);
                all_state_invariants.extend(extra_state_invariants);
            }
        }
    }).collect();

    let stripped_items: Vec<TokenStream2> = strip_our_attrs(&input.items);

    let expanded = quote! {
        impl #self_ty {
            #(#stripped_items)*
        }

        impl bluetext_model::metadata::StateMachineModel for #self_ty {
            fn metadata() -> bluetext_model::metadata::StateMachineMeta {
                let (module, state_vars) = Self::__state_machine_base_meta();
                let mod_path = module_path!();
                let root_module = mod_path.rsplit("::").next().unwrap_or(mod_path);
                let mut all_mutations: Vec<bluetext_model::metadata::MutationMeta> = vec![#(#mutation_tokens),*];
                let mut all_getters: Vec<bluetext_model::metadata::GetterMeta> = vec![#(#getter_tokens),*];
                let mut all_state_invariants: Vec<bluetext_model::metadata::StateInvariantMeta> = vec![#(#state_invariant_tokens),*];
                for m in &mut all_mutations { m.source_module = Some(root_module.to_string()); }
                for g in &mut all_getters { g.source_module = Some(root_module.to_string()); }
                for i in &mut all_state_invariants { i.source_module = Some(root_module.to_string()); }
                #(#extra_meta_calls)*
                bluetext_model::metadata::StateMachineMeta {
                    module,
                    modules: vec![],
                    types: vec![],
                    aliases: vec![],
                    state_vars,
                    mutations: all_mutations,
                    getters: all_getters,
                    commands: vec![],
                    requests: vec![],
                    controllers: vec![],
                    access: vec![],
                    state_invariants: all_state_invariants,
                    eventual_consistency_constraints: vec![],
                    simulation_inits: vec![#(#simulation_init_tokens),*],
                    simulation_steps: vec![#(#simulation_step_tokens),*],
                }
            }

        }

        impl bluetext_model::simulation::Simulatable for #self_ty {
            fn available_simulation_inits() -> Vec<&'static str> {
                vec![#(#init_name_strs),*]
            }
            fn available_simulation_steps() -> Vec<&'static str> {
                vec![#(#step_name_strs),*]
            }
            fn available_state_invariants() -> Vec<&'static str> {
                vec![#(#state_invariant_name_strs),*]
            }
            async fn reset(&mut self) {
                self.__stores_reset().await;
            }
            async fn dump(&self) -> String {
                self.__stores_dump().await
            }
            async fn call_simulation_init(&mut self, name: &str) -> bool {
                match name {
                    #(#init_arms)*
                    _ => false,
                }
            }
            async fn call_simulation_step(&mut self, name: &str) -> bool {
                match name {
                    #(#step_arms)*
                    _ => false,
                }
            }
            async fn check_state_invariant(&self, name: &str) -> bool {
                match name {
                    #(#state_invariant_arms)*
                    _ => {}
                }
                // Try extra module invariant dispatchers
                #(#extra_check_invariant_dispatches)*
                true // Unknown invariant — assume holds
            }
            async fn call_mutation(&mut self, name: &str, __args: &serde_json::Value) -> bool {
                match name {
                    #(#mutation_call_arms)*
                    _ => {}
                }
                // Try extra module dispatchers
                #(#extra_call_mutation_dispatches)*
                false
            }
            async fn collection_keys(&self, name: &str) -> Vec<String> {
                self.__collection_keys(name).await
            }
        }
    };

    TokenStream::from(expanded)
}

// ── ExtraConfig for state_machine_impl attribute ────────────────────

struct ExtraConfig {
    _extra_actions: Vec<Ident>,
    extra_state_invariants: Vec<Ident>,
    extra_meta: Vec<Path>,
}

impl Parse for ExtraConfig {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut extra_actions = Vec::new();
        let mut extra_state_invariants = Vec::new();
        let mut extra_meta = Vec::new();

        while !input.is_empty() {
            let key: Ident = input.parse()?;
            let _: Token![=] = input.parse()?;

            let content;
            syn::bracketed!(content in input);

            match key.to_string().as_str() {
                "extra_actions" => {
                    while !content.is_empty() {
                        extra_actions.push(content.parse::<Ident>()?);
                        if content.peek(Token![,]) { let _: Token![,] = content.parse()?; }
                    }
                }
                // Accept both old and new name for backward compat
                "extra_state_invariants" => {
                    while !content.is_empty() {
                        extra_state_invariants.push(content.parse::<Ident>()?);
                        if content.peek(Token![,]) { let _: Token![,] = content.parse()?; }
                    }
                }
                "extra_meta" => {
                    while !content.is_empty() {
                        extra_meta.push(content.parse::<Path>()?);
                        if content.peek(Token![,]) { let _: Token![,] = content.parse()?; }
                    }
                }
                _ => return Err(syn::Error::new(key.span(), format!("unknown key '{}'", key))),
            }

            if input.peek(Token![,]) { let _: Token![,] = input.parse()?; }
        }

        Ok(ExtraConfig { _extra_actions: extra_actions, extra_state_invariants, extra_meta })
    }
}

// ── Shared helpers for metadata token generation ────────────────────

fn generate_mutation_meta_tokens(mutations: &[MutationInfo]) -> Vec<TokenStream2> {
    mutations.iter().map(|m| {
        let name = &m.name;
        let params: Vec<TokenStream2> = m.params.iter().map(|(n, t, k)| {
            let key_for = match k {
                Some(kf) => quote! { Some(#kf.to_string()) },
                None => quote! { None },
            };
            quote! { bluetext_model::metadata::FieldMeta { name: #n.to_string(), type_name: #t.to_string(), fields: vec![], references: None, key_for: #key_for } }
        }).collect();
        let modifies = &m.modifies;
        let reads = &m.reads;
        let start_line = m.start_line as u32;
        let end_line = m.end_line as u32;
        quote! {
            bluetext_model::metadata::MutationMeta {
                name: #name.to_string(),
                params: vec![#(#params),*],
                modifies: vec![#(#modifies.to_string()),*],
                reads: vec![#(#reads.to_string()),*],
                source_file: Some(file!().to_string()), source_line: Some(#start_line), source_end_line: Some(#end_line), source_module: None,
            }
        }
    }).collect()
}

fn generate_getter_meta_tokens(getters: &[GetterInfo]) -> Vec<TokenStream2> {
    getters.iter().map(|g| {
        let name = &g.name;
        let params: Vec<TokenStream2> = g.params.iter().map(|(n, t, k)| {
            let key_for = match k {
                Some(kf) => quote! { Some(#kf.to_string()) },
                None => quote! { None },
            };
            quote! { bluetext_model::metadata::FieldMeta { name: #n.to_string(), type_name: #t.to_string(), fields: vec![], references: None, key_for: #key_for } }
        }).collect();
        let reads = &g.reads;
        let start_line = g.start_line as u32;
        let end_line = g.end_line as u32;
        quote! {
            bluetext_model::metadata::GetterMeta {
                name: #name.to_string(),
                params: vec![#(#params),*],
                reads: vec![#(#reads.to_string()),*],
                source_file: Some(file!().to_string()), source_line: Some(#start_line), source_end_line: Some(#end_line), source_module: None,
            }
        }
    }).collect()
}

fn generate_simulation_init_meta_tokens(inits: &[SimulationInitInfo]) -> Vec<TokenStream2> {
    inits.iter().map(|i| {
        let name = &i.name;
        let start_line = i.start_line as u32;
        let end_line = i.end_line as u32;
        quote! {
            bluetext_model::metadata::SimulationInitMeta {
                name: #name.to_string(),
                source_file: Some(file!().to_string()), source_line: Some(#start_line), source_end_line: Some(#end_line), source_module: None,
            }
        }
    }).collect()
}

fn generate_simulation_step_meta_tokens(steps: &[SimulationStepInfo]) -> Vec<TokenStream2> {
    steps.iter().map(|s| {
        let name = &s.name;
        let start_line = s.start_line as u32;
        let end_line = s.end_line as u32;
        quote! {
            bluetext_model::metadata::SimulationStepMeta {
                name: #name.to_string(),
                source_file: Some(file!().to_string()), source_line: Some(#start_line), source_end_line: Some(#end_line), source_module: None,
            }
        }
    }).collect()
}

fn generate_state_invariant_meta_tokens(constraints: &[StateInvariantInfo]) -> Vec<TokenStream2> {
    constraints.iter().map(|inv| {
        let name = &inv.name;
        let reads = &inv.reads;
        let start_line = inv.start_line as u32;
        let end_line = inv.end_line as u32;
        quote! {
            bluetext_model::metadata::StateInvariantMeta {
                name: #name.to_string(),
                reads: vec![#(#reads.to_string()),*],
                source_file: Some(file!().to_string()), source_line: Some(#start_line), source_end_line: Some(#end_line), source_module: None,
            }
        }
    }).collect()
}

fn strip_our_attrs(items: &[ImplItem]) -> Vec<TokenStream2> {
    items.iter().map(|item| {
        if let ImplItem::Fn(method) = item {
            let is_mutation = method.attrs.iter().any(|a| is_our_attr(a, "mutation"));
            let mut cleaned = method.clone();
            cleaned.attrs.retain(|attr| {
                !is_our_attr(attr, "mutation")
                    && !is_our_attr(attr, "getter")
                    && !is_our_attr(attr, "simulation_init")
                    && !is_our_attr(attr, "simulation_step")
                    && !is_our_attr(attr, "state_invariant")
            });
            // Strip #[key(...)] from parameters
            for input in &mut cleaned.sig.inputs {
                if let syn::FnArg::Typed(pat_type) = input {
                    pat_type.attrs.retain(|a| !a.path().is_ident("key"));
                }
            }
            // Inject trace_action at the start of #[mutation] methods
            if is_mutation {
                let name = method.sig.ident.to_string();
                let trace_stmt: syn::Stmt = syn::parse_quote! {
                    bluetext_model::simulation::trace_action(#name);
                };
                cleaned.block.stmts.insert(0, trace_stmt);
            }
            quote! { #cleaned }
        } else {
            quote! { #item }
        }
    }).collect()
}

// ── Helper types and functions ──────────────────────────────────────

struct MutationInfo {
    name: String,
    params: Vec<(String, String, Option<String>)>,  // (name, type, key_for)
    modifies: Vec<String>,
    reads: Vec<String>,
    start_line: usize,
    end_line: usize,
}

struct GetterInfo {
    name: String,
    params: Vec<(String, String, Option<String>)>,
    reads: Vec<String>,
    start_line: usize,
    end_line: usize,
}

struct SimulationInitInfo {
    name: String,
    start_line: usize,
    end_line: usize,
}

struct SimulationStepInfo {
    name: String,
    start_line: usize,
    end_line: usize,
}

struct StateInvariantInfo {
    name: String,
    reads: Vec<String>,
    start_line: usize,
    end_line: usize,
}

enum MethodKind {
    Mutation(Option<Vec<String>>),
    Getter,
    SimulationInit,
    SimulationStep,
    StateInvariant,
    None,
}

fn classify_method(attrs: &[Attribute]) -> MethodKind {
    for attr in attrs {
        if is_our_attr(attr, "mutation") {
            let explicit_modifies = extract_modifies_attr(attr);
            return MethodKind::Mutation(explicit_modifies);
        }
        if is_our_attr(attr, "getter") {
            return MethodKind::Getter;
        }
        if is_our_attr(attr, "simulation_init") {
            return MethodKind::SimulationInit;
        }
        if is_our_attr(attr, "simulation_step") {
            return MethodKind::SimulationStep;
        }
        if is_our_attr(attr, "state_invariant") {
            return MethodKind::StateInvariant;
        }
    }
    MethodKind::None
}

fn is_our_attr(attr: &Attribute, name: &str) -> bool {
    attr.path().is_ident(name)
}

fn extract_modifies_attr(attr: &Attribute) -> Option<Vec<String>> {
    // Parse #[mutation(modifies = [field1, field2])]
    if let Meta::List(meta_list) = &attr.meta {
        let tokens = meta_list.tokens.to_string();
        if let Some(start) = tokens.find('[') {
            if let Some(end) = tokens.find(']') {
                let fields_str = &tokens[start + 1..end];
                let fields: Vec<String> = fields_str
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if !fields.is_empty() {
                    return Some(fields);
                }
            }
        }
    }
    None
}

/// Extract `#[references(TypeName)]` from field attributes.
fn extract_references_attr(attrs: &[Attribute]) -> TokenStream2 {
    for attr in attrs {
        if attr.path().is_ident("references") {
            if let Meta::List(meta_list) = &attr.meta {
                let type_name = meta_list.tokens.to_string().trim().to_string();
                if !type_name.is_empty() {
                    return quote! { Some(#type_name.to_string()) };
                }
            }
        }
    }
    quote! { None }
}

/// Extracted parameter info: (name, type, key_for).
fn extract_params(method: &ImplItemFn) -> Vec<(String, String, Option<String>)> {
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
                    let ty = type_to_string(&pat_type.ty);
                    let key_for = extract_key_attr(&pat_type.attrs);
                    return Some((name, ty, key_for));
                }
            }
            None
        })
        .collect()
}

/// Extract `#[key(collection_name)]` from param attributes.
fn extract_key_attr(attrs: &[Attribute]) -> Option<String> {
    for attr in attrs {
        if attr.path().is_ident("key") {
            if let Meta::List(meta_list) = &attr.meta {
                return Some(meta_list.tokens.to_string().trim().to_string());
            }
        }
    }
    None
}

fn analyze_reads(stmts: &[syn::Stmt]) -> Vec<String> {
    let (_, reads) = analyze_field_access(stmts);
    reads
}

fn extract_state_machine_name(attrs: &[Attribute]) -> Option<String> {
    for attr in attrs {
        if attr.path().is_ident("state_machine") {
            if let Meta::List(meta_list) = &attr.meta {
                let tokens = meta_list.tokens.to_string();
                // Parse: name = "Bank"
                if let Some(eq_pos) = tokens.find('=') {
                    let value = tokens[eq_pos + 1..].trim().trim_matches('"').to_string();
                    if !value.is_empty() {
                        return Some(value);
                    }
                }
            }
        }
    }
    None
}

/// Convert a syn::Type to a display string.
pub fn type_to_string(ty: &Type) -> String {
    quote!(#ty).to_string().replace(" ", "")
}

/// Extract type references (capitalized identifiers) from a type.
/// E.g., `HashMap<AccountId, Account>` → ["AccountId", "Account"]
pub fn extract_type_refs(ty: &Type) -> Vec<String> {
    let mut refs = Vec::new();
    collect_type_refs(ty, &mut refs);
    refs.sort();
    refs.dedup();
    // Filter to only capitalized names (likely user-defined types)
    refs.retain(|r| {
        r.chars().next().map_or(false, |c| c.is_uppercase())
            && !is_builtin_type(r)
    });
    refs
}

fn collect_type_refs(ty: &Type, refs: &mut Vec<String>) {
    match ty {
        Type::Path(type_path) => {
            for segment in &type_path.path.segments {
                let name = segment.ident.to_string();
                refs.push(name);
                if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                    for arg in &args.args {
                        if let syn::GenericArgument::Type(inner_ty) = arg {
                            collect_type_refs(inner_ty, refs);
                        }
                    }
                }
            }
        }
        Type::Reference(type_ref) => {
            collect_type_refs(&type_ref.elem, refs);
        }
        _ => {}
    }
}

fn is_builtin_type(name: &str) -> bool {
    matches!(
        name,
        "HashMap" | "BTreeMap" | "Vec" | "Option" | "Result" | "String"
        | "Box" | "Arc" | "Rc" | "HashSet" | "BTreeSet" | "VecDeque"
        | "Stores"
    )
}
