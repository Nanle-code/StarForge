//! Integration tests for real-time contract event streaming (D-10):
//! filtering, routing, alert patterns, analytics, and replay.

use starforge::utils::event_stream as es;

fn record(
    id: &str,
    etype: &str,
    ledger: u32,
    topics: &[&str],
    value: &str,
    ts: &str,
) -> es::EventRecord {
    es::EventRecord {
        id: id.to_string(),
        contract_id: "CCONTRACT".to_string(),
        event_type: etype.to_string(),
        ledger,
        topics: topics.iter().map(|s| s.to_string()).collect(),
        value: value.to_string(),
        received_at: ts.to_string(),
    }
}

fn epoch(rfc3339: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .unwrap()
        .timestamp()
}

#[test]
fn filtering_routing_alerting_and_analytics_pipeline() {
    let events = vec![
        record(
            "e1",
            "contract",
            100,
            &["transfer", "alice"],
            "amount=50",
            "2026-02-01T00:00:00+00:00",
        ),
        record(
            "e2",
            "contract",
            101,
            &["transfer", "bob"],
            "amount=75",
            "2026-02-01T00:00:10+00:00",
        ),
        record(
            "e3",
            "contract",
            102,
            &["error", "overflow"],
            "panic",
            "2026-02-01T00:00:20+00:00",
        ),
        record(
            "e4",
            "contract",
            103,
            &["error", "auth"],
            "denied",
            "2026-02-01T00:00:25+00:00",
        ),
        record(
            "e5",
            "system",
            103,
            &["fee"],
            "100",
            "2026-02-01T00:00:30+00:00",
        ),
    ];

    // Filtering: a transfer-topic filter selects exactly the two transfers.
    let transfer_filter = es::EventFilter {
        topic_pattern: Some("transfer,*".into()),
        ..Default::default()
    };
    let transfers: Vec<_> = events
        .iter()
        .filter(|e| transfer_filter.matches(e))
        .collect();
    assert_eq!(transfers.len(), 2);

    // Routing: a log route for errors matches the two error events.
    let routes = vec![es::EventRoute {
        name: "errors".into(),
        filter: es::EventFilter {
            topic_pattern: Some("error".into()),
            ..Default::default()
        },
        action: es::RouteAction::Log,
        enabled: true,
    }];
    let error_matches: usize = events
        .iter()
        .map(|e| es::match_routes(e, &routes).len())
        .sum();
    assert_eq!(error_matches, 2);

    // Alerting: ≥2 error events within a 60s window fires.
    let patterns = vec![es::AlertPattern {
        name: "error-spike".into(),
        filter: es::EventFilter {
            topic_pattern: Some("error".into()),
            ..Default::default()
        },
        window_secs: 60,
        threshold: 2,
        severity: "critical".into(),
        enabled: true,
    }];
    let now = epoch("2026-02-01T00:00:40+00:00");
    let alerts = es::evaluate_alerts(&patterns, &events, now);
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].count, 2);
    assert_eq!(alerts[0].severity, "critical");

    // Analytics: aggregate counts by type and ledger range.
    let analytics = es::EventAnalytics::from_events(&events);
    assert_eq!(analytics.total, 5);
    assert_eq!(analytics.by_type["contract"], 4);
    assert_eq!(analytics.by_type["system"], 1);
    assert_eq!(analytics.first_ledger, Some(100));
    assert_eq!(analytics.last_ledger, Some(103));

    // Replay: end-to-end summary over history.
    let summary = es::replay(&events, &routes, &patterns, now);
    assert_eq!(summary.processed, 5);
    assert_eq!(summary.matched_routes, 2);
    assert_eq!(summary.alerts.len(), 1);
}

#[test]
fn json_roundtrip_for_routes_and_patterns() {
    let route = es::EventRoute {
        name: "webhook-transfers".into(),
        filter: es::EventFilter {
            topic_pattern: Some("transfer".into()),
            ..Default::default()
        },
        action: es::RouteAction::Webhook {
            url: "https://example.com/hook".into(),
        },
        enabled: true,
    };
    let json = serde_json::to_string(&route).unwrap();
    let back: es::EventRoute = serde_json::from_str(&json).unwrap();
    assert_eq!(route, back);

    let pattern = es::AlertPattern {
        name: "spike".into(),
        filter: es::EventFilter::default(),
        window_secs: 120,
        threshold: 10,
        severity: "warning".into(),
        enabled: true,
    };
    let json = serde_json::to_string(&pattern).unwrap();
    let back: es::AlertPattern = serde_json::from_str(&json).unwrap();
    assert_eq!(pattern, back);
}
