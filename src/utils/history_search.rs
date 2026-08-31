use crate::utils::history::{redact_command, HistoryEntry};
use chrono::{DateTime, Utc};

#[derive(Debug, Default)]
pub struct HistoryQuery<'a> {
    pub command: Option<&'a str>,
    pub network: Option<&'a str>,
    pub correlation_id: Option<&'a str>,
    pub from: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
}

pub fn search_history(entries: &[HistoryEntry], query: &HistoryQuery<'_>) -> Vec<HistoryEntry> {
    entries
        .iter()
        .filter(|entry| {
            query
                .command
                .map_or(true, |term| entry.command.contains(term))
        })
        .filter(|entry| {
            query.network.map_or(true, |network| {
                has_value(&entry.command, "--network", network)
            })
        })
        .filter(|entry| {
            query
                .correlation_id
                .map_or(true, |id| entry.command.contains(id))
        })
        .filter(|entry| query.from.map_or(true, |from| entry.timestamp >= from))
        .filter(|entry| query.until.map_or(true, |until| entry.timestamp <= until))
        .map(|entry| HistoryEntry {
            command: redact_command(&entry.command),
            ..entry.clone()
        })
        .collect()
}

fn has_value(command: &str, flag: &str, expected: &str) -> bool {
    let tokens: Vec<&str> = command.split_whitespace().collect();
    tokens
        .windows(2)
        .any(|pair| pair[0] == flag && pair[1] == expected)
        || tokens
            .iter()
            .any(|token| *token == format!("{}={}", flag, expected))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn entry(command: &str, timestamp: DateTime<Utc>) -> HistoryEntry {
        HistoryEntry {
            command: command.to_string(),
            timestamp,
            count: 1,
            last_used: timestamp,
        }
    }

    #[test]
    fn filters_by_command_network_correlation_and_time() {
        let time = Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap();
        let entries = vec![entry(
            "deploy --network testnet --correlation-id incident-7",
            time,
        )];
        let result = search_history(
            &entries,
            &HistoryQuery {
                command: Some("deploy"),
                network: Some("testnet"),
                correlation_id: Some("incident-7"),
                from: Some(time),
                until: Some(time),
            },
        );
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn redacts_secrets_in_search_results() {
        let time = Utc::now();
        let result = search_history(
            &[entry("invoke --token private", time)],
            &HistoryQuery::default(),
        );
        assert!(!result[0].command.contains("private"));
    }
}
