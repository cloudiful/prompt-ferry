use axum::{
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::json;

pub fn client_token(headers: &HeaderMap) -> Result<&str, Box<Response>> {
    let bearer = headers
        .get(http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let api_key = headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok());

    if let (Some(bearer), Some(api_key)) = (bearer, api_key)
        && bearer != api_key
    {
        return Err(Box::new(error_response(
            StatusCode::UNAUTHORIZED,
            "invalid_authorization",
            "Authorization and x-api-key must contain the same token",
        )));
    }

    bearer
        .or(api_key)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| {
            Box::new(error_response(
                StatusCode::UNAUTHORIZED,
                "missing_authorization",
                "missing Authorization or x-api-key header",
            ))
        })
}

pub fn bearer_token(headers: &HeaderMap) -> Result<&str, Box<Response>> {
    let Some(value) = headers.get(http::header::AUTHORIZATION) else {
        return Err(Box::new(error_response(
            StatusCode::UNAUTHORIZED,
            "missing_authorization",
            "missing Authorization header",
        )));
    };

    let Ok(value) = value.to_str() else {
        return Err(Box::new(error_response(
            StatusCode::UNAUTHORIZED,
            "invalid_authorization",
            "invalid Authorization header",
        )));
    };

    value.strip_prefix("Bearer ").ok_or_else(|| {
        Box::new(error_response(
            StatusCode::UNAUTHORIZED,
            "invalid_authorization",
            "Authorization must use Bearer token",
        ))
    })
}

/// Bearer-token check shared by authenticated endpoints. An empty expected
/// token disables authentication entirely for that endpoint; this is used by
/// the relay worker bridge when no worker token is configured.
pub fn check_bearer(headers: &HeaderMap, expected: &str) -> Result<(), Box<Response>> {
    if expected.is_empty() {
        return Ok(());
    }

    let token = bearer_token(headers)?;

    if token != expected {
        return Err(Box::new(error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "invalid token",
        )));
    }

    Ok(())
}

pub fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        axum::Json(json!({
            "error": {
                "code": code,
                "message": message,
            }
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_missing_bearer() {
        let headers = HeaderMap::new();
        assert!(check_bearer(&headers, "secret").is_err());
    }

    #[test]
    fn accepts_matching_bearer() {
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::AUTHORIZATION,
            http::HeaderValue::from_static("Bearer secret"),
        );
        assert!(check_bearer(&headers, "secret").is_ok());
    }

    #[test]
    fn rejects_wrong_bearer() {
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::AUTHORIZATION,
            http::HeaderValue::from_static("Bearer other"),
        );
        assert!(check_bearer(&headers, "secret").is_err());
    }

    #[test]
    fn empty_expected_token_disables_authentication() {
        let headers = HeaderMap::new();
        assert!(check_bearer(&headers, "").is_ok());
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::AUTHORIZATION,
            http::HeaderValue::from_static("Bearer anything"),
        );
        assert!(check_bearer(&headers, "").is_ok());
    }

    #[test]
    fn accepts_anthropic_api_key() {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", http::HeaderValue::from_static("secret"));
        assert_eq!(client_token(&headers).unwrap(), "secret");
    }

    #[test]
    fn rejects_conflicting_authentication_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::AUTHORIZATION,
            http::HeaderValue::from_static("Bearer first"),
        );
        headers.insert("x-api-key", http::HeaderValue::from_static("second"));
        assert!(client_token(&headers).is_err());
    }
}
