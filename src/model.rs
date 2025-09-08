use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub enum SearchParamType {
    #[serde(rename = "integer")]
    Integer,
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
pub enum LinkSearchParam {
    Name(String),
    JsonDeref { json_deref: Vec<String> },
}

#[derive(Clone, Debug, Deserialize)]
pub struct Link {
    pub kind: String,
    pub search: String,
    pub search_params: Vec<LinkSearchParam>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Resource {
    pub name: String,
    pub search: HashMap<String, Search>,
    pub links: Option<HashMap<String, Link>>,
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
        if let LinkSearchParam::JsonDeref { json_deref } = p {
            if json_deref.is_empty() {
                bail!("search param {idx} has an empty json_deref");
            }
        }
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
    for (resource_id, resource) in resources {
        if let Some(links) = &resource.links {
            validate_resource_links(resources, links)
                .with_context(|| format!("error validating {resource_id}.links"))?;
        }
    }
    Ok(())
}
