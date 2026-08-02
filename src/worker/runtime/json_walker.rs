use anyhow::Result;
use serde_json::Value;

pub(crate) struct JsonStringContext<'a> {
    pub(crate) json_path: &'a str,
    pub(crate) field_name: Option<&'a str>,
    pub(crate) object_type: Option<&'a str>,
}

pub(crate) fn walk_json_strings(
    value: &mut Value,
    visitor: impl FnMut(JsonStringContext<'_>, &str) -> Result<Option<String>>,
) -> Result<()> {
    let mut visitor = visitor;
    let mut json_path = String::new();
    walk_json_value(value, &mut json_path, None, None, &mut visitor)
}

fn walk_json_value<F>(
    value: &mut Value,
    json_path: &mut String,
    field_name: Option<&str>,
    object_type: Option<&str>,
    visitor: &mut F,
) -> Result<()>
where
    F: FnMut(JsonStringContext<'_>, &str) -> Result<Option<String>>,
{
    match value {
        Value::String(text) => {
            if let Some(replacement) = visitor(
                JsonStringContext {
                    json_path,
                    field_name,
                    object_type,
                },
                text,
            )? {
                *text = replacement;
            }
        }
        Value::Array(items) => {
            let base_len = json_path.len();
            for (index, item) in items.iter_mut().enumerate() {
                json_path.push('/');
                json_path.push_str(&index.to_string());
                walk_json_value(item, json_path, None, None, visitor)?;
                json_path.truncate(base_len);
            }
        }
        Value::Object(object) => {
            let object_type = object
                .get("type")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let base_len = json_path.len();
            for (key, child) in object.iter_mut() {
                json_path.push('/');
                json_path.push_str(key);
                walk_json_value(child, json_path, Some(key), object_type.as_deref(), visitor)?;
                json_path.truncate(base_len);
            }
        }
        _ => {}
    }
    Ok(())
}
