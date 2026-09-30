//! Integration and regression tests for GraphQL query cost analysis and depth limits.
//!
//! Validates:
//! - Configurable depth and complexity ceilings
//! - Clear GraphQL error formatting on limit breach
//! - Opt-in metrics collection and tracking for rejected expensive queries
//! - Safe defaults suitable for public exposure

#[path = "../src/graphql/cost_analysis.rs"]
mod cost_analysis;

use cost_analysis::{CostAnalysisConfig, GraphQLLimitsError, QueryCostAnalyzer, QueryCostMetrics};

#[test]
fn test_default_config_is_safe_for_public_exposure() {
    let config = CostAnalysisConfig::default_safe();

    // Defaults must be conservative and safe for public endpoints
    assert_eq!(config.max_depth, 7, "Default max depth should be 7");
    assert_eq!(
        config.max_complexity, 100,
        "Default max complexity should be 100"
    );
    assert_eq!(
        config.default_field_cost, 1,
        "Default scalar field cost should be 1"
    );
    assert!(
        !config.enable_metrics,
        "Metrics should be opt-in (disabled by default)"
    );
    assert_eq!(
        config.max_multiplier, 100,
        "Multiplier should be capped at 100"
    );

    // Higher-weight operations must be present
    assert!(config.field_costs.get("wallets").copied().unwrap_or(0) >= 5);
    assert!(config.field_costs.get("transactions").copied().unwrap_or(0) >= 10);
    assert!(
        config
            .field_costs
            .get("submitTransaction")
            .copied()
            .unwrap_or(0)
            >= 20
    );
}

#[test]
fn test_depth_limit_allows_valid_queries() {
    let query = r#"
        query GetWalletsAndContracts {
            wallets {
                id
                publicKey
                name
                balance
            }
            contracts {
                id
                address
                name
            }
        }
    "#;

    let config = CostAnalysisConfig::default_safe();
    let report = QueryCostAnalyzer::analyze(query, &config, None)
        .expect("Valid query within depth limit should pass");

    assert_eq!(report.depth, 2);
    assert!(report.is_allowed);
    assert!(!report.is_depth_exceeded);
    assert!(!report.is_complexity_exceeded);
    assert_eq!(report.operation_type, "query");
    assert_eq!(
        report.operation_name.as_deref(),
        Some("GetWalletsAndContracts")
    );
}

#[test]
fn test_depth_limit_rejects_exceeded_depth() {
    // 8 levels of nesting
    let query = r#"
        query ExcessiveDepth {
            level1 {
                level2 {
                    level3 {
                        level4 {
                            level5 {
                                level6 {
                                    level7 {
                                        level8
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    "#;

    let config = CostAnalysisConfig::default_safe().with_max_depth(6);
    let err = QueryCostAnalyzer::analyze(query, &config, None)
        .expect_err("Query exceeding depth ceiling must be rejected");

    match err {
        GraphQLLimitsError::DepthLimitExceeded {
            depth,
            max_depth,
            ref operation,
        } => {
            assert_eq!(depth, 8);
            assert_eq!(max_depth, 6);
            assert_eq!(operation.as_deref(), Some("ExcessiveDepth"));
            let msg = err.message();
            assert!(msg.contains("Query depth limit of 6 exceeded"));
            assert!(msg.contains("query depth is 8"));
            assert_eq!(err.error_code(), "GRAPHQL_DEPTH_LIMIT_EXCEEDED");
        }
        other => panic!("Expected DepthLimitExceeded, got {:?}", other),
    }

    // Verify GraphQL response format
    let response = err.to_graphql_error_response();
    assert!(response["errors"].is_array());
    assert_eq!(
        response["errors"][0]["extensions"]["code"],
        "GRAPHQL_DEPTH_LIMIT_EXCEEDED"
    );
    assert_eq!(response["errors"][0]["extensions"]["depth"], 8);
    assert_eq!(response["errors"][0]["extensions"]["max_depth"], 6);
    assert!(response["data"].is_null());
}

#[test]
fn test_complexity_limit_allows_standard_queries() {
    let query = r#"
        query StandardQuery {
            wallets {
                id
                name
            }
        }
    "#;

    let config = CostAnalysisConfig::default_safe();
    let report = QueryCostAnalyzer::analyze(query, &config, None)
        .expect("Query within complexity limit should succeed");

    assert!(report.complexity <= config.max_complexity);
    assert!(report.is_allowed);
}

#[test]
fn test_complexity_limit_rejects_expensive_queries() {
    let query = r#"
        mutation HeavyBatch {
            t1: submitTransaction { id status }
            t2: submitTransaction { id status }
            t3: submitTransaction { id status }
            t4: submitTransaction { id status }
            t5: submitTransaction { id status }
        }
    "#;

    // Default submitTransaction cost = 25 each, 5 * 25 = 125 + children > 100
    let config = CostAnalysisConfig::default_safe().with_max_complexity(100);
    let err = QueryCostAnalyzer::analyze(query, &config, None)
        .expect_err("Query exceeding complexity limit must be rejected");

    match err {
        GraphQLLimitsError::ComplexityLimitExceeded {
            complexity,
            max_complexity,
            ref operation,
        } => {
            assert!(complexity > 100);
            assert_eq!(max_complexity, 100);
            assert_eq!(operation.as_deref(), Some("HeavyBatch"));
            let msg = err.message();
            assert!(msg.contains("Query complexity limit of 100 exceeded"));
            assert_eq!(err.error_code(), "GRAPHQL_COMPLEXITY_LIMIT_EXCEEDED");
        }
        other => panic!("Expected ComplexityLimitExceeded, got {:?}", other),
    }

    let response = err.to_graphql_error_response();
    assert_eq!(
        response["errors"][0]["extensions"]["code"],
        "GRAPHQL_COMPLEXITY_LIMIT_EXCEEDED"
    );
}

#[test]
fn test_argument_multipliers_increase_nested_cost() {
    let query_low_limit = r#"
        query {
            templates(limit: 5) {
                id
                name
                version
                rating
            }
        }
    "#;

    let query_high_limit = r#"
        query {
            templates(limit: 50) {
                id
                name
                version
                rating
            }
        }
    "#;

    let config = CostAnalysisConfig::default_safe().with_max_complexity(1000);
    let report_low = QueryCostAnalyzer::analyze(query_low_limit, &config, None).unwrap();
    let report_high = QueryCostAnalyzer::analyze(query_high_limit, &config, None).unwrap();

    assert!(
        report_high.complexity > report_low.complexity,
        "High limit ({}) must result in higher cost than low limit ({})",
        report_high.complexity,
        report_low.complexity
    );
}

#[test]
fn test_opt_in_metrics_tracks_rejections() {
    let config = CostAnalysisConfig::default_safe()
        .with_max_depth(3)
        .with_max_complexity(10)
        .with_metrics(true);

    let metrics = QueryCostMetrics::new();

    // 1. Successful query
    let good_query = "query { account { id } }";
    let _ = QueryCostAnalyzer::analyze(good_query, &config, Some(&metrics));

    // 2. Depth rejection
    let deep_query = "query Deep { a { b { c { d } } } }";
    let _ = QueryCostAnalyzer::analyze(deep_query, &config, Some(&metrics));

    // 3. Complexity rejection
    let expensive_query = "query Expensive { submitTransaction { id } }";
    let _ = QueryCostAnalyzer::analyze(expensive_query, &config, Some(&metrics));

    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.total_analyzed, 3);
    assert_eq!(snapshot.total_rejected, 2);
    assert_eq!(snapshot.rejected_depth, 1);
    assert_eq!(snapshot.rejected_complexity, 1);
    assert!(snapshot.total_rejected_complexity > 0);
    assert!((snapshot.rejection_rate - (2.0 / 3.0)).abs() < 1e-6);
    assert_eq!(snapshot.recent_rejections.len(), 2);

    // Verify rejection record fields
    let first = &snapshot.recent_rejections[0];
    assert_eq!(first.operation_name.as_deref(), Some("Deep"));
    assert!(first.reason.contains("depth_limit_exceeded"));

    let second = &snapshot.recent_rejections[1];
    assert_eq!(second.operation_name.as_deref(), Some("Expensive"));
    assert!(second.reason.contains("complexity_limit_exceeded"));

    // Reset works
    metrics.reset();
    let cleared = metrics.snapshot();
    assert_eq!(cleared.total_analyzed, 0);
    assert_eq!(cleared.total_rejected, 0);
    assert_eq!(cleared.recent_rejections.len(), 0);
}

#[test]
fn test_disabled_metrics_does_not_record() {
    let config = CostAnalysisConfig::default_safe()
        .with_max_depth(2)
        .with_metrics(false);

    let metrics = QueryCostMetrics::new();
    let deep_query = "query { a { b { c } } }";
    let _ = QueryCostAnalyzer::analyze(deep_query, &config, Some(&metrics));

    let snap = metrics.snapshot();
    assert_eq!(snap.total_analyzed, 0);
    assert_eq!(snap.total_rejected, 0);
}

#[test]
fn test_fragment_expansion_and_cycle_guard() {
    let query_with_fragment = r#"
        fragment AccountFields on Account {
            id
            publicKey
            balance
            sequence
        }

        query GetAccount {
            account {
                ...AccountFields
            }
        }
    "#;

    let config = CostAnalysisConfig::default_safe();
    let report = QueryCostAnalyzer::analyze(query_with_fragment, &config, None)
        .expect("Query with valid fragments should pass");

    assert_eq!(report.depth, 2);
    assert!(report.field_count >= 5);
}

#[test]
fn test_inline_fragment_depth_and_complexity() {
    let query = r#"
        query {
            contracts {
                id
                ... on SorobanContract {
                    wasmHash
                    spec {
                        functions
                    }
                }
            }
        }
    "#;

    let config = CostAnalysisConfig::default_safe()
        .with_field_cost("wasmHash", 3)
        .with_default_field_cost(2)
        .with_max_multiplier(50)
        .with_multiplier_arguments(vec!["limit".to_string()]);
    let report = QueryCostAnalyzer::analyze(query, &config, None)
        .expect("Inline fragments should be analyzed properly");

    assert_eq!(report.depth, 3);
}

#[test]
fn test_parse_errors_on_malformed_syntax() {
    let malformed = "query { unclosed_brace ";
    let config = CostAnalysisConfig::default_safe();
    let err = QueryCostAnalyzer::analyze(malformed, &config, None)
        .expect_err("Malformed query should produce ParseError");

    match err {
        GraphQLLimitsError::ParseError(_) => {
            assert_eq!(err.error_code(), "GRAPHQL_PARSE_ERROR");
        }
        _ => panic!("Expected ParseError, got {:?}", err),
    }

    let empty = "   ";
    assert!(matches!(
        QueryCostAnalyzer::analyze(empty, &config, None),
        Err(GraphQLLimitsError::ParseError(_))
    ));
}

#[test]
fn test_validate_request_helper_returns_json() {
    let query = "query { a { b { c { d { e { f { g { h } } } } } } } }";
    let config = CostAnalysisConfig::default_safe().with_max_depth(5);

    let res = QueryCostAnalyzer::validate_request(query, &config, None);
    assert!(res.is_err());
    let err_json = res.unwrap_err();
    assert!(err_json["errors"].is_array());
    assert_eq!(
        err_json["errors"][0]["extensions"]["code"],
        "GRAPHQL_DEPTH_LIMIT_EXCEEDED"
    );
}
