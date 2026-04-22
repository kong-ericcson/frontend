use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input, Ident, Path, Token,
};

struct ModelConfig {
    state_machine: Path,
    source_dir: String,
    modules: Vec<Ident>,
    types: Vec<Path>,
    /// Paths to __commands_block_meta functions from #[commands] blocks.
    /// `commands: true` is shorthand for the state machine's own module.
    command_sources: Vec<Path>,
    state_factory: Option<syn::Expr>,
}

impl Parse for ModelConfig {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut state_machine = None;
        let mut source_dir = None;
        let mut modules = Vec::new();
        let mut types = Vec::new();
        let mut command_sources: Vec<Path> = Vec::new();
        let mut state_factory = None;

        while !input.is_empty() {
            let key: Ident = input.parse()?;
            let _: Token![:] = input.parse()?;

            match key.to_string().as_str() {
                "state_machine" => {
                    state_machine = Some(input.parse::<Path>()?);
                }
                "source_dir" => {
                    let lit: syn::LitStr = input.parse()?;
                    source_dir = Some(lit.value());
                }
                "modules" => {
                    let content;
                    syn::bracketed!(content in input);
                    while !content.is_empty() {
                        modules.push(content.parse::<Ident>()?);
                        if content.peek(Token![,]) { let _: Token![,] = content.parse()?; }
                    }
                }
                "types" => {
                    let content;
                    syn::bracketed!(content in input);
                    while !content.is_empty() {
                        types.push(content.parse::<Path>()?);
                        if content.peek(Token![,]) { let _: Token![,] = content.parse()?; }
                    }
                }
                "commands" => {
                    let content;
                    syn::bracketed!(content in input);
                    while !content.is_empty() {
                        command_sources.push(content.parse::<Path>()?);
                        if content.peek(Token![,]) { let _: Token![,] = content.parse()?; }
                    }
                }
                "state_factory" => {
                    state_factory = Some(input.parse::<syn::Expr>()?);
                }
                _ => return Err(syn::Error::new(key.span(), format!("unknown key '{}'", key))),
            }

            if input.peek(Token![,]) { let _: Token![,] = input.parse()?; }
        }

        Ok(ModelConfig {
            state_machine: state_machine.ok_or_else(|| input.error("missing `state_machine`"))?,
            source_dir: source_dir.ok_or_else(|| input.error("missing `source_dir`"))?,
            modules,
            types,
            command_sources,
            state_factory,
        })
    }
}

pub fn model_impl(input: TokenStream) -> TokenStream {
    let config = parse_macro_input!(input as ModelConfig);
    let sm = &config.state_machine;
    let source_dir = &config.source_dir;

    let state_factory_expr = match &config.state_factory {
        Some(expr) => quote! { || async { #expr } },
        None => quote! { || async { <#sm as Default>::default() } },
    };

    // Generate module entries
    let module_entries: Vec<TokenStream2> = config.modules.iter().map(|m| {
        let name = m.to_string();
        let file = format!("{}/{}.rs", source_dir, name);
        quote! {
            bluetext_model::metadata::ModuleEntry {
                name: #name.to_string(),
                file: #file.to_string(),
            }
        }
    }).collect();

    // Generate type collection
    let type_entries: Vec<TokenStream2> = config.types.iter().map(|ty| {
        quote! {
            <#ty as bluetext_model::metadata::ModelTypeMeta>::type_meta()
        }
    }).collect();

    // Infer state_var source_module from action modifies data.
    // Prefer sub-module ownership over root module: if a sub-module action modifies
    // a field, that sub-module owns the state var. Root module is the fallback.
    let sm_str = config.state_machine.segments.last()
        .map(|s| s.ident.to_string())
        .unwrap_or_default();
    let root_module_name = pascal_to_snake(&sm_str)
        .strip_suffix("_state").unwrap_or(&pascal_to_snake(&sm_str)).to_string();

    let collect_commands = if !config.command_sources.is_empty() {
        let sources = &config.command_sources;
        quote! {
            {
                let mut all_commands = Vec::new();
                #(
                    {
                        let (cmds, _names) = #sources();
                        all_commands.extend(cmds);
                    }
                )*
                meta.commands = all_commands;
            }
        }
    } else {
        quote! {}
    };

    let infer_state_var_modules = quote! {
        {
            let root_module = #root_module_name.to_string();
            let mut field_module: std::collections::HashMap<String, String> = std::collections::HashMap::new();
            for mutation in &meta.mutations {
                if let Some(ref module) = mutation.source_module {
                    for field in &mutation.modifies {
                        let entry = field_module.entry(field.clone()).or_insert(module.clone());
                        // Sub-module wins over root module
                        if *entry == root_module && *module != root_module {
                            *entry = module.clone();
                        }
                    }
                }
            }
            // Assign state vars: prefer the module whose name matches the field
            // (e.g., "customers" → "customer", "standing_orders" → "standing_order"),
            // then fall back to the inferred module from action modifies.
            let module_names: Vec<String> = meta.modules.iter().map(|m| m.name.clone()).collect();
            for sv in &mut meta.state_vars {
                // Check for singular/plural name match against declared modules
                let name_match = module_names.iter().find(|m| {
                    sv.name == **m
                        || sv.name == format!("{}s", m)
                        || sv.name == format!("{}es", m)
                        || sv.name.strip_suffix("ies").map(|s| format!("{}y", s)) == Some(m.to_string())
                });
                sv.source_module = name_match.cloned()
                    .or_else(|| field_module.get(&sv.name).cloned())
                    .or(Some(root_module.clone()));
            }
        }
    };

    let expanded = quote! {
        pub fn __model_metadata() -> bluetext_model::metadata::StateMachineMeta {
            use bluetext_model::metadata::StateMachineModel;
            let mut meta = <#sm as StateMachineModel>::metadata();
            meta.modules = vec![#(#module_entries),*];
            meta.types = vec![#(#type_entries),*];
            #collect_commands
            #infer_state_var_modules
            meta
        }

        #[tokio::main(flavor = "current_thread")]
        pub async fn main() {
            bluetext_model::entry::model_main::<#sm, _, _>(
                __model_metadata,
                #state_factory_expr,
            ).await;
        }
    };

    TokenStream::from(expanded)
}

fn pascal_to_snake(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            result.push(c.to_lowercase().next().unwrap());
        } else {
            result.push(c);
        }
    }
    result
}
