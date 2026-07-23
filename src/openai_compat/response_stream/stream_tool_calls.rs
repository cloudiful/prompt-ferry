use super::*;
use tracing::warn;

impl ChatResponseStreamAdapter {
    pub(super) fn observe_tool_call_delta(
        &mut self,
        delta: ChatToolCallDelta,
        output: &mut Vec<Vec<u8>>,
    ) -> Result<(), CompatError> {
        self.ensure_response_created(output)?;

        let position = self.resolve_tool_call_position(delta.index, delta.call_id.as_deref());
        let (added_item, delta_item) = {
            let state = self
                .tool_calls
                .get_mut(position)
                .expect("tool call state inserted");
            if let Some(call_id) = delta.call_id.filter(|value| !value.is_empty()) {
                state.call_id = call_id;
            }
            if let Some(name) = delta.name.filter(|value| !value.is_empty()) {
                state.name = Some(name);
            }

            let added_item = if !state.added_emitted {
                state.added_emitted = true;
                Some((
                    state.output_index,
                    function_call_item(
                        &state.call_id,
                        state.name.as_deref().unwrap_or(""),
                        "",
                        "in_progress",
                    ),
                ))
            } else {
                None
            };

            let delta_item = if !delta.arguments.is_empty() {
                state.arguments.push_str(&delta.arguments);
                Some((state.call_id.clone(), state.output_index, delta.arguments))
            } else {
                None
            };
            (added_item, delta_item)
        };

        if let Some((output_index, item)) = added_item {
            self.push_event(
                output,
                json!({
                    "type": "response.output_item.added",
                    "output_index": output_index,
                    "item": item,
                }),
            )?;
        }
        if let Some((call_id, output_index, arguments_delta)) = delta_item {
            self.push_event(
                output,
                json!({
                    "type": "response.function_call_arguments.delta",
                    "response_id": self.current_response_id(),
                    "item_id": call_id.clone(),
                    "output_index": output_index,
                    "call_id": call_id,
                    "delta": arguments_delta,
                }),
            )?;
        }

        Ok(())
    }

    pub(super) fn emit_pending_tool_completions(
        &mut self,
        output: &mut Vec<Vec<u8>>,
    ) -> Result<(), CompatError> {
        let response_id = self.current_response_id();
        let mut events = self
            .tool_calls
            .iter_mut()
            .filter(|state| !state.done_emitted)
            .map(|state| {
                let (arguments, repair_status) = normalize_tool_call_arguments(
                    state.name.as_deref().unwrap_or(""),
                    &state.arguments,
                    &self.full_text,
                )?;
                if repair_status == ToolCallArgumentRepairStatus::Repaired {
                    warn!(
                        model = self.model.as_deref().unwrap_or("unknown"),
                        tool_name = state.name.as_deref().unwrap_or(""),
                        streaming = true,
                        "repaired invalid upstream tool call arguments from assistant text"
                    );
                }
                state.arguments = arguments;
                let event = (
                    state.call_id.clone(),
                    state.name.clone().unwrap_or_default(),
                    state.arguments.clone(),
                    state.output_index,
                );
                state.done_emitted = true;
                Ok(event)
            })
            .collect::<Result<Vec<_>, CompatError>>()?;
        events.sort_by_key(|(_, _, _, output_index)| *output_index);
        for (call_id, name, arguments, output_index) in events {
            self.push_event(
                output,
                json!({
                    "type": "response.function_call_arguments.done",
                    "response_id": response_id,
                    "item_id": call_id,
                    "output_index": output_index,
                    "call_id": call_id,
                    "name": name,
                    "arguments": arguments,
                }),
            )?;
            self.push_event(
                output,
                json!({
                    "type": "response.output_item.done",
                    "output_index": output_index,
                    "item": function_call_item(
                        &call_id,
                        &name,
                        &arguments,
                        "completed",
                    ),
                }),
            )?;
        }
        Ok(())
    }

    pub(super) fn current_output_items(&self) -> Vec<Value> {
        let mut items = Vec::new();
        if let Some(output_index) = self
            .reasoning_output_index
            .filter(|_| self.reasoning_started)
        {
            items.push((
                output_index,
                reasoning_item_with_status(
                    &self.reasoning_id,
                    &self.full_reasoning_text,
                    "completed",
                ),
            ));
        }
        if let Some(output_index) = self.message_output_index.filter(|_| self.content_started) {
            items.push((
                output_index,
                message_item_with_status(&self.message_id, &self.full_text, "completed"),
            ));
        }
        for state in &self.tool_calls {
            items.push((
                state.output_index,
                function_call_item(
                    &state.call_id,
                    state.name.as_deref().unwrap_or(""),
                    &state.arguments,
                    "completed",
                ),
            ));
        }
        items.sort_by_key(|(output_index, _)| *output_index);
        items.into_iter().map(|(_, item)| item).collect()
    }

    pub(super) fn allocate_output_index(&mut self) -> usize {
        let output_index = self.next_output_index;
        self.next_output_index += 1;
        output_index
    }

    pub(super) fn current_response_id(&self) -> String {
        self.response_id
            .clone()
            .unwrap_or_else(generate_response_id)
    }

    pub(super) fn push_event(
        &mut self,
        output: &mut Vec<Vec<u8>>,
        mut event: Value,
    ) -> Result<(), CompatError> {
        if let Some(object) = event.as_object_mut() {
            object.insert(
                "sequence_number".to_string(),
                Value::from(self.next_sequence_number as u64),
            );
        }
        self.next_sequence_number += 1;
        output.push(sse_event(&event)?);
        Ok(())
    }

    fn resolve_tool_call_position(&mut self, index: usize, call_id: Option<&str>) -> usize {
        if let Some(call_id) = call_id
            && let Some(position) = self
                .tool_calls
                .iter()
                .position(|state| state.call_id == call_id)
        {
            self.active_tool_call_positions.insert(index, position);
            return position;
        }

        if let Some(position) = self.active_tool_call_positions.get(&index).copied() {
            let state = self
                .tool_calls
                .get(position)
                .expect("active tool call position should exist");
            if call_id.is_none() || state.call_id == call_id.unwrap_or_default() {
                return position;
            }
        }

        let position = self.tool_calls.len();
        let call_id = call_id
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(generate_call_id);
        let output_index = self.allocate_output_index();
        self.tool_calls
            .push(StreamToolCallState::new(call_id, output_index));
        self.active_tool_call_positions.insert(index, position);
        position
    }
}
