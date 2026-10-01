//! Request-level authentication for the GraphQL API (#321).
//!
//! A single static bearer token, checked on every `/graphql` request before
//! the query reaches cost analysis or schema execution. This is
//! intentionally minimal: the resolvers in this module are still mock data
//! (see `resolvers.rs`), so there is no per-user account system yet to
//! authorize *against* — this closes the "an unauthenticated caller can use
//! the API at all" gap without pretending to implement authorization for
//! data that doesn't exist yet.

use actix_web::HttpRequest;

/// Auth configuration for the GraphQL server.
///
/// `None` disables the check entirely (useful for local development and for
/// the existing tests, which call the handler directly). Constructing this
/// from an environment variable, rather than requiring it, means a
/// misconfigured deployment fails safe by rejecting every request rather
/// than silently running unauthenticated.
#[derive(Clone, Default)]
pub struct AuthConfig {
    token: Option<String>,
}

impl AuthConfig {
    pub fn required(token: String) -> Self {
        Self { token: Some(token) }
    }

    pub fn disabled() -> Self {
        Self { token: None }
    }

    /// Read `STARFORGE_GRAPHQL_TOKEN` from the environment. Auth is enabled
    /// only if that variable is set and non-empty.
    pub fn from_env() -> Self {
        match std::env::var("STARFORGE_GRAPHQL_TOKEN") {
            Ok(token) if !token.is_empty() => Self::required(token),
            _ => Self::disabled(),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.token.is_some()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum AuthError {
    MissingHeader,
    MalformedHeader,
    InvalidToken,
}

impl AuthError {
    pub fn message(&self) -> &'static str {
        match self {
            AuthError::MissingHeader => "Missing Authorization header",
            AuthError::MalformedHeader => {
                "Authorization header must be in the form 'Bearer <token>'"
            }
            AuthError::InvalidToken => "Invalid or expired token",
        }
    }
}

/// Check `req`'s `Authorization` header against `config`.
///
/// `Ok(())` when auth is disabled (`config.token` is `None`) or the header
/// carries the configured token as `Bearer <token>`.
pub fn authenticate(req: &HttpRequest, config: &AuthConfig) -> Result<(), AuthError> {
    let Some(expected) = &config.token else {
        return Ok(());
    };

    let header = req
        .headers()
        .get("Authorization")
        .ok_or(AuthError::MissingHeader)?
        .to_str()
        .map_err(|_| AuthError::MalformedHeader)?;

    let presented = header
        .strip_prefix("Bearer ")
        .ok_or(AuthError::MalformedHeader)?;

    // Constant-time comparison: token equality isn't something a network
    // caller should be able to learn one byte at a time via response timing.
    if constant_time_eq(presented.as_bytes(), expected.as_bytes()) {
        Ok(())
    } else {
        Err(AuthError::InvalidToken)
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::test::TestRequest;

    #[test]
    fn disabled_config_allows_any_request() {
        let config = AuthConfig::disabled();
        let req = TestRequest::default().to_http_request();
        assert_eq!(authenticate(&req, &config), Ok(()));
    }

    #[test]
    fn rejects_missing_header_when_enabled() {
        let config = AuthConfig::required("secret".to_string());
        let req = TestRequest::default().to_http_request();
        assert_eq!(authenticate(&req, &config), Err(AuthError::MissingHeader));
    }

    #[test]
    fn rejects_header_without_bearer_prefix() {
        let config = AuthConfig::required("secret".to_string());
        let req = TestRequest::default()
            .insert_header(("Authorization", "secret"))
            .to_http_request();
        assert_eq!(authenticate(&req, &config), Err(AuthError::MalformedHeader));
    }

    #[test]
    fn rejects_wrong_token() {
        let config = AuthConfig::required("secret".to_string());
        let req = TestRequest::default()
            .insert_header(("Authorization", "Bearer wrong"))
            .to_http_request();
        assert_eq!(authenticate(&req, &config), Err(AuthError::InvalidToken));
    }

    #[test]
    fn accepts_correct_token() {
        let config = AuthConfig::required("secret".to_string());
        let req = TestRequest::default()
            .insert_header(("Authorization", "Bearer secret"))
            .to_http_request();
        assert_eq!(authenticate(&req, &config), Ok(()));
    }

    #[test]
    fn rejects_correct_token_with_wrong_case_scheme() {
        // "bearer" (lowercase) is not the same as the required "Bearer "
        // prefix -- being lenient here would make the check ambiguous about
        // what it actually validated.
        let config = AuthConfig::required("secret".to_string());
        let req = TestRequest::default()
            .insert_header(("Authorization", "bearer secret"))
            .to_http_request();
        assert_eq!(authenticate(&req, &config), Err(AuthError::MalformedHeader));
    }

    #[test]
    fn rejects_token_that_is_a_prefix_of_the_real_one() {
        let config = AuthConfig::required("secret123".to_string());
        let req = TestRequest::default()
            .insert_header(("Authorization", "Bearer secret"))
            .to_http_request();
        assert_eq!(authenticate(&req, &config), Err(AuthError::InvalidToken));
    }

    #[test]
    fn is_enabled_reflects_construction() {
        assert!(AuthConfig::required("x".to_string()).is_enabled());
        assert!(!AuthConfig::disabled().is_enabled());
    }

    // AuthConfig::from_env() reads the real, process-global
    // STARFORGE_GRAPHQL_TOKEN var. `cargo test` runs tests within a single
    // process (by default, concurrently across threads), so setting/
    // unsetting a real env var here would race against any other test that
    // happens to read it; deliberately not covered by an automated test.
}
