use axum::{
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::json;

pub fn bearer_token(headers: &HeaderMap) -> Result<&str, Response> {
    let Some(value) = headers.get(http::header::AUTHORIZATION) else {
        return Err(error_response(
            StatusCode::UNAUTHORIZED,
            "missing_authorization",
            "missing Authorization header",
        ));
    };

    let Ok(value) = value.to_str() else {
        return Err(error_response(
            StatusCode::UNAUTHORIZED,
            "invalid_authorization",
            "invalid Authorization header",
        ));
    };

    value.strip_prefix("Bearer ").ok_or_else(|| {
        error_response(
            StatusCode::UNAUTHORIZED,
            "invalid_authorization",
            "Authorization must use Bearer token",
        )
    })
}

pub fn check_bearer(headers: &HeaderMap, expected: &str) -> Result<(), Response> {
    if expected.is_empty() {
        return Err(error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "auth_not_configured",
            "authentication token is not configured",
        ));
    }

    let token = bearer_token(headers)?;

    if token != expected {
        return Err(error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "invalid token",
        ));
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
}
