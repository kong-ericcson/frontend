#[cfg(test)]
mod tests {
    use crate::simulation::*;
    use crate::metadata::*;

    #[test]
    fn test_simulation_config_serialization() {
        let config = SimulationConfig {
            max_samples: 100,
            max_steps: 20,
            seed: Some(42),
            init_name: "init".to_string(),
            step_name: "step".to_string(),
            state_invariants: vec!["no_negative".to_string()],
        };
        let json = serde_json::to_string(&config).unwrap();
        let parsed: SimulationConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.max_samples, 100);
        assert_eq!(parsed.seed, Some(42));
    }

    #[test]
    fn test_simulation_result_serialization() {
        let result = SimulationResult {
            status: SimulationStatus::Ok,
            output: "[ok] No violation".to_string(),
            seed: Some(42),
            stats: Some("min=5, max=20, avg=12".to_string()),
            violation: None,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"ok\""));
    }

    #[test]
    fn test_rand_usize() {
        let val = rand_usize(10);
        assert!(val < 10);
    }

    #[test]
    fn test_rand_range() {
        let val = rand_range(5, 10);
        assert!((5..=10).contains(&val));
    }

    #[test]
    fn test_one_of() {
        let items = vec![1, 2, 3];
        let val = one_of(&items).unwrap();
        assert!(items.contains(&val));
    }

    #[test]
    fn test_metadata_types_serialization() {
        let meta = StateMachineMeta {
            module: "Bank".to_string(),
            modules: Vec::new(),
            types: vec![TypeMeta {
                name: "Account".to_string(),
                kind: TypeKind::Record,
                fields: vec![
                    FieldMeta { name: "id".to_string(), type_name: "String".to_string(), fields: vec![], references: None, key_for: None },
                    FieldMeta { name: "balance".to_string(), type_name: "i64".to_string(), fields: vec![], references: None, key_for: None },
                ],
                source_module: None,
                source_line: None,
                source_end_line: None,
            }],
            aliases: vec![AliasMeta {
                name: "AccountId".to_string(),
                target: "String".to_string(),
            }],
            state_vars: vec![StateVarMeta {
                name: "accounts".to_string(),
                type_name: "HashMap<AccountId, Account>".to_string(),
                type_refs: vec!["Account".to_string()],
                source_module: None,
                source_file: None,
                source_line: None,
            }],
            mutations: vec![MutationMeta {
                name: "transfer".to_string(),
                params: vec![
                    FieldMeta { name: "from".to_string(), type_name: "AccountId".to_string(), fields: vec![], references: None, key_for: None },
                    FieldMeta { name: "to".to_string(), type_name: "AccountId".to_string(), fields: vec![], references: None, key_for: None },
                ],
                modifies: vec!["accounts".to_string()],
                reads: vec!["accounts".to_string()],
                source_file: Some("src/bank.rs".to_string()),
                source_line: Some(28),
                source_end_line: Some(42),
                source_module: None,
            }],
            getters: vec![],
            commands: vec![],
            requests: vec![],
            controllers: vec![],
            access: vec![],
            state_invariants: vec![StateInvariantMeta {
                name: "no_negative_balances".to_string(),
                reads: vec!["accounts".to_string()],
                source_file: None,
                source_line: None,
                source_end_line: None,
                source_module: None,
            }],
            eventual_consistency_constraints: vec![],
            simulation_inits: vec![SimulationInitMeta {
                name: "init".to_string(),
                source_file: None,
                source_line: None,
                source_end_line: None,
                source_module: None,
            }],
            simulation_steps: vec![SimulationStepMeta {
                name: "step".to_string(),
                source_file: None,
                source_line: None,
                source_end_line: None,
                source_module: None,
            }],
        };

        let json = serde_json::to_string_pretty(&meta).unwrap();
        assert!(json.contains("\"Bank\""));
        assert!(json.contains("\"Account\""));
        assert!(json.contains("\"transfer\""));
        assert!(json.contains("\"no_negative_balances\""));

        // Verify snake_case serialization
        assert!(json.contains("\"state_vars\""));
        assert!(json.contains("\"type_refs\""));
        assert!(json.contains("\"source_file\""));
        assert!(json.contains("\"mutations\""));
        assert!(json.contains("\"simulation_inits\""));
        assert!(json.contains("\"simulation_steps\""));

        // Roundtrip
        let parsed: StateMachineMeta = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.module, "Bank");
        assert_eq!(parsed.types.len(), 1);
        assert_eq!(parsed.mutations.len(), 1);
    }

    #[test]
    fn test_workflow_duration_parsing() {
        use crate::workflow::parse_duration;
        assert_eq!(parse_duration("30 days").as_secs(), 30 * 86400);
        assert_eq!(parse_duration("5 minutes").as_secs(), 300);
        assert_eq!(parse_duration("1 hour").as_secs(), 3600);
        assert_eq!(parse_duration("10 seconds").as_secs(), 10);
        assert_eq!(parse_duration("2 weeks").as_secs(), 2 * 604800);
    }
}
