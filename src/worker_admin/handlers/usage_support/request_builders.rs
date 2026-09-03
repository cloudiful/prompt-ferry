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
        match current.request_storage_mode.as_deref().unwrap_or("full") {
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
    let raw_store = state.raw_payload_store.read().await.clone();
    match db::get_visible_usage_event_detail_with_raw_store(
        &state.pool,
        record_id,
        visible_user_id,
        raw_store.as_deref(),
    )
    .await
    {
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
        conversation_source: entry
            .conversation_source
            .unwrap_or_else(|| "none".to_string()),
        client_installation_id: entry.client_installation_id,
        normalized_item_count: entry.normalized_item_count,
        request_storage_mode: entry
            .request_storage_mode
            .unwrap_or_else(|| "full".to_string()),
        request_raw_json: entry.request_raw_json,
        request_has_previous_response_id: entry.request_has_previous_response_id.unwrap_or(false),
        request_previous_response_id: entry.request_previous_response_id,
        request_previous_response_parent_found: entry.request_previous_response_parent_found,
        rendered_text,
        messages,
    })
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

pub(in crate::worker_admin::handlers) fn request_record_not_found() -> Response {
    error(
        StatusCode::NOT_FOUND,
        "not_found",
        "request record not found",
    )
}
