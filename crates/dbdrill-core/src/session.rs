use std::collections::HashMap;
use std::fmt::Write;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use jsonpath_rust::JsonPath;

use crate::connect::connect;
use crate::json_helpers::extract_single_value;
use crate::model::{ColumnExpression, Link, LinkCondition, Resource, Search, SearchParamType};
use crate::to_sql::{sql_value_from_json_slice, sql_value_from_string};
use crate::value::{ResultSet, Row, Value};

/// The resources a [`Session`] can query, keyed by resource identifier.
pub type Resources = HashMap<String, Resource>;

/// The rows returned by a search or a link, and where they came from.
pub struct QueryOutcome {
    /// Identifier of the resource the rows belong to.
    pub resource_id: String,
    /// Human readable description of the query that produced the rows.
    pub title: String,
    pub rows: ResultSet,
}

/// A database connection, and the resources reachable through it.
///
/// Queries block the calling thread and run one at a time. Callers that must
/// stay responsive are expected to drive a session from a thread of their own.
pub struct Session {
    db: postgres::Client,
    resources: Arc<Resources>,
}

impl Session {
    pub fn new(db: postgres::Client, resources: Arc<Resources>) -> Self {
        Session { db, resources }
    }

    /// Connects to `db_dsn` and returns a session querying `resources`.
    pub fn connect(db_dsn: &str, resources: Arc<Resources>) -> Result<Self> {
        Ok(Session::new(connect(db_dsn)?, resources))
    }

    pub fn resources(&self) -> &Arc<Resources> {
        &self.resources
    }

    /// Runs the search `search_id` of `resource_id`.
    ///
    /// `params` holds the raw, user supplied value of each search parameter, in
    /// the order the search declares them.
    pub fn run_search(
        &mut self,
        resource_id: &str,
        search_id: &str,
        params: &[String],
    ) -> Result<QueryOutcome> {
        let resources = Arc::clone(&self.resources);
        let resource = get_resource(&resources, resource_id)?;
        let search = resource
            .search
            .get(search_id)
            .with_context(|| format!("resource {resource_id} has no search named {search_id}"))?;

        let mut title = String::new();
        let mut values: Vec<Value> = Vec::with_capacity(search.params.len());

        write!(&mut title, "{} / {} (", resource.name, search_id)?;

        for (idx, (param, str_val)) in search.params.iter().zip(params.iter()).enumerate() {
            if idx > 0 {
                write!(&mut title, ", ")?;
            }

            write!(&mut title, "{}={}", param.name, str_val)?;

            values.push(
                sql_value_from_string(str_val, param.ty.clone().unwrap_or(SearchParamType::Text))
                    .with_context(|| format!("error parsing parameter {}", param.name))?,
            );
        }

        write!(&mut title, ")")?;

        let rows = self.query(&search.query, &values)?;

        Ok(QueryOutcome {
            resource_id: resource_id.to_owned(),
            title,
            rows,
        })
    }

    /// Follows the link `link_name` of `resource_id`, using `row` to fill in the
    /// parameters of the search the link points at.
    pub fn follow_link(
        &mut self,
        resource_id: &str,
        link_name: &str,
        row: &Row,
    ) -> Result<QueryOutcome> {
        let resources = Arc::clone(&self.resources);
        let resource = get_resource(&resources, resource_id)?;
        let link = resource
            .links
            .get(link_name)
            .with_context(|| format!("resource {resource_id} has no link named {link_name}"))?;
        let target = get_resource(&resources, &link.kind)?;
        let search = target.search.get(&link.search).with_context(|| {
            format!("resource {} has no search named {}", link.kind, link.search)
        })?;

        let (param_titles, values) = link_params(link, search, row)?;

        let mut title = String::new();
        write!(
            &mut title,
            "{} ({}) → {link_name}",
            resource.name,
            param_titles.join(", ")
        )?;

        let rows = self.query(&search.query, &values)?;

        Ok(QueryOutcome {
            resource_id: link.kind.clone(),
            title,
            rows,
        })
    }

    fn query(&mut self, sql: &str, params: &[Value]) -> Result<ResultSet> {
        let params: Vec<&(dyn postgres::types::ToSql + Sync)> = params
            .iter()
            .map(|value| value as &(dyn postgres::types::ToSql + Sync))
            .collect();

        let rows = self
            .db
            .query(sql, &params)
            .context("error running SQL query")?;

        Ok(ResultSet::from_postgres_rows(&rows))
    }
}

fn get_resource<'a>(resources: &'a Resources, resource_id: &str) -> Result<&'a Resource> {
    resources
        .get(resource_id)
        .with_context(|| format!("unknown resource {resource_id}"))
}

/// Builds the parameters of the search a link points at, out of a source row.
///
/// Returns the value of each parameter along with a description of where it
/// came from, used to label the resulting rows.
fn link_params(link: &Link, search: &Search, row: &Row) -> Result<(Vec<String>, Vec<Value>)> {
    let mut titles = Vec::with_capacity(link.search_params.len());
    let mut values = Vec::with_capacity(link.search_params.len());

    for (param, target_param) in link.search_params.iter().zip(search.params.iter()) {
        match param {
            ColumnExpression::Name(name) => {
                let value = column_value(row, name)?;
                titles.push(value.to_string());
                values.push(value.clone());
            }
            ColumnExpression::JsonPath {
                col_and_path: (col_name, path),
            } => {
                let value = column_value(row, col_name)?;
                let results = json_column(value, col_name)?
                    .query(path)
                    .context("error dereferencing value")?;

                titles.push(format!("{path}={value}"));
                values.push(sql_value_from_json_slice(
                    &results,
                    target_param.ty.clone().unwrap_or(SearchParamType::Text),
                )?);
            }
        }
    }

    Ok((titles, values))
}

/// Tells whether a link applies to `row`.
///
/// Links without a condition always apply.
pub fn evaluate_link_condition(cond: Option<&LinkCondition>, row: &Row) -> Result<bool> {
    let Some(cond) = cond else {
        return Ok(true);
    };

    let matches = match cond {
        LinkCondition::Eq(ColumnExpression::Name(col_name), expected) => {
            &column_value(row, col_name)?.to_string() == expected
        }
        LinkCondition::Eq(
            ColumnExpression::JsonPath {
                col_and_path: (col_name, path),
            },
            expected,
        ) => {
            let value = column_value(row, col_name)?;
            let results = json_column(value, col_name)?
                .query(path)
                .context("error evaluating JSONPath")?;
            let val_str = extract_single_value(&results)?
                .as_str()
                .with_context(|| format!("dereferenced value {:?} is not a string", results[0]))?;

            val_str == expected
        }
    };

    Ok(matches)
}

fn column_value<'a>(row: &'a Row, col_name: &str) -> Result<&'a Value> {
    row.get_by_name(col_name)
        .with_context(|| format!("no column named {col_name} in the result"))
}

fn json_column<'a>(value: &'a Value, col_name: &str) -> Result<&'a serde_json::Value> {
    match value {
        Value::Json(json) => Ok(json),
        Value::Error(msg) => bail!("error decoding column {col_name}: {msg}"),
        _ => bail!("column {col_name} does not hold JSON"),
    }
}

#[cfg(test)]
mod tests {
    use postgres::types::Type;

    use super::*;
    use crate::value::Column;

    fn row(columns: &[(&str, Type)], values: Vec<Value>) -> Row {
        let columns = columns
            .iter()
            .map(|(name, ty)| Column {
                name: (*name).to_owned(),
                ty: ty.clone(),
            })
            .collect();

        ResultSet::new(columns, vec![values]).rows()[0].clone()
    }

    fn eq_column(col_name: &str, expected: &str) -> LinkCondition {
        LinkCondition::Eq(
            ColumnExpression::Name(col_name.to_owned()),
            expected.to_owned(),
        )
    }

    fn eq_json_path(col_name: &str, path: &str, expected: &str) -> LinkCondition {
        LinkCondition::Eq(
            ColumnExpression::JsonPath {
                col_and_path: (col_name.to_owned(), path.to_owned()),
            },
            expected.to_owned(),
        )
    }

    #[test]
    fn links_without_a_condition_always_apply() {
        let row = row(&[("id", Type::INT4)], vec![Value::Int4(1)]);

        assert!(evaluate_link_condition(None, &row).unwrap());
    }

    #[test]
    fn column_conditions_compare_the_displayed_value() {
        let row = row(
            &[("kind", Type::TEXT)],
            vec![Value::Text("blog".to_owned())],
        );

        assert!(evaluate_link_condition(Some(&eq_column("kind", "blog")), &row).unwrap());
        assert!(!evaluate_link_condition(Some(&eq_column("kind", "post")), &row).unwrap());
    }

    #[test]
    fn column_conditions_work_on_non_text_columns() {
        let row = row(&[("id", Type::INT4)], vec![Value::Int4(7)]);

        assert!(evaluate_link_condition(Some(&eq_column("id", "7")), &row).unwrap());
        assert!(!evaluate_link_condition(Some(&eq_column("id", "8")), &row).unwrap());
    }

    #[test]
    fn conditions_on_an_unknown_column_are_an_error() {
        let row = row(
            &[("kind", Type::TEXT)],
            vec![Value::Text("blog".to_owned())],
        );

        let err = evaluate_link_condition(Some(&eq_column("nope", "blog")), &row)
            .expect_err("matched an unknown column");

        assert!(err.to_string().contains("no column named nope"), "{err}");
    }

    #[test]
    fn json_path_conditions_dereference_the_column() {
        let row = row(
            &[("meta", Type::JSONB)],
            vec![Value::Json(serde_json::json!({"kind": "blog"}))],
        );

        assert!(
            evaluate_link_condition(Some(&eq_json_path("meta", "$.kind", "blog")), &row).unwrap()
        );
        assert!(
            !evaluate_link_condition(Some(&eq_json_path("meta", "$.kind", "post")), &row).unwrap()
        );
    }

    #[test]
    fn json_path_conditions_need_a_json_column() {
        let row = row(&[("meta", Type::TEXT)], vec![Value::Text("{}".to_owned())]);

        let err = evaluate_link_condition(Some(&eq_json_path("meta", "$.kind", "blog")), &row)
            .expect_err("dereferenced a text column");

        assert!(err.to_string().contains("does not hold JSON"), "{err}");
    }

    #[test]
    fn json_path_conditions_report_undecodable_columns() {
        let row = row(
            &[("meta", Type::JSONB)],
            vec![Value::Error("unsupported type: point".to_owned())],
        );

        let err = evaluate_link_condition(Some(&eq_json_path("meta", "$.kind", "blog")), &row)
            .expect_err("dereferenced an undecodable column");

        assert!(err.to_string().contains("unsupported type: point"), "{err}");
    }
}
