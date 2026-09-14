use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::model::{Resource, validate_resources};

pub fn load(path: &Path) -> Result<HashMap<String, Resource>> {
    let resources: HashMap<String, Resource> =
        toml::from_str(&fs::read_to_string(path).context("error opening resources file")?)
            .context("error parsing resources files")?;

    validate_resources(&resources).context("error validating resources")?;

    Ok(resources)
}
