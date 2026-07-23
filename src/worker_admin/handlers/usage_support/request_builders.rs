use super::*;

pub(in crate::worker_admin::handlers) async fn reconstruct_usage_request_refs(
    pool: &sqlx::PgPool,
    entry: &db::UsageEventChainEntry,
) -> anyhow::Result<Option<Vec<db::PromptMessageRef>>> {
    let mut current = entry.clone();
    let mut segments = Vec::new();
    let mut depth = 0;
    loop {
        depth += 1;
        if depth > REQUEST_CHAIN_DEPTH_LIMIT {
            return Ok(None);
        }
        match current.request_storage_mode.as_str() {
            "full" => {
                let Some(value) = current.request_full_json.as_ref() else {
                    return Ok(None);
                };
                segments.push(db::decode_prompt_message_refs(value)?);
                break;
            }
            "append_delta" => {
                let Some(value) = current.request_delta_json.as_ref() else {
                    return Ok(None);
                };
                segments.push(db::decode_prompt_message_refs(value)?);
                let Some(parent_event_id) = current.parent_event_id else {
                    return Ok(None);
                };
                let Some(parent) = db::get_usage_event_chain_entry(pool, parent_event_id).await?
                else {
                    return Ok(None);
                };
                current = parent;
            }
            _ => return Ok(None),
        }
    }
    segments.reverse();
    let mut refs = Vec::new();
    for segment in segments {
        refs.extend(segment);
    }
    Ok(Some(refs))
}

pub(in crate::worker_admin::handlers) async fn build_usage_request_messages(
    pool: &sqlx::PgPool,
    refs: &[db::PromptMessageRef],
    parent_refs: &[db::PromptMessageRef],
    parent_turn: Option<i32>,
) -> anyhow::Result<Vec<UsageRequestFullMessage>> {
    let mut hashes = refs
        .iter()
        .map(|reference| reference.block_hash.clone())
        .collect::<Vec<_>>();
    hashes.sort();
    hashes.dedup();
    let blocks = db::get_usage_prompt_blocks(pool, &hashes).await?;
    let block_map = blocks
        .into_iter()
        .map(|block| (block.block_hash.clone(), block))
        .collect::<HashMap<_, _>>();
    let repeated_parent_systems = if parent_turn.is_some() {
        parent_refs
            .iter()
            .filter(|reference| reference.role == "system")
            .map(|reference| reference.block_hash.clone())
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    Ok(refs
        .iter()
        .map(|reference| {
            let block = block_map.get(&reference.block_hash);
            UsageRequestFullMessage {
                role: reference.role.clone(),
                block_hash: reference.block_hash.clone(),
                preview_text: block
                    .map(|block| block.preview_text.clone())
                    .unwrap_or_else(|| reference.block_hash.clone()),
                content_json: block
                    .map(|block| block.content_json.clone())
                    .unwrap_or(Value::Null),
                same_as_turn: (reference.role == "system"
                    && repeated_parent_systems.contains(&reference.block_hash))
                .then_some(parent_turn.unwrap_or(1)),
            }
        })
        .collect())
}

pub(in crate::worker_admin::handlers) async fn get_visible_usage_event_detail_or_not_found(
    state: &AdminState,
    record_id: i64,
    visible_user_id: Option<i64>,
) -> Result<db::UsageEventDetail, Response> {
    match db::get_visible_usage_event_detail(&state.pool, record_id, visible_user_id).await {
        Ok(Some(event)) => Ok(event),
        Ok(None) => Err(request_record_not_found()),
        Err(err) => Err(internal(state, err)),
    }
}

pub(in crate::worker_admin::handlers) async fn get_visible_usage_chain_entry_or_not_found(
    state: &AdminState,
    record_id: i64,
    visible_user_id: Option<i64>,
) -> Result<db::UsageEventChainEntry, Response> {
    match db::get_visible_usage_event_chain_entry(&state.pool, record_id, visible_user_id).await {
        Ok(Some(entry)) => Ok(entry),
        Ok(None) => Err(request_record_not_found()),
        Err(err) => Err(internal(state, err)),
    }
}

pub(in crate::worker_admin::handlers) async fn build_session_route_options_response(
    state: &AdminState,
    record_id: i64,
    visible_user_id: Option<i64>,
    fallback_user_id: i64,
) -> Result<SessionRouteOptionsResponse, Response> {
    let event =
        get_visible_usage_event_detail_or_not_found(state, record_id, visible_user_id).await?;
    let Some(conversation_id) = event.conversation_id else {
        return Err(bad_request("request record has no conversation_id"));
    };
    let route_user_id = event.user_id.unwrap_or(fallback_user_id);
    let override_entry = db::get_conversation_endpoint_override(&state.pool, conversation_id)
        .await
        .map_err(|err| internal(state, err))?;
    let (fallback_route, candidate) = db::resolve_model_route_with_fallback(
        &state.pool,
        route_user_id,
        event.model.as_deref(),
        true,
    )
    .await
    .map_err(|err| internal(state, err))?;

    let mut options = if let Some(candidate) = candidate {
        build_candidate_session_route_options(&event, &override_entry, candidate)
    } else {
        Vec::new()
    };
    if options.is_empty()
        && let Some(endpoint_id) = event
            .endpoint_id
            .or(fallback_route.map(|route| route.route_id))
        && let Ok(Some(endpoint)) = db::get_endpoint(&state.pool, endpoint_id).await
    {
        options.push(db::SessionRouteOption {
            endpoint_id,
            endpoint_name: endpoint.name,
            keys: endpoint
                .api_keys
                .iter()
                .filter(|key| key.enabled && !key.key_id.is_nil())
                .map(|key| db::SessionRouteKeyOption {
                    key_id: key.key_id,
                    key_label: key.key_label.clone(),
                })
                .collect(),
            is_override: override_entry
                .as_ref()
                .is_some_and(|entry| entry.endpoint_id == endpoint_id),
            is_preferred: true,
        });
    }

    Ok(SessionRouteOptionsResponse {
        conversation_id,
        current_endpoint_id: event.endpoint_id,
        current_endpoint_key_id: event.endpoint_key_id,
        current_endpoint_key_label: event.endpoint_key_label.clone(),
        override_endpoint_id: override_entry.as_ref().map(|entry| entry.endpoint_id),
        override_endpoint_key_id: override_entry
            .as_ref()
            .and_then(|entry| entry.endpoint_key_id),
        override_endpoint_key_label: override_entry
            .as_ref()
            .and_then(|entry| entry.endpoint_key_label.clone()),
        options,
    })
}

pub(in crate::worker_admin::handlers) async fn build_usage_request_full_response(
    state: &AdminState,
    record_id: i64,
    visible_user_id: Option<i64>,
) -> Result<UsageRequestFullResponse, Response> {
    let entry =
        get_visible_usage_chain_entry_or_not_found(state, record_id, visible_user_id).await?;
    let refs = reconstruct_usage_request_refs(&state.pool, &entry)
        .await
        .map_err(|err| internal(state, err))?
        .unwrap_or_default();
    let (parent_refs, parent_turn) = if let Some(parent_event_id) = entry.parent_event_id {
        let parent =
            get_visible_usage_chain_entry_or_not_found(state, parent_event_id, visible_user_id)
                .await?;
        let refs = reconstruct_usage_request_refs(&state.pool, &parent)
            .await
            .map_err(|err| internal(state, err))?
            .unwrap_or_default();
        (refs, parent.conversation_seq)
    } else {
        (Vec::new(), None)
    };
    let messages = build_usage_request_messages(&state.pool, &refs, &parent_refs, parent_turn)
        .await
        .map_err(|err| internal(state, err))?;
    let rendered_text = render_usage_request_text(&messages);

    Ok(UsageRequestFullResponse {
        conversation_id: entry.conversation_id,
        record_id,
        conversation_source: entry.conversation_source,
        client_installation_id: entry.client_installation_id,
        normalized_item_count: entry.normalized_item_count,
        request_storage_mode: entry.request_storage_mode,
        request_raw_json: entry.request_raw_json,
        request_has_previous_response_id: entry.request_has_previous_response_id,
        request_previous_response_id: entry.request_previous_response_id,
        request_previous_response_parent_found: entry.request_previous_response_parent_found,
        rendered_text,
        messages,
    })
}

fn build_candidate_session_route_options(
    event: &db::UsageEventDetail,
    override_entry: &Option<db::ConversationEndpointOverride>,
    candidate: db::ModelRouteCandidate,
) -> Vec<db::SessionRouteOption> {
    candidate
        .targets
        .iter()
        .map(|target| db::SessionRouteOption {
            endpoint_id: target.endpoint_id,
            endpoint_name: target.endpoint_name.clone(),
            keys: target
                .api_keys
                .iter()
                .filter(|key| key.enabled && !key.key_id.is_nil())
                .map(|key| db::SessionRouteKeyOption {
                    key_id: key.key_id,
                    key_label: key.key_label.clone(),
                })
                .collect(),
            is_override: override_entry
                .as_ref()
                .is_some_and(|entry| entry.endpoint_id == target.endpoint_id),
            is_preferred: event.endpoint_id == Some(target.endpoint_id),
        })
        .collect()
}

fn render_usage_request_text(messages: &[UsageRequestFullMessage]) -> String {
    if messages.is_empty() {
        return String::new();
    }

    render_prompt_text(
        &messages
            .iter()
            .map(|message| RenderedPromptMessage {
                role: message.role.clone(),
                block_hash: message.block_hash.clone(),
                preview_text: message.preview_text.clone(),
                content_json: message.content_json.clone(),
                same_as_turn: message.same_as_turn,
            })
            .collect::<Vec<_>>(),
    )
}

fn request_record_not_found() -> Response {
    error(
        StatusCode::NOT_FOUND,
        "not_found",
        "request record not found",
    )
}
