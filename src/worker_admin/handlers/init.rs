use super::*;

fn origin_allowed(headers: &HeaderMap) -> bool {
    let mut origins = headers.get_all(header::ORIGIN).iter();
    let Some(origin) = origins.next() else {
        return true;
    };
    if origins.next().is_some() || headers.get_all(header::HOST).iter().count() != 1 {
        return false;
    }
    let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let Ok(origin) = origin.to_str().unwrap_or("").parse::<axum::http::Uri>() else {
        return false;
    };
    matches!(origin.scheme_str(), Some("http" | "https"))
        && origin
            .authority()
            .is_some_and(|a| !a.as_str().contains('@') && a.as_str().eq_ignore_ascii_case(host))
        && origin.path() == "/"
        && origin.query().is_none()
}

fn valid(body: &SetupRequest) -> bool {
    !body.login_name.trim().is_empty()
        && body.login_name.len() <= 256
        && !body.password.trim().is_empty()
        && body.password.len() <= 1024
        && body
            .display_name
            .as_ref()
            .is_none_or(|v| !v.trim().is_empty() && v.len() <= 256)
}

pub(super) async fn setup_status(State(state): State<AdminState>) -> Response {
    if state.node_setup.is_none() {
        return Json(SetupStatus {
            initialized: true,
            setup_available: false,
        })
        .into_response();
    }
    match db::initial_admin::status(&state.pool).await {
        Ok(initialized) => Json(SetupStatus {
            initialized,
            setup_available: !initialized,
        })
        .into_response(),
        Err(_) => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "service_unavailable",
            "setup unavailable",
        ),
    }
}

pub(super) async fn setup_initialize(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<SetupRequest>,
) -> Response {
    let Some(auth) = state.node_setup.as_ref() else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "service_unavailable",
            "setup unavailable",
        );
    };
    if !origin_allowed(&headers) {
        return error(StatusCode::FORBIDDEN, "forbidden", "origin not allowed");
    }
    let Ok(_permit) = auth.attempts.try_acquire() else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "service_unavailable",
            "setup busy",
        );
    };
    if !auth.verify(&body.master_secret) {
        return error(StatusCode::UNAUTHORIZED, "unauthorized", "invalid proof");
    }
    if !valid(&body) {
        return bad_request("invalid setup fields");
    }
    let hash = match hash_password(&body.password) {
        Ok(hash) => hash,
        Err(_) => {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "service_unavailable",
                "setup unavailable",
            );
        }
    };
    let display = body.display_name.as_deref().unwrap_or(&body.login_name);
    match db::initial_admin::create(&state.pool, &body.login_name, display, hash).await {
        Ok(true) => (
            StatusCode::CREATED,
            Json(SetupStatus {
                initialized: true,
                setup_available: false,
            }),
        )
            .into_response(),
        Ok(false) => error(StatusCode::CONFLICT, "conflict", "already initialized"),
        Err(_) => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "service_unavailable",
            "setup unavailable",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_origin_and_fields() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "example.test".parse().unwrap());
        headers.insert(header::ORIGIN, "https://example.test".parse().unwrap());
        assert!(origin_allowed(&headers));
        headers.insert(header::ORIGIN, "https://evil.test".parse().unwrap());
        headers.insert("x-forwarded-host", "evil.test".parse().unwrap());
        assert!(!origin_allowed(&headers));
        headers.remove(header::HOST);
        assert!(!origin_allowed(&headers));
        headers.insert(header::HOST, "evil.test".parse().unwrap());
        headers.append(header::HOST, "example.test".parse().unwrap());
        assert!(!origin_allowed(&headers));
        let mut body = SetupRequest {
            master_secret: String::new(),
            login_name: "admin".into(),
            password: "password".into(),
            display_name: None,
        };
        assert!(valid(&body));
        body.login_name = " ".into();
        assert!(!valid(&body));
        body.login_name = "admin".into();
        body.password = "x".repeat(1025);
        assert!(!valid(&body));
        body.password = "secret".into();
        body.display_name = Some(" ".into());
        assert!(!valid(&body));
    }
}
