use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub enum SearchParamType {
    #[serde(rename = "integer")]
    Integer,
    #[serde(rename = "text[]")]
    TextArray,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SearchParam {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: Option<SearchParamType>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Search {
    pub query: String,
    pub params: Vec<SearchParam>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum ColumnExpression {
    Name(String),
    JsonPath {
        #[serde(rename = "json_path")]
        col_and_path: (String, String),
    },
}

#[derive(Clone, Debug, Deserialize)]
pub enum LinkCondition {
    #[serde(rename = "eq")]
    Eq(ColumnExpression, String),
}

#[derive(Clone, Debug, Deserialize)]
pub struct Link {
    pub kind: String,
    pub search: String,
    pub search_params: Vec<ColumnExpression>,
    #[serde(rename = "if")]
    pub condition: Option<LinkCondition>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Resource {
    pub name: String,
    #[serde(default)]
    pub search: HashMap<String, Search>,
    #[serde(default)]
    pub links: HashMap<String, Link>,
}

fn validate_resource_link(resources: &HashMap<String, Resource>, link: &Link) -> Result<()> {
    let Some(target_resource) = resources.get(&link.kind) else {
        bail!("link references a non existing resource {}", &link.kind);
    };

    let Some(target_search) = target_resource.search.get(&link.search) else {
        bail!(
            "referenced resource {} has no search named {}",
            &link.kind,
            &link.search
        );
    };

    if target_search.params.len() != link.search_params.len() {
        bail!(
            "referenced search {} has {} params but link specifies {}",
            &link.search,
            target_search.params.len(),
            link.search_params.len()
        );
    }

    for (idx, p) in link.search_params.iter().enumerate() {
        if let ColumnExpression::JsonPath {
            col_and_path: (_, path),
        } = p
        {
            jsonpath_rust::parser::parse_json_path(path).with_context(|| {
                format!("invalid JSONPath expression for search parameter {idx}")
            })?;
        }
    }

    if let Some(LinkCondition::Eq(
        ColumnExpression::JsonPath {
            col_and_path: (_, path),
        },
        _,
    )) = &link.condition
    {
        jsonpath_rust::parser::parse_json_path(path)
            .context("link condition (\"if\") is an invalid JSONPath expression")?;
    }

    Ok(())
}

fn validate_resource_links(
    resources: &HashMap<String, Resource>,
    links: &HashMap<String, Link>,
) -> Result<()> {
    for (link_name, link) in links {
        validate_resource_link(resources, link)
            .with_context(|| format!("error validating link {link_name}"))?;
    }
    Ok(())
}

pub fn validate_resources(resources: &HashMap<String, Resource>) -> Result<()> {
    let mut used_names: HashMap<&str, &str> = HashMap::new();

    for (resource_id, resource) in resources {
        if resource_id.is_empty() {
            bail!("resource identifiers can't be empty");
        }

        if resource.name.is_empty() {
            bail!("resource {resource_id} has an empty name");
        }

        if let Some(other_resource_id) = used_names.insert(&resource.name, resource_id) {
            bail!("resource {resource_id} has the same name as {other_resource_id}");
        }

        validate_resource_links(resources, &resource.links)
            .with_context(|| format!("error validating {resource_id}.links"))?;
    }
    Ok(())
}
