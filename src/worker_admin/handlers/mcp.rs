use super::*;

pub(super) async fn list_mcp_servers(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Query(query): Query<TablePageQuery>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    let first = query.first.unwrap_or(0).max(0);
    let rows = query.rows.unwrap_or(20).clamp(1, 200);
    let result = if user.is_admin {
        db::list_mcp_servers_page(&state.pool, first, rows).await
    } else {
        db::list_user_mcp_servers_page(&state.pool, user.user_id, first, rows).await
    };
    match result {
        Ok((total, servers)) => Json(McpServerPageResponse {
            servers,
            total,
            first,
            rows,
        })
        .into_response(),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn create_mcp_server(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<McpServerRequest>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    if let Err(response) = body.validate_for_create(&state, &user).await {
        tracing::warn!(
            user_id = user.user_id,
            is_admin = user.is_admin,
            name = %body.name,
            transport = %body.transport,
            "mcp server create validation failed"
        );
        return response;
    }
    match db::create_mcp_server(&state.pool, body.into_input(&user, None)).await {
        Ok(server) => {
            state.mcp_catalog_cache.invalidate(server.server_id).await;
            if server.enabled {
                state.mcp_catalog_service.spawn_refresh(server.clone());
            }
            Json(server).into_response()
        }
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn update_mcp_server(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(server_id): Path<Uuid>,
    Json(body): Json<McpServerRequest>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    let existing = if user.is_admin {
        match db::get_mcp_server(&state.pool, server_id).await {
            Ok(server) => server,
            Err(err) => return internal(&state, err),
        }
    } else {
        match db::get_user_mcp_server(&state.pool, user.user_id, server_id).await {
            Ok(server) => server,
            Err(err) => return internal(&state, err),
        }
    };
    let Some(existing) = existing else {
        return error(StatusCode::NOT_FOUND, "not_found", "mcp server not found");
    };
    if let Err(response) = body.validate_for_update(&state, server_id, &user).await {
        tracing::warn!(
            user_id = user.user_id,
            is_admin = user.is_admin,
            server_id = %server_id,
            name = %body.name,
            transport = %body.transport,
            "mcp server update validation failed"
        );
        return response;
    }
    match db::update_mcp_server(
        &state.pool,
        server_id,
        body.into_input(&user, Some(&existing)),
    )
    .await
    {
        Ok(Some(server)) => {
            state.mcp_catalog_cache.invalidate(server.server_id).await;
            if server.enabled {
                state.mcp_catalog_service.spawn_refresh(server.clone());
            }
            Json(server).into_response()
        }
        Ok(None) => error(StatusCode::NOT_FOUND, "not_found", "mcp server not found"),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn delete_mcp_server(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(server_id): Path<Uuid>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    if !user.is_admin {
        match db::get_user_mcp_server(&state.pool, user.user_id, server_id).await {
            Ok(Some(_)) => {}
            Ok(None) => return error(StatusCode::NOT_FOUND, "not_found", "mcp server not found"),
            Err(err) => return internal(&state, err),
        }
    }
    match db::delete_mcp_server(&state.pool, server_id).await {
        Ok(true) => {
            state.mcp_catalog_service.invalidate(server_id).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => error(StatusCode::NOT_FOUND, "not_found", "mcp server not found"),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn test_mcp_server(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(server_id): Path<Uuid>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    let server = if user.is_admin {
        match db::get_mcp_server(&state.pool, server_id).await {
            Ok(server) => server,
            Err(err) => return internal(&state, err),
        }
    } else {
        match db::get_user_mcp_server(&state.pool, user.user_id, server_id).await {
            Ok(server) => server,
            Err(err) => return internal(&state, err),
        }
    };
    let Some(server) = server else {
        return error(StatusCode::NOT_FOUND, "not_found", "mcp server not found");
    };
    if !server.enabled {
        return Json(McpTestResponse {
            ok: false,
            message: "mcp server is disabled".to_string(),
            duration_ms: 0,
            tool_count: 0,
            resource_count: 0,
            prompt_count: 0,
            tools: Vec::new(),
            resources: Vec::new(),
            prompts: Vec::new(),
        })
        .into_response();
    }
    let started = Instant::now();
    let (server, _) = match state
        .mcp_catalog_service
        .refresh_server_by_id(server_id)
        .await
    {
        Ok(result) => result,
        Err(err) => {
            return Json(McpTestResponse {
                ok: false,
                message: maybe_redact(&state, &err.to_string()),
                duration_ms: started.elapsed().as_millis(),
                tool_count: 0,
                resource_count: 0,
                prompt_count: 0,
                tools: Vec::new(),
                resources: Vec::new(),
                prompts: Vec::new(),
            })
            .into_response();
        }
    };
    let catalog = match crate::mcp::catalog_for_server(
        &state.mcp_catalog_cache,
        std::slice::from_ref(&server),
        &server.name,
    )
    .await
    {
        Ok(catalog) => catalog,
        Err(err) => return internal(&state, err),
    };
    Json(McpTestResponse {
        ok: true,
        message: format!(
            "{} tools, {} resources, {} prompts",
            catalog.tools.len(),
            catalog.resources.len(),
            catalog.prompts.len()
        ),
        duration_ms: started.elapsed().as_millis(),
        tool_count: catalog.tools.len(),
        resource_count: catalog.resources.len(),
        prompt_count: catalog.prompts.len(),
        tools: catalog.tools,
        resources: catalog.resources,
        prompts: catalog.prompts,
    })
    .into_response()
}

pub(super) async fn get_mcp_catalog(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(server_id): Path<Uuid>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    let server = if user.is_admin {
        match db::get_mcp_server(&state.pool, server_id).await {
            Ok(server) => server,
            Err(err) => return internal(&state, err),
        }
    } else {
        match db::get_user_mcp_server(&state.pool, user.user_id, server_id).await {
            Ok(server) => server,
            Err(err) => return internal(&state, err),
        }
    };
    let Some(server) = server else {
        return error(StatusCode::NOT_FOUND, "not_found", "mcp server not found");
    };
    if !server.enabled {
        return Json(McpCatalogResponse {
            tools: Vec::new(),
            resources: Vec::new(),
            prompts: Vec::new(),
        })
        .into_response();
    }
    let visible_servers = match db::list_visible_mcp_servers(&state.pool, Some(user.user_id)).await
    {
        Ok(servers) => servers,
        Err(err) => return internal(&state, err),
    };
    match crate::mcp::catalog_for_server(&state.mcp_catalog_cache, &visible_servers, &server.name)
        .await
    {
        Ok(catalog) => Json(catalog).into_response(),
        Err(err) if err.to_string().contains("catalog is not ready") => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mcp_catalog_unavailable",
            "mcp catalog is not ready",
        ),
        Err(err) => internal(&state, err),
    }
}
