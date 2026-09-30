//! GraphQL query cost analysis and depth limiting.
//!
//! Provides defense-in-depth protection against unbounded or maliciously deep
//! queries that threaten API availability. Enforces configurable maximum query
//! depth and complexity ceilings, with clear GraphQL errors on limit breach and
//! opt-in rejection metrics.

#[cfg(test)]
extern crate serde_derive;
#[cfg(not(test))]
use serde::{Deserialize, Serialize};
#[cfg(test)]
use serde_derive::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, RwLock};

/// Configuration for GraphQL query depth and complexity ceilings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostAnalysisConfig {
    /// Maximum allowed selection nesting depth.
    ///
    /// Root fields are depth 1; each nested field selection increases depth by 1.
    /// Default safe limit for public exposure is 7.
    pub max_depth: usize,

    /// Maximum allowed total query complexity / cost score.
    ///
    /// Default safe limit for public exposure is 100.
    pub max_complexity: usize,

    /// Default cost assigned to a scalar or object field when no explicit override exists.
    /// Default is 1.
    pub default_field_cost: usize,

    /// Explicit cost overrides per field name.
    ///
    /// For example, expensive database queries or pagination roots can be assigned
    /// higher weights (e.g. "wallets" -> 5, "transactions" -> 10).
    pub field_costs: HashMap<String, usize>,

    /// Argument names that act as multipliers for child selection sets
    /// (e.g. "limit", "first", "take", "count", "size").
    pub multiplier_arguments: Vec<String>,

    /// Maximum multiplier allowed from arguments to prevent DOS via integer overflow.
    /// Default is 100.
    pub max_multiplier: usize,

    /// Whether to track metrics for rejected expensive queries (opt-in).
    pub enable_metrics: bool,
}

impl Default for CostAnalysisConfig {
    fn default() -> Self {
        Self::default_safe()
    }
}

impl CostAnalysisConfig {
    /// Safe defaults suitable for public API exposure.
    ///
    /// - Max depth: 7
    /// - Max complexity: 100
    /// - Default field cost: 1
    /// - Known multiplier arguments: "limit", "first", "take", "count", "size"
    /// - Max multiplier: 100
    /// - Metrics: disabled (opt-in)
    pub fn default_safe() -> Self {
        let mut field_costs = HashMap::new();
        // Common higher-cost operations in StarForge
        field_costs.insert("wallets".to_string(), 5);
        field_costs.insert("contracts".to_string(), 5);
        field_costs.insert("templates".to_string(), 5);
        field_costs.insert("transactions".to_string(), 10);
        field_costs.insert("account".to_string(), 5);
        field_costs.insert("submitTransaction".to_string(), 25);
        field_costs.insert("deployContract".to_string(), 30);
        field_costs.insert("invokeContract".to_string(), 20);

        Self {
            max_depth: 7,
            max_complexity: 100,
            default_field_cost: 1,
            field_costs,
            multiplier_arguments: vec![
                "limit".to_string(),
                "first".to_string(),
                "take".to_string(),
                "count".to_string(),
                "size".to_string(),
            ],
            max_multiplier: 100,
            enable_metrics: false,
        }
    }

    /// Builder: set maximum query depth.
    pub fn with_max_depth(mut self, depth: usize) -> Self {
        self.max_depth = depth;
        self
    }

    /// Builder: set maximum query complexity.
    pub fn with_max_complexity(mut self, complexity: usize) -> Self {
        self.max_complexity = complexity;
        self
    }

    /// Builder: set default cost per field.
    pub fn with_default_field_cost(mut self, cost: usize) -> Self {
        self.default_field_cost = cost;
        self
    }

    /// Builder: add or override cost for a specific field name.
    pub fn with_field_cost(mut self, field_name: impl Into<String>, cost: usize) -> Self {
        self.field_costs.insert(field_name.into(), cost);
        self
    }

    /// Builder: configure multiplier arguments.
    pub fn with_multiplier_arguments(mut self, args: Vec<String>) -> Self {
        self.multiplier_arguments = args;
        self
    }

    /// Builder: configure maximum multiplier cap.
    pub fn with_max_multiplier(mut self, max: usize) -> Self {
        self.max_multiplier = max;
        self
    }

    /// Builder: enable or disable opt-in rejection metrics.
    pub fn with_metrics(mut self, enable: bool) -> Self {
        self.enable_metrics = enable;
        self
    }
}

/// Detailed cost breakdown for a specific top-level field.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FieldCostDetail {
    pub field_name: String,
    pub depth: usize,
    pub cost: usize,
    pub multiplier: usize,
}

/// Analysis report summarizing query depth, complexity, and whether limits were breached.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QueryCostReport {
    /// Name of the operation, if provided in the document.
    pub operation_name: Option<String>,
    /// Operation type: "query", "mutation", or "subscription".
    pub operation_type: String,
    /// Maximum nesting depth found in this query.
    pub depth: usize,
    /// Total calculated complexity cost score.
    pub complexity: usize,
    /// Allowed depth ceiling configured at time of analysis.
    pub max_depth_allowed: usize,
    /// Allowed complexity ceiling configured at time of analysis.
    pub max_complexity_allowed: usize,
    /// Total number of field selections analyzed.
    pub field_count: usize,
    /// Whether depth limit was exceeded.
    pub is_depth_exceeded: bool,
    /// Whether complexity ceiling was exceeded.
    pub is_complexity_exceeded: bool,
    /// Whether the query is within all configured limits.
    pub is_allowed: bool,
    /// Cost breakdown of top-level fields.
    pub field_breakdown: Vec<FieldCostDetail>,
}

/// GraphQL limit error raised when a query breaches depth or complexity ceilings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphQLLimitsError {
    /// Depth ceiling exceeded.
    DepthLimitExceeded {
        depth: usize,
        max_depth: usize,
        operation: Option<String>,
    },
    /// Complexity ceiling exceeded.
    ComplexityLimitExceeded {
        complexity: usize,
        max_complexity: usize,
        operation: Option<String>,
    },
    /// Syntax or structure parse error in the query.
    ParseError(String),
}

impl GraphQLLimitsError {
    /// Human-readable error message following GraphQL error conventions.
    pub fn message(&self) -> String {
        match self {
            Self::DepthLimitExceeded {
                depth,
                max_depth,
                operation,
            } => {
                if let Some(op) = operation {
                    format!(
                        "Query depth limit of {} exceeded for operation '{}': query depth is {}",
                        max_depth, op, depth
                    )
                } else {
                    format!(
                        "Query depth limit of {} exceeded: query depth is {}",
                        max_depth, depth
                    )
                }
            }
            Self::ComplexityLimitExceeded {
                complexity,
                max_complexity,
                operation,
            } => {
                if let Some(op) = operation {
                    format!(
                        "Query complexity limit of {} exceeded for operation '{}': calculated complexity is {}",
                        max_complexity, op, complexity
                    )
                } else {
                    format!(
                        "Query complexity limit of {} exceeded: calculated complexity is {}",
                        max_complexity, complexity
                    )
                }
            }
            Self::ParseError(msg) => format!("GraphQL query parse error: {}", msg),
        }
    }

    /// Machine-readable error code.
    pub fn error_code(&self) -> &'static str {
        match self {
            Self::DepthLimitExceeded { .. } => "GRAPHQL_DEPTH_LIMIT_EXCEEDED",
            Self::ComplexityLimitExceeded { .. } => "GRAPHQL_COMPLEXITY_LIMIT_EXCEEDED",
            Self::ParseError(_) => "GRAPHQL_PARSE_ERROR",
        }
    }

    /// Error extensions map for standard GraphQL response.
    pub fn extensions(&self) -> serde_json::Value {
        match self {
            Self::DepthLimitExceeded {
                depth,
                max_depth,
                operation,
            } => serde_json::json!({
                "code": self.error_code(),
                "depth": depth,
                "max_depth": max_depth,
                "operation": operation,
            }),
            Self::ComplexityLimitExceeded {
                complexity,
                max_complexity,
                operation,
            } => serde_json::json!({
                "code": self.error_code(),
                "complexity": complexity,
                "max_complexity": max_complexity,
                "operation": operation,
            }),
            Self::ParseError(detail) => serde_json::json!({
                "code": self.error_code(),
                "details": detail,
            }),
        }
    }

    /// Complete GraphQL error payload response matching the GraphQL specification.
    pub fn to_graphql_error_response(&self) -> serde_json::Value {
        serde_json::json!({
            "errors": [
                {
                    "message": self.message(),
                    "extensions": self.extensions(),
                }
            ],
            "data": serde_json::Value::Null
        })
    }
}

impl std::fmt::Display for GraphQLLimitsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message())
    }
}

impl std::error::Error for GraphQLLimitsError {}

/// Record of an individual rejected query for auditing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RejectedQueryRecord {
    pub timestamp_epoch_ms: u64,
    pub operation_name: Option<String>,
    pub reason: String,
    pub depth: usize,
    pub max_depth: usize,
    pub complexity: usize,
    pub max_complexity: usize,
    pub query_snippet: String,
}

#[derive(Debug, Default)]
struct MetricsInner {
    total_analyzed: u64,
    total_rejected: u64,
    rejected_depth: u64,
    rejected_complexity: u64,
    total_rejected_complexity: u64,
    recent_rejections: VecDeque<RejectedQueryRecord>,
}

/// Thread-safe opt-in metrics store for expensive / rejected GraphQL queries.
#[derive(Debug, Clone)]
pub struct QueryCostMetrics {
    inner: Arc<RwLock<MetricsInner>>,
    max_recent_records: usize,
}

/// Point-in-time snapshot of GraphQL rejection metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    pub total_analyzed: u64,
    pub total_rejected: u64,
    pub rejected_depth: u64,
    pub rejected_complexity: u64,
    pub total_rejected_complexity: u64,
    pub rejection_rate: f64,
    pub recent_rejections: Vec<RejectedQueryRecord>,
}

impl Default for QueryCostMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl QueryCostMetrics {
    /// Create a new empty metrics store retaining up to 100 recent rejections.
    pub fn new() -> Self {
        Self::with_history_limit(100)
    }

    /// Create with a custom history retention limit.
    pub fn with_history_limit(limit: usize) -> Self {
        Self {
            inner: Arc::new(RwLock::new(MetricsInner::default())),
            max_recent_records: limit,
        }
    }

    /// Record a query analysis result.
    pub fn record(&self, query: &str, report: &QueryCostReport) {
        if let Ok(mut inner) = self.inner.write() {
            inner.total_analyzed += 1;

            if !report.is_allowed {
                inner.total_rejected += 1;
                inner.total_rejected_complexity += report.complexity as u64;

                let mut reason = String::new();
                if report.is_depth_exceeded {
                    inner.rejected_depth += 1;
                    reason.push_str("depth_limit_exceeded");
                }
                if report.is_complexity_exceeded {
                    inner.rejected_complexity += 1;
                    if !reason.is_empty() {
                        reason.push_str(", ");
                    }
                    reason.push_str("complexity_limit_exceeded");
                }

                let snippet = if query.len() > 120 {
                    format!("{}...", &query[..120].replace('\n', " "))
                } else {
                    query.replace('\n', " ")
                };

                let record = RejectedQueryRecord {
                    timestamp_epoch_ms: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0),
                    operation_name: report.operation_name.clone(),
                    reason,
                    depth: report.depth,
                    max_depth: report.max_depth_allowed,
                    complexity: report.complexity,
                    max_complexity: report.max_complexity_allowed,
                    query_snippet: snippet,
                };

                if inner.recent_rejections.len() >= self.max_recent_records {
                    inner.recent_rejections.pop_front();
                }
                inner.recent_rejections.push_back(record);
            }
        }
    }

    /// Obtain a read-only snapshot of current metrics.
    pub fn snapshot(&self) -> MetricsSnapshot {
        if let Ok(inner) = self.inner.read() {
            let rate = if inner.total_analyzed > 0 {
                inner.total_rejected as f64 / inner.total_analyzed as f64
            } else {
                0.0
            };
            MetricsSnapshot {
                total_analyzed: inner.total_analyzed,
                total_rejected: inner.total_rejected,
                rejected_depth: inner.rejected_depth,
                rejected_complexity: inner.rejected_complexity,
                total_rejected_complexity: inner.total_rejected_complexity,
                rejection_rate: rate,
                recent_rejections: inner.recent_rejections.iter().cloned().collect(),
            }
        } else {
            MetricsSnapshot {
                total_analyzed: 0,
                total_rejected: 0,
                rejected_depth: 0,
                rejected_complexity: 0,
                total_rejected_complexity: 0,
                rejection_rate: 0.0,
                recent_rejections: Vec::new(),
            }
        }
    }

    /// Reset all metric counters and clear history.
    pub fn reset(&self) {
        if let Ok(mut inner) = self.inner.write() {
            *inner = MetricsInner::default();
        }
    }
}

// -----------------------------------------------------------------------------
// GraphQL Query AST & Recursive Descent Parser
// -----------------------------------------------------------------------------

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
enum TokenKind {
    Name(String),
    Int(i64),
    Float(f64),
    StringLit(String),
    Punct(char), // '{', '}', '(', ')', ':', '$', '=', '@', '!'
    Spread,      // '...'
    Eof,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct Token {
    kind: TokenKind,
    pos: usize,
}

struct Lexer<'a> {
    input: &'a str,
    chars: Vec<(usize, char)>,
    idx: usize,
}

impl<'a> Lexer<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input,
            chars: input.char_indices().collect(),
            idx: 0,
        }
    }

    fn peek_char(&self) -> Option<char> {
        self.chars.get(self.idx).map(|&(_, c)| c)
    }

    fn next_char(&mut self) -> Option<(usize, char)> {
        if self.idx < self.chars.len() {
            let item = self.chars[self.idx];
            self.idx += 1;
            Some(item)
        } else {
            None
        }
    }

    fn skip_whitespace_and_comments(&mut self) {
        while let Some(c) = self.peek_char() {
            if c.is_whitespace() || c == ',' {
                self.next_char();
            } else if c == '#' {
                // Comment until end of line
                while let Some((_, ch)) = self.next_char() {
                    if ch == '\n' || ch == '\r' {
                        break;
                    }
                }
            } else {
                break;
            }
        }
    }

    fn next_token(&mut self) -> Result<Token, GraphQLLimitsError> {
        self.skip_whitespace_and_comments();

        let (pos, c) = match self.next_char() {
            Some(pair) => pair,
            None => {
                return Ok(Token {
                    kind: TokenKind::Eof,
                    pos: self.input.len(),
                })
            }
        };

        match c {
            '{' | '}' | '(' | ')' | ':' | '$' | '=' | '@' | '!' | '[' | ']' => Ok(Token {
                kind: TokenKind::Punct(c),
                pos,
            }),
            '.' => {
                if self.peek_char() == Some('.') {
                    self.next_char();
                    if self.peek_char() == Some('.') {
                        self.next_char();
                        return Ok(Token {
                            kind: TokenKind::Spread,
                            pos,
                        });
                    }
                }
                Err(GraphQLLimitsError::ParseError(format!(
                    "Unexpected character '.' at position {}",
                    pos
                )))
            }
            '"' => {
                let mut s = String::new();
                let mut escaped = false;
                while let Some((_, ch)) = self.next_char() {
                    if escaped {
                        match ch {
                            '"' => s.push('"'),
                            '\\' => s.push('\\'),
                            'n' => s.push('\n'),
                            'r' => s.push('\r'),
                            't' => s.push('\t'),
                            _ => s.push(ch),
                        }
                        escaped = false;
                    } else if ch == '\\' {
                        escaped = true;
                    } else if ch == '"' {
                        return Ok(Token {
                            kind: TokenKind::StringLit(s),
                            pos,
                        });
                    } else {
                        s.push(ch);
                    }
                }
                Err(GraphQLLimitsError::ParseError(
                    "Unterminated string literal".to_string(),
                ))
            }
            'a'..='z' | 'A'..='Z' | '_' => {
                let mut name = String::new();
                name.push(c);
                while let Some(ch) = self.peek_char() {
                    if ch.is_alphanumeric() || ch == '_' {
                        self.next_char();
                        name.push(ch);
                    } else {
                        break;
                    }
                }
                Ok(Token {
                    kind: TokenKind::Name(name),
                    pos,
                })
            }
            '-' | '0'..='9' => {
                let mut num_str = String::new();
                num_str.push(c);
                let mut is_float = false;
                while let Some(ch) = self.peek_char() {
                    if ch.is_ascii_digit() {
                        self.next_char();
                        num_str.push(ch);
                    } else if ch == '.' && !is_float {
                        is_float = true;
                        self.next_char();
                        num_str.push(ch);
                    } else {
                        break;
                    }
                }
                if is_float {
                    let val = num_str.parse::<f64>().map_err(|e| {
                        GraphQLLimitsError::ParseError(format!("Invalid float literal: {}", e))
                    })?;
                    Ok(Token {
                        kind: TokenKind::Float(val),
                        pos,
                    })
                } else {
                    let val = num_str.parse::<i64>().map_err(|e| {
                        GraphQLLimitsError::ParseError(format!("Invalid integer literal: {}", e))
                    })?;
                    Ok(Token {
                        kind: TokenKind::Int(val),
                        pos,
                    })
                }
            }
            other => Err(GraphQLLimitsError::ParseError(format!(
                "Unexpected character '{}' at position {}",
                other, pos
            ))),
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct AstField {
    name: String,
    alias: Option<String>,
    arguments: HashMap<String, AstValue>,
    selection_set: Vec<AstSelection>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
enum AstValue {
    Int(i64),
    Float(f64),
    String(String),
    Boolean(bool),
    EnumOrVar(String),
    List(Vec<AstValue>),
    Object(HashMap<String, AstValue>),
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
enum AstSelection {
    Field(AstField),
    FragmentSpread(String),
    InlineFragment {
        type_condition: Option<String>,
        selection_set: Vec<AstSelection>,
    },
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct AstOperation {
    op_type: String, // "query", "mutation", "subscription"
    name: Option<String>,
    selection_set: Vec<AstSelection>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct AstFragment {
    name: String,
    type_condition: String,
    selection_set: Vec<AstSelection>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
struct AstDocument {
    operations: Vec<AstOperation>,
    fragments: HashMap<String, AstFragment>,
}

struct Parser<'a> {
    lexer: Lexer<'a>,
    current_token: Token,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Result<Self, GraphQLLimitsError> {
        let mut lexer = Lexer::new(input);
        let current_token = lexer.next_token()?;
        Ok(Self {
            lexer,
            current_token,
        })
    }

    fn advance(&mut self) -> Result<Token, GraphQLLimitsError> {
        let prev = self.current_token.clone();
        self.current_token = self.lexer.next_token()?;
        Ok(prev)
    }

    fn expect_punct(&mut self, expected: char) -> Result<(), GraphQLLimitsError> {
        match &self.current_token.kind {
            TokenKind::Punct(c) if *c == expected => {
                self.advance()?;
                Ok(())
            }
            _ => Err(GraphQLLimitsError::ParseError(format!(
                "Expected punctuation '{}', found {:?}",
                expected, self.current_token.kind
            ))),
        }
    }

    fn expect_name(&mut self) -> Result<String, GraphQLLimitsError> {
        match &self.current_token.kind {
            TokenKind::Name(n) => {
                let name = n.clone();
                self.advance()?;
                Ok(name)
            }
            _ => Err(GraphQLLimitsError::ParseError(format!(
                "Expected identifier name, found {:?}",
                self.current_token.kind
            ))),
        }
    }

    fn parse_document(&mut self) -> Result<AstDocument, GraphQLLimitsError> {
        let mut doc = AstDocument::default();

        while self.current_token.kind != TokenKind::Eof {
            match &self.current_token.kind {
                TokenKind::Punct('{') => {
                    // Shorthand query
                    let selection_set = self.parse_selection_set()?;
                    doc.operations.push(AstOperation {
                        op_type: "query".to_string(),
                        name: None,
                        selection_set,
                    });
                }
                TokenKind::Name(n) => match n.as_str() {
                    "query" | "mutation" | "subscription" => {
                        let op = self.parse_operation()?;
                        doc.operations.push(op);
                    }
                    "fragment" => {
                        let frag = self.parse_fragment()?;
                        doc.fragments.insert(frag.name.clone(), frag);
                    }
                    other => {
                        return Err(GraphQLLimitsError::ParseError(format!(
                            "Unexpected top-level definition keyword '{}'",
                            other
                        )))
                    }
                },
                _ => {
                    return Err(GraphQLLimitsError::ParseError(format!(
                        "Unexpected token at top level: {:?}",
                        self.current_token.kind
                    )))
                }
            }
        }

        Ok(doc)
    }

    fn parse_operation(&mut self) -> Result<AstOperation, GraphQLLimitsError> {
        let op_type = self.expect_name()?;

        let mut name = None;
        if let TokenKind::Name(_) = self.current_token.kind {
            name = Some(self.expect_name()?);
        }

        // Variable definitions: ($var: Type = Default, ...)
        if self.current_token.kind == TokenKind::Punct('(') {
            self.advance()?;
            while self.current_token.kind != TokenKind::Punct(')')
                && self.current_token.kind != TokenKind::Eof
            {
                self.advance()?;
            }
            self.expect_punct(')')?;
        }

        // Directives: @directive(...)
        self.skip_directives()?;

        let selection_set = self.parse_selection_set()?;

        Ok(AstOperation {
            op_type,
            name,
            selection_set,
        })
    }

    fn parse_fragment(&mut self) -> Result<AstFragment, GraphQLLimitsError> {
        self.expect_name()?; // "fragment"
        let name = self.expect_name()?;

        let on_keyword = self.expect_name()?;
        if on_keyword != "on" {
            return Err(GraphQLLimitsError::ParseError(format!(
                "Expected 'on' in fragment definition, found '{}'",
                on_keyword
            )));
        }

        let type_condition = self.expect_name()?;
        self.skip_directives()?;
        let selection_set = self.parse_selection_set()?;

        Ok(AstFragment {
            name,
            type_condition,
            selection_set,
        })
    }

    fn skip_directives(&mut self) -> Result<(), GraphQLLimitsError> {
        while self.current_token.kind == TokenKind::Punct('@') {
            self.advance()?; // skip '@'
            self.expect_name()?; // directive name
            if self.current_token.kind == TokenKind::Punct('(') {
                self.advance()?;
                let mut depth = 1;
                while depth > 0 && self.current_token.kind != TokenKind::Eof {
                    match self.current_token.kind {
                        TokenKind::Punct('(') => depth += 1,
                        TokenKind::Punct(')') => depth -= 1,
                        _ => {}
                    }
                    self.advance()?;
                }
            }
        }
        Ok(())
    }

    fn parse_selection_set(&mut self) -> Result<Vec<AstSelection>, GraphQLLimitsError> {
        self.expect_punct('{')?;
        let mut selections = Vec::new();

        while self.current_token.kind != TokenKind::Punct('}')
            && self.current_token.kind != TokenKind::Eof
        {
            selections.push(self.parse_selection()?);
        }

        self.expect_punct('}')?;
        Ok(selections)
    }

    fn parse_selection(&mut self) -> Result<AstSelection, GraphQLLimitsError> {
        match &self.current_token.kind {
            TokenKind::Spread => {
                self.advance()?; // skip '...'
                if let TokenKind::Name(n) = &self.current_token.kind {
                    if n == "on" {
                        // Inline fragment: ... on Type { ... }
                        self.advance()?; // skip "on"
                        let type_condition = Some(self.expect_name()?);
                        self.skip_directives()?;
                        let selection_set = self.parse_selection_set()?;
                        Ok(AstSelection::InlineFragment {
                            type_condition,
                            selection_set,
                        })
                    } else {
                        // Named fragment spread: ...FragmentName
                        let frag_name = self.expect_name()?;
                        self.skip_directives()?;
                        Ok(AstSelection::FragmentSpread(frag_name))
                    }
                } else if self.current_token.kind == TokenKind::Punct('{') {
                    // Inline fragment without type condition: ... { ... }
                    let selection_set = self.parse_selection_set()?;
                    Ok(AstSelection::InlineFragment {
                        type_condition: None,
                        selection_set,
                    })
                } else {
                    Err(GraphQLLimitsError::ParseError(
                        "Expected fragment name or inline fragment after '...'".to_string(),
                    ))
                }
            }
            TokenKind::Name(_) => {
                let first_name = self.expect_name()?;
                let (alias, name) = if self.current_token.kind == TokenKind::Punct(':') {
                    self.advance()?; // skip ':'
                    let real_name = self.expect_name()?;
                    (Some(first_name), real_name)
                } else {
                    (None, first_name)
                };

                let mut arguments = HashMap::new();
                if self.current_token.kind == TokenKind::Punct('(') {
                    arguments = self.parse_arguments()?;
                }

                self.skip_directives()?;

                let mut selection_set = Vec::new();
                if self.current_token.kind == TokenKind::Punct('{') {
                    selection_set = self.parse_selection_set()?;
                }

                Ok(AstSelection::Field(AstField {
                    name,
                    alias,
                    arguments,
                    selection_set,
                }))
            }
            _ => Err(GraphQLLimitsError::ParseError(format!(
                "Unexpected token in selection set: {:?}",
                self.current_token.kind
            ))),
        }
    }

    fn parse_arguments(&mut self) -> Result<HashMap<String, AstValue>, GraphQLLimitsError> {
        self.expect_punct('(')?;
        let mut args = HashMap::new();

        while self.current_token.kind != TokenKind::Punct(')')
            && self.current_token.kind != TokenKind::Eof
        {
            let arg_name = self.expect_name()?;
            self.expect_punct(':')?;
            let arg_val = self.parse_value()?;
            args.insert(arg_name, arg_val);
        }

        self.expect_punct(')')?;
        Ok(args)
    }

    fn parse_value(&mut self) -> Result<AstValue, GraphQLLimitsError> {
        match &self.current_token.kind {
            TokenKind::Int(i) => {
                let val = *i;
                self.advance()?;
                Ok(AstValue::Int(val))
            }
            TokenKind::Float(f) => {
                let val = *f;
                self.advance()?;
                Ok(AstValue::Float(val))
            }
            TokenKind::StringLit(s) => {
                let val = s.clone();
                self.advance()?;
                Ok(AstValue::String(val))
            }
            TokenKind::Name(n) => {
                let name = n.clone();
                self.advance()?;
                match name.as_str() {
                    "true" => Ok(AstValue::Boolean(true)),
                    "false" => Ok(AstValue::Boolean(false)),
                    _ => Ok(AstValue::EnumOrVar(name)),
                }
            }
            TokenKind::Punct('$') => {
                self.advance()?; // skip '$'
                let var_name = self.expect_name()?;
                Ok(AstValue::EnumOrVar(format!("${}", var_name)))
            }
            TokenKind::Punct('[') => {
                self.advance()?;
                let mut list = Vec::new();
                while self.current_token.kind != TokenKind::Punct(']')
                    && self.current_token.kind != TokenKind::Eof
                {
                    list.push(self.parse_value()?);
                }
                self.expect_punct(']')?;
                Ok(AstValue::List(list))
            }
            TokenKind::Punct('{') => {
                self.advance()?;
                let mut obj = HashMap::new();
                while self.current_token.kind != TokenKind::Punct('}')
                    && self.current_token.kind != TokenKind::Eof
                {
                    let key = self.expect_name()?;
                    self.expect_punct(':')?;
                    let val = self.parse_value()?;
                    obj.insert(key, val);
                }
                self.expect_punct('}')?;
                Ok(AstValue::Object(obj))
            }
            _ => Err(GraphQLLimitsError::ParseError(format!(
                "Unexpected token for value: {:?}",
                self.current_token.kind
            ))),
        }
    }
}

// -----------------------------------------------------------------------------
// Query Cost Analyzer
// -----------------------------------------------------------------------------

/// The primary engine for analyzing GraphQL query depth and complexity.
pub struct QueryCostAnalyzer;

impl QueryCostAnalyzer {
    /// Analyzes a GraphQL query document against the provided configuration.
    ///
    /// If depth or complexity ceilings are exceeded, returns a `GraphQLLimitsError`.
    /// If `metrics` is provided and `config.enable_metrics` is true, rejections are recorded.
    pub fn analyze(
        query: &str,
        config: &CostAnalysisConfig,
        metrics: Option<&QueryCostMetrics>,
    ) -> Result<QueryCostReport, GraphQLLimitsError> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Err(GraphQLLimitsError::ParseError(
                "Query string cannot be empty".to_string(),
            ));
        }

        let mut parser = Parser::new(trimmed)?;
        let doc = parser.parse_document()?;

        if doc.operations.is_empty() {
            return Err(GraphQLLimitsError::ParseError(
                "Document contains no executable operations".to_string(),
            ));
        }

        // We analyze the highest cost/depth operation in the document
        let mut overall_report: Option<QueryCostReport> = None;

        for op in &doc.operations {
            let mut visited_fragments = HashSet::new();
            let mut total_field_count = 0;

            let (depth, complexity, breakdown) = Self::analyze_selection_set(
                &op.selection_set,
                1, // root fields start at depth 1
                1, // root multiplier
                config,
                &doc.fragments,
                &mut visited_fragments,
                &mut total_field_count,
            );

            let is_depth_exceeded = depth > config.max_depth;
            let is_complexity_exceeded = complexity > config.max_complexity;
            let is_allowed = !is_depth_exceeded && !is_complexity_exceeded;

            let report = QueryCostReport {
                operation_name: op.name.clone(),
                operation_type: op.op_type.clone(),
                depth,
                complexity,
                max_depth_allowed: config.max_depth,
                max_complexity_allowed: config.max_complexity,
                field_count: total_field_count,
                is_depth_exceeded,
                is_complexity_exceeded,
                is_allowed,
                field_breakdown: breakdown,
            };

            // Keep the most severe or first operation report
            match &overall_report {
                None => overall_report = Some(report),
                Some(existing) => {
                    if (!report.is_allowed && existing.is_allowed)
                        || report.complexity > existing.complexity
                        || report.depth > existing.depth
                    {
                        overall_report = Some(report);
                    }
                }
            }
        }

        let final_report = overall_report.unwrap();

        // Record metrics if opt-in enabled
        if config.enable_metrics {
            if let Some(m) = metrics {
                m.record(query, &final_report);
            }
        }

        // Enforce depth limit first, then complexity limit
        if final_report.is_depth_exceeded {
            return Err(GraphQLLimitsError::DepthLimitExceeded {
                depth: final_report.depth,
                max_depth: config.max_depth,
                operation: final_report.operation_name,
            });
        }

        if final_report.is_complexity_exceeded {
            return Err(GraphQLLimitsError::ComplexityLimitExceeded {
                complexity: final_report.complexity,
                max_complexity: config.max_complexity,
                operation: final_report.operation_name,
            });
        }

        Ok(final_report)
    }

    /// Evaluates a query and returns either `Ok(QueryCostReport)` or a standard
    /// GraphQL error JSON payload (`serde_json::Value`), suitable for direct HTTP response.
    pub fn validate_request(
        query: &str,
        config: &CostAnalysisConfig,
        metrics: Option<&QueryCostMetrics>,
    ) -> Result<QueryCostReport, serde_json::Value> {
        Self::analyze(query, config, metrics).map_err(|err| err.to_graphql_error_response())
    }

    fn analyze_selection_set(
        selections: &[AstSelection],
        current_depth: usize,
        current_multiplier: usize,
        config: &CostAnalysisConfig,
        fragments: &HashMap<String, AstFragment>,
        visited_fragments: &mut HashSet<String>,
        total_field_count: &mut usize,
    ) -> (usize, usize, Vec<FieldCostDetail>) {
        let mut max_depth_seen = current_depth.saturating_sub(1);
        let mut total_cost: usize = 0;
        let mut breakdown = Vec::new();

        for sel in selections {
            match sel {
                AstSelection::Field(field) => {
                    *total_field_count += 1;
                    let base_cost = config
                        .field_costs
                        .get(&field.name)
                        .copied()
                        .unwrap_or(config.default_field_cost);

                    // Inspect multiplier argument if configured (e.g. limit: 20)
                    let mut field_multiplier = 1;
                    for mult_arg in &config.multiplier_arguments {
                        if let Some(AstValue::Int(val)) = field.arguments.get(mult_arg) {
                            if *val > 0 {
                                field_multiplier =
                                    (*val as usize).min(config.max_multiplier).max(1);
                                break;
                            }
                        }
                    }

                    let effective_child_multiplier =
                        current_multiplier.saturating_mul(field_multiplier);

                    let (child_depth, child_cost, _) = if !field.selection_set.is_empty() {
                        Self::analyze_selection_set(
                            &field.selection_set,
                            current_depth + 1,
                            effective_child_multiplier,
                            config,
                            fragments,
                            visited_fragments,
                            total_field_count,
                        )
                    } else {
                        (current_depth, 0, Vec::new())
                    };

                    let field_depth = current_depth.max(child_depth);
                    if field_depth > max_depth_seen {
                        max_depth_seen = field_depth;
                    }

                    // Field total cost = (base_cost * current_multiplier) + child_cost
                    let own_cost = base_cost.saturating_mul(current_multiplier);
                    let field_total_cost = own_cost.saturating_add(child_cost);
                    total_cost = total_cost.saturating_add(field_total_cost);

                    if current_depth == 1 {
                        breakdown.push(FieldCostDetail {
                            field_name: field.name.clone(),
                            depth: field_depth,
                            cost: field_total_cost,
                            multiplier: field_multiplier,
                        });
                    }
                }
                AstSelection::InlineFragment { selection_set, .. } => {
                    let (frag_depth, frag_cost, _) = Self::analyze_selection_set(
                        selection_set,
                        current_depth,
                        current_multiplier,
                        config,
                        fragments,
                        visited_fragments,
                        total_field_count,
                    );
                    if frag_depth > max_depth_seen {
                        max_depth_seen = frag_depth;
                    }
                    total_cost = total_cost.saturating_add(frag_cost);
                }
                AstSelection::FragmentSpread(name) => {
                    // Prevent circular fragment expansion
                    if !visited_fragments.contains(name) {
                        visited_fragments.insert(name.clone());
                        if let Some(frag) = fragments.get(name) {
                            let (frag_depth, frag_cost, _) = Self::analyze_selection_set(
                                &frag.selection_set,
                                current_depth,
                                current_multiplier,
                                config,
                                fragments,
                                visited_fragments,
                                total_field_count,
                            );
                            if frag_depth > max_depth_seen {
                                max_depth_seen = frag_depth;
                            }
                            total_cost = total_cost.saturating_add(frag_cost);
                        }
                        visited_fragments.remove(name);
                    }
                }
            }
        }

        (max_depth_seen, total_cost, breakdown)
    }
}

// -----------------------------------------------------------------------------
// Unit Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_defaults() {
        let config = CostAnalysisConfig::default_safe();
        assert_eq!(config.max_depth, 7);
        assert_eq!(config.max_complexity, 100);
        assert_eq!(config.default_field_cost, 1);
        assert!(!config.enable_metrics);
        assert!(config.field_costs.contains_key("wallets"));
        assert!(config.field_costs.contains_key("transactions"));
    }

    #[test]
    fn test_simple_shallow_query_passes() {
        let query = r#"
            query GetWallet {
                wallet(id: "w123") {
                    id
                    publicKey
                    balance
                }
            }
        "#;
        let config = CostAnalysisConfig::default_safe();
        let report = QueryCostAnalyzer::analyze(query, &config, None).expect("Should pass");

        assert_eq!(report.depth, 2);
        assert!(report.complexity > 0);
        assert!(report.is_allowed);
        assert_eq!(report.operation_type, "query");
        assert_eq!(report.operation_name.as_deref(), Some("GetWallet"));
    }

    #[test]
    fn test_depth_limit_enforced() {
        let query = r#"
            query DeepQuery {
                level1 {
                    level2 {
                        level3 {
                            level4 {
                                level5 {
                                    level6 {
                                        level7 {
                                            level8 {
                                                id
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        "#;
        let config = CostAnalysisConfig::default_safe().with_max_depth(5);
        let err = QueryCostAnalyzer::analyze(query, &config, None).expect_err("Must fail depth");

        match err {
            GraphQLLimitsError::DepthLimitExceeded {
                depth, max_depth, ..
            } => {
                assert_eq!(depth, 9);
                assert_eq!(max_depth, 5);
                assert!(err.message().contains("Query depth limit of 5 exceeded"));
                assert_eq!(err.error_code(), "GRAPHQL_DEPTH_LIMIT_EXCEEDED");
            }
            _ => panic!("Expected DepthLimitExceeded, got {:?}", err),
        }
    }

    #[test]
    fn test_complexity_limit_enforced() {
        let query = r#"
            query ManyExpensiveFields {
                transactions { id }
                wallets { id }
                account { id }
                contracts { id }
                templates { id }
            }
        "#;
        // Strict complexity ceiling
        let config = CostAnalysisConfig::default_safe().with_max_complexity(15);
        let err =
            QueryCostAnalyzer::analyze(query, &config, None).expect_err("Must fail complexity");

        match err {
            GraphQLLimitsError::ComplexityLimitExceeded {
                complexity,
                max_complexity,
                ..
            } => {
                assert!(complexity > 15);
                assert_eq!(max_complexity, 15);
                assert!(err
                    .message()
                    .contains("Query complexity limit of 15 exceeded"));
                assert_eq!(err.error_code(), "GRAPHQL_COMPLEXITY_LIMIT_EXCEEDED");
            }
            _ => panic!("Expected ComplexityLimitExceeded, got {:?}", err),
        }
    }

    #[test]
    fn test_multiplier_arguments_scale_children() {
        let query_single = r#"
            query {
                templates(limit: 1) {
                    name
                    description
                    rating
                }
            }
        "#;
        let query_twenty = r#"
            query {
                templates(limit: 20) {
                    name
                    description
                    rating
                }
            }
        "#;

        let config = CostAnalysisConfig::default_safe().with_max_complexity(500);
        let r1 = QueryCostAnalyzer::analyze(query_single, &config, None).unwrap();
        let r20 = QueryCostAnalyzer::analyze(query_twenty, &config, None).unwrap();

        // 20 limit must have higher complexity than 1 limit
        assert!(r20.complexity > r1.complexity);
    }

    #[test]
    fn test_fragments_resolution_and_cycle_guard() {
        let query = r#"
            fragment WalletDetails on Wallet {
                id
                publicKey
                balance
            }

            query {
                wallets {
                    ...WalletDetails
                }
            }
        "#;
        let config = CostAnalysisConfig::default_safe();
        let report = QueryCostAnalyzer::analyze(query, &config, None).expect("Fragment must pass");

        assert_eq!(report.depth, 2);
        assert!(report.field_count >= 4);
    }

    #[test]
    fn test_opt_in_metrics_recording() {
        let query = r#"
            query TooDeep {
                a { b { c { d { e { f } } } } }
            }
        "#;
        let config = CostAnalysisConfig::default_safe()
            .with_max_depth(3)
            .with_metrics(true);
        let metrics = QueryCostMetrics::new();

        let _ = QueryCostAnalyzer::analyze(query, &config, Some(&metrics));

        let snap = metrics.snapshot();
        assert_eq!(snap.total_analyzed, 1);
        assert_eq!(snap.total_rejected, 1);
        assert_eq!(snap.rejected_depth, 1);
        assert_eq!(snap.rejected_complexity, 0);
        assert_eq!(snap.recent_rejections.len(), 1);
        assert!(snap.recent_rejections[0]
            .reason
            .contains("depth_limit_exceeded"));
    }

    #[test]
    fn test_graphql_error_response_formatting() {
        let err = GraphQLLimitsError::DepthLimitExceeded {
            depth: 8,
            max_depth: 5,
            operation: Some("BigQuery".to_string()),
        };
        let json = err.to_graphql_error_response();

        assert!(json["errors"].is_array());
        let first_err = &json["errors"][0];
        assert_eq!(
            first_err["extensions"]["code"],
            "GRAPHQL_DEPTH_LIMIT_EXCEEDED"
        );
        assert_eq!(first_err["extensions"]["depth"], 8);
        assert_eq!(first_err["extensions"]["max_depth"], 5);
        assert!(json["data"].is_null());
    }
}
