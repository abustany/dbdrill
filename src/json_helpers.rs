use anyhow::{Result, bail};

pub fn extract_single_value<'a>(vals: &[&'a serde_json::Value]) -> Result<&'a serde_json::Value> {
    match vals {
        [value] => Ok(value),
        _ => {
            bail!("expected 1 result, got {}", vals.len())
        }
    }
}
