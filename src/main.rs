use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::{collections::HashMap, fs};

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use cursive::view::{Nameable, Resizable};
use cursive::views;
use serde::Deserialize;

#[derive(Parser)]
#[command(name = "dbdrill")]
#[command(about = "A PostgreSQL database drilling tool")]
#[command(version)]
struct Args {
    /// PostgreSQL database connection string (DSN)
    #[arg(
        help = "PostgreSQL database connection string (e.g., postgres://user:password@host:port/database)"
    )]
    db_dsn: String,

    /// Path to the TOML resources file
    #[arg(help = "Path to the TOML file containing resources configuration")]
    resources_file: PathBuf,
}

#[derive(Clone, Debug, Deserialize)]
enum SearchParamType {
    #[serde(rename = "integer")]
    Integer,
}

#[derive(Clone, Debug, Deserialize)]
struct SearchParam {
    name: String,
    #[serde(rename = "type")]
    ty: Option<SearchParamType>,
}

#[derive(Clone, Debug, Deserialize)]
struct Search {
    query: String,
    params: Vec<SearchParam>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
enum LinkSearchParam {
    Name(String),
    JsonDeref { json_deref: Vec<String> },
}

#[derive(Clone, Debug, Deserialize)]
struct Link {
    kind: String,
    search: String,
    search_params: Vec<LinkSearchParam>,
}

#[derive(Clone, Debug, Deserialize)]
struct Resource {
    name: String,
    search: HashMap<String, Search>,
    links: Option<HashMap<String, Link>>,
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

fn validate_resources(resources: &HashMap<String, Resource>) -> Result<()> {
    for (resource_id, resource) in resources {
        if let Some(links) = &resource.links {
            validate_resource_links(resources, links)
                .with_context(|| format!("error validating {resource_id}.links"))?;
        }
    }
    Ok(())
}

struct AppData {
    resources: HashMap<String, Resource>,
    db: postgres::Client,
}

type AppDataPtr = Arc<Mutex<AppData>>;

fn main() -> Result<()> {
    let args = Args::parse();

    println!("Database DSN: {}", args.db_dsn);
    println!("Resources file: {}", args.resources_file.display());

    let resources: HashMap<String, Resource> = toml::from_str(
        &fs::read_to_string(&args.resources_file).context("error opening resources file")?,
    )
    .context("error parsing resources files")?;

    validate_resources(&resources).context("error validating resources")?;

    println!("Connecting to the DB...");
    let db = postgres::Client::connect(&args.db_dsn, postgres::NoTls)
        .context("error connecting to DB")?;

    let app_data_ptr = Arc::new(Mutex::new(AppData { resources, db }));

    let mut siv = cursive::default();
    siv.add_global_callback('q', |s| s.quit());

    show_resource_picker_dialog(app_data_ptr, &mut siv);

    siv.run();

    Ok(())
}

fn show_resource_picker_dialog(app_data_ptr: AppDataPtr, siv: &mut cursive::Cursive) {
    siv.add_layer(views::Dialog::around(build_resource_picker(Arc::clone(
        &app_data_ptr,
    ))));
}

fn build_resource_picker(app_data_ptr: AppDataPtr) -> impl cursive::view::View {
    let mut select_view = views::SelectView::new();

    {
        let app_data = app_data_ptr.lock().unwrap();

        for (k, v) in &app_data.resources {
            select_view.add_item(&v.name, k.to_owned());
        }
    }

    select_view.sort_by_label();
    select_view.set_on_submit(move |s, resource_id| {
        on_pick_resource(Arc::clone(&app_data_ptr), s, resource_id)
    });

    views::LinearLayout::vertical()
        .child(views::TextView::new("Resources"))
        .child(select_view)
}

fn on_pick_resource(app_data_ptr: AppDataPtr, siv: &mut cursive::Cursive, resource_id: &str) {
    siv.pop_layer();
    show_search_picker_dialog(app_data_ptr, siv, resource_id);
}

fn show_search_picker_dialog(
    app_data_ptr: AppDataPtr,
    siv: &mut cursive::Cursive,
    resource_id: &str,
) {
    siv.add_layer(views::Dialog::around(
        views::OnEventView::new(build_search_picker(Arc::clone(&app_data_ptr), resource_id))
            .on_event(cursive::event::Key::Esc, move |siv| {
                siv.pop_layer();
                show_resource_picker_dialog(Arc::clone(&app_data_ptr), siv);
            }),
    ));
}

fn build_search_picker(app_data_ptr: AppDataPtr, resource_id: &str) -> impl cursive::view::View {
    let mut select_view = views::SelectView::new();

    let r = {
        let app_data = app_data_ptr.lock().unwrap();
        app_data
            .resources
            .get(resource_id)
            .expect("invalid resource id")
            .clone()
    };

    for search in r.search.keys() {
        select_view.add_item_str(search);
    }

    select_view.sort_by_label();

    {
        let resource_id = resource_id.to_owned();
        select_view.set_on_submit(move |s, search| {
            on_pick_search(Arc::clone(&app_data_ptr), s, &resource_id, search)
        });
    }

    let title = format!("Search {} by...", &r.name);

    views::LinearLayout::vertical()
        .child(views::TextView::new(&title))
        .child(select_view)
}

fn on_pick_search(
    app_data_ptr: AppDataPtr,
    siv: &mut cursive::Cursive,
    resource_id: &str,
    search_id: &str,
) {
    siv.pop_layer();
    show_query_dialog(app_data_ptr, siv, resource_id, search_id);
}

fn show_query_dialog(
    app_data_ptr: AppDataPtr,
    siv: &mut cursive::Cursive,
    resource_id: &str,
    search_id: &str,
) {
    let resource_id = resource_id.to_owned();
    siv.add_layer(views::Dialog::around(
        views::OnEventView::new(build_query(
            Arc::clone(&app_data_ptr),
            &resource_id,
            search_id,
        ))
        .on_event(cursive::event::Key::Esc, move |siv| {
            siv.pop_layer();
            show_search_picker_dialog(Arc::clone(&app_data_ptr), siv, &resource_id);
        }),
    ));
}

fn build_query(
    app_data_ptr: AppDataPtr,
    resource_id: &str,
    search_id: &str,
) -> impl cursive::view::View {
    let r = {
        let app_data = app_data_ptr.lock().unwrap();
        app_data
            .resources
            .get(resource_id)
            .expect("invalid resource id")
            .clone()
    };

    let s = r.search.get(search_id).expect("invalid search id");

    let title = format!("Search {} by {}", &r.name, search_id);
    let mut layout = views::LinearLayout::vertical().child(views::TextView::new(&title));

    for param in &s.params {
        let input = views::EditView::new().with_name(&param.name);
        layout.add_child(views::Panel::new(input).title(&param.name));
    }

    {
        let resource_id = resource_id.to_owned();
        let search_id = search_id.to_owned();

        layout.add_child(views::Button::new("Search", move |s| {
            on_query(Arc::clone(&app_data_ptr), s, &resource_id, &search_id)
        }));
    }

    layout
}

fn on_query_helper(
    app_data_ptr: AppDataPtr,
    siv: &mut cursive::Cursive,
    resource_id: &str,
    search_id: &str,
) -> Result<Vec<postgres::Row>> {
    let r = {
        let app_data = app_data_ptr.lock().unwrap();
        app_data
            .resources
            .get(resource_id)
            .expect("invalid resource id")
            .clone()
    };
    let s = r.search.get(search_id).expect("invalid search id");
    let mut param_values: Vec<Box<dyn postgres::types::ToSql + Sync>> = Vec::new();

    for param in &s.params {
        let str_val: String = siv
            .call_on_name(&param.name, |view: &mut views::EditView| view.get_content())
            .expect("missing param view")
            .as_ref()
            .clone();
        let val: Box<dyn postgres::types::ToSql + Sync> = match param.ty {
            None => Box::new(str_val),
            Some(SearchParamType::Integer) => {
                let integer_val: i32 = str_val.parse().with_context(|| {
                    format!(
                        "error parsing parameter {} as string: {}",
                        param.name, str_val
                    )
                })?;
                Box::new(integer_val)
            }
        };
        param_values.push(val);
    }

    let param_values_ref: Vec<&(dyn postgres::types::ToSql + Sync)> =
        param_values.iter().map(|v| v.as_ref()).collect();

    let mut app_data = app_data_ptr.lock().unwrap();
    app_data
        .db
        .query(&s.query, &param_values_ref)
        .context("error running SQL query")
}

fn on_query(
    app_data_ptr: AppDataPtr,
    siv: &mut cursive::Cursive,
    resource_id: &str,
    search_id: &str,
) {
    match on_query_helper(Arc::clone(&app_data_ptr), siv, resource_id, search_id) {
        Ok(rows) => {
            siv.pop_layer();
            show_query_results_dialog(
                Arc::clone(&app_data_ptr),
                siv,
                resource_id,
                search_id,
                &rows,
            );
        }
        Err(err) => {
            siv.add_layer(views::Dialog::around(build_query_error(&err)));
        }
    };
}

struct SQLValueAsString(String);

impl<T: std::fmt::Display> From<T> for SQLValueAsString {
    fn from(value: T) -> Self {
        SQLValueAsString(value.to_string())
    }
}

impl postgres::types::FromSql<'_> for SQLValueAsString {
    fn from_sql(
        ty: &postgres::types::Type,
        raw: &'_ [u8],
    ) -> std::result::Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        if ty == &postgres::types::Type::INT2 {
            return Ok(SQLValueAsString::from(i16::from_sql(ty, raw)?));
        }

        if ty == &postgres::types::Type::INT4 {
            return Ok(SQLValueAsString::from(i32::from_sql(ty, raw)?));
        }

        if ty == &postgres::types::Type::INT8 {
            return Ok(SQLValueAsString::from(i64::from_sql(ty, raw)?));
        }

        if ty == &postgres::types::Type::JSONB {
            return Ok(SQLValueAsString::from(serde_json::Value::from_sql(
                ty, raw,
            )?));
        }

        if ty == &postgres::types::Type::TEXT {
            return Ok(SQLValueAsString::from(String::from_sql(ty, raw)?));
        }

        if ty == &postgres::types::Type::TIMESTAMPTZ {
            return Ok(SQLValueAsString::from(jiff::Timestamp::from_sql(ty, raw)?));
        }

        Err(anyhow!("unsupported type: {}", ty).into_boxed_dyn_error())
    }

    fn from_sql_nullable(
        ty: &postgres::types::Type,
        raw: Option<&'_ [u8]>,
    ) -> std::result::Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        match raw {
            Some(val) => Self::from_sql(ty, val),
            None => Ok(SQLValueAsString(String::from("<NULL>"))),
        }
    }

    fn accepts(ty: &postgres::types::Type) -> bool {
        ty == &postgres::types::Type::INT2
            || ty == &postgres::types::Type::INT4
            || ty == &postgres::types::Type::INT8
            || ty == &postgres::types::Type::JSONB
            || ty == &postgres::types::Type::TEXT
            || ty == &postgres::types::Type::TIMESTAMPTZ
    }
}

#[derive(Clone)]
struct ResultRow(postgres::Row);

impl cursive_table_view::TableViewItem<usize> for ResultRow {
    fn to_column(&self, column: usize) -> String {
        let val: SQLValueAsString = self
            .0
            .try_get(column)
            .unwrap_or_else(|err| SQLValueAsString(err.to_string()));
        val.0
    }

    fn cmp(&self, other: &Self, column: usize) -> std::cmp::Ordering
    where
        Self: Sized,
    {
        let self_val = self.to_column(column);
        let other_val = other.to_column(column);
        self_val.cmp(&other_val)
    }
}

fn col_size<'a>(rows: &'a [postgres::Row], col: usize) -> usize {
    if rows.is_empty() {
        return 0;
    }

    let first = &rows[0];
    let mut res = first.columns()[col].name().len();

    for row in rows {
        res = std::cmp::min(
            32,
            std::cmp::max(
                res,
                row.try_get::<'a, usize, SQLValueAsString>(col)
                    .map(|v| v.0.len())
                    .unwrap_or(0),
            ),
        );
    }

    res
}

fn show_query_results_dialog(
    app_data_ptr: AppDataPtr,
    siv: &mut cursive::Cursive,
    resource_id: &str,
    search_id: &str,
    rows: &[postgres::Row],
) {
    let resource_id = resource_id.to_owned();
    let search_id = search_id.to_owned();
    siv.add_layer(views::Dialog::around(
        views::OnEventView::new(build_query_results(
            Arc::clone(&app_data_ptr),
            &resource_id,
            rows,
        ))
        .on_event(cursive::event::Key::Esc, move |siv| {
            siv.pop_layer();
            show_query_dialog(Arc::clone(&app_data_ptr), siv, &resource_id, &search_id);
        }),
    ));
}

fn build_query_results(
    app_data_ptr: AppDataPtr,
    resource_id: &str,
    rows: &[postgres::Row],
) -> impl cursive::view::View {
    let mut table = cursive_table_view::TableView::<ResultRow, usize>::new();

    if !rows.is_empty() {
        let first = &rows[0];

        for (idx, col) in first.columns().iter().enumerate() {
            table.add_column(idx, col.name(), |col| col.width(col_size(rows, idx)));
        }

        table.set_items(rows.iter().map(|r| ResultRow(r.clone())).collect());
        table.set_on_submit(|siv: &mut cursive::Cursive, _row: usize, index: usize| {
            let row = siv
                .call_on_name(
                    "results",
                    |table: &mut cursive_table_view::TableView<ResultRow, usize>| {
                        table.borrow_item(index).unwrap().clone()
                    },
                )
                .expect("missing results view");
            siv.add_layer(views::Dialog::around(build_row_view(&row)));
        });
    }

    let table_with_events = {
        let resource_id = resource_id.to_owned();
        views::OnEventView::new(table.with_name("results")).on_event('l', move |siv| {
            if let Some(row) = siv
                .call_on_name(
                    "results",
                    |table: &mut cursive_table_view::TableView<ResultRow, usize>| {
                        table
                            .item()
                            .map(|idx| table.borrow_item(idx).unwrap().clone())
                    },
                )
                .expect("missing results view")
            {
                on_show_links(Arc::clone(&app_data_ptr), siv, &resource_id, &row);
            }
        })
    };

    views::LinearLayout::vertical()
        .child(views::TextView::new("Query results"))
        .child(table_with_events.full_screen())
}

fn build_query_error(err: &anyhow::Error) -> impl cursive::view::View {
    views::LinearLayout::vertical()
        .child(views::TextView::new("Query Error"))
        .child(views::TextView::new(err.to_string()))
        .child(views::Button::new("OK", |s| {
            s.pop_layer();
        }))
}

fn build_row_view<'a>(row: &'a ResultRow) -> impl cursive::view::View {
    let row = &row.0;
    let mut values = views::LinearLayout::vertical();

    for (idx, col) in row.columns().iter().enumerate() {
        let view = match row.try_get::<'a, usize, SQLValueAsString>(idx) {
            Ok(v) => cursive::views::TextView::new(&v.0),
            Err(err) => cursive::views::TextView::new(err.to_string()),
        };
        values.add_child(views::Panel::new(view).title(col.name()));
    }

    views::LinearLayout::vertical()
        .child(values)
        .child(views::Button::new("Close", |s| {
            s.pop_layer();
        }))
}

fn on_show_links(
    app_data_ptr: AppDataPtr,
    siv: &mut cursive::Cursive,
    resource_id: &str,
    row: &ResultRow,
) {
    siv.add_layer(views::Dialog::around(
        views::OnEventView::new(build_link_picker(
            Arc::clone(&app_data_ptr),
            resource_id,
            row,
        ))
        .on_event(cursive::event::Key::Esc, |siv| {
            siv.pop_layer();
        }),
    ));
}

fn build_link_picker(
    app_data_ptr: AppDataPtr,
    resource_id: &str,
    row: &ResultRow,
) -> impl cursive::view::View {
    let mut select_view = views::SelectView::new();

    let r = {
        let app_data = app_data_ptr.lock().unwrap();
        app_data
            .resources
            .get(resource_id)
            .expect("invalid resource id")
            .clone()
    };

    for link in r.links.unwrap_or_default().keys() {
        select_view.add_item_str(link);
    }

    select_view.sort_by_label();

    {
        let resource_id = resource_id.to_owned();
        let row = row.clone();
        select_view.set_on_submit(move |s, link_name| {
            on_pick_link(Arc::clone(&app_data_ptr), s, &resource_id, link_name, &row)
        });
    }

    views::LinearLayout::vertical()
        .child(views::TextView::new("Links"))
        .child(select_view)
}

fn on_pick_link_helper(
    app_data_ptr: AppDataPtr,
    resource_id: &str,
    link_name: &str,
    row: &ResultRow,
) -> Result<Vec<postgres::Row>> {
    let r = {
        let app_data = app_data_ptr.lock().unwrap();
        app_data
            .resources
            .get(resource_id)
            .expect("invalid resource id")
            .clone()
    };
    let links = r.links.unwrap_or_default();
    let link = links.get(link_name).expect("invalid link name");
    let link_target_resource = {
        let app_data = app_data_ptr.lock().unwrap();
        app_data
            .resources
            .get(&link.kind)
            .expect("invalid link kind")
            .clone()
    };
    let link_search = link_target_resource
        .search
        .get(&link.search)
        .expect("invalid link search name");

    let mut param_values: Vec<Box<dyn postgres::types::ToSql + Sync>> = Vec::new();

    for param in &link.search_params {
        param_values.push(match param {
            LinkSearchParam::Name(name) => {
                let col = row
                    .0
                    .columns()
                    .iter()
                    .find(|col| col.name() == name)
                    .expect("invalid column name");
                let col_ty = col.type_();

                let val: Box<dyn postgres::types::ToSql + Sync> =
                    if col_ty == &postgres::types::Type::TEXT {
                        let val: Option<String> = row.0.get(name.as_str());
                        Box::new(val)
                    } else if col_ty == &postgres::types::Type::INT4 {
                        let val: Option<i32> = row.0.get(name.as_str());
                        Box::new(val)
                    } else {
                        todo!();
                    };

                val
            }
            LinkSearchParam::JsonDeref { json_deref } => todo!(),
        });
    }

    eprintln!("{} {:?}", link_search.query, &param_values);

    let param_values_ref: Vec<&(dyn postgres::types::ToSql + Sync)> =
        param_values.iter().map(|v| v.as_ref()).collect();

    let mut app_data = app_data_ptr.lock().unwrap();

    app_data
        .db
        .query(&link_search.query, &param_values_ref)
        .context("error running SQL query")
}

fn on_pick_link(
    app_data_ptr: AppDataPtr,
    siv: &mut cursive::Cursive,
    resource_id: &str,
    link_name: &str,
    row: &ResultRow,
) {
    match on_pick_link_helper(Arc::clone(&app_data_ptr), resource_id, link_name, row) {
        Ok(rows) => {
            siv.pop_layer();
            siv.add_layer(views::Dialog::around(build_query_results(
                Arc::clone(&app_data_ptr),
                resource_id,
                &rows,
            )));
        }
        Err(err) => {
            siv.add_layer(views::Dialog::around(build_query_error(&err)));
        }
    };
}
