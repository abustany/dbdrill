use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::{collections::HashMap, fs};

use anyhow::{Context, Result, bail};
use clap::Parser;
use cursive::view::Nameable;
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

    let app_data = Arc::new(Mutex::new(AppData { resources, db }));

    let mut siv = cursive::default();
    siv.add_global_callback('q', |s| s.quit());

    siv.add_layer(views::Dialog::around(build_resource_picker(Arc::clone(
        &app_data,
    ))));

    siv.run();

    Ok(())
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
    siv.add_layer(views::Dialog::around(build_search_picker(
        app_data_ptr,
        resource_id,
    )));
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
    siv.add_layer(views::Dialog::around(build_query(
        app_data_ptr,
        resource_id,
        search_id,
    )));
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
    match on_query_helper(app_data_ptr, siv, resource_id, search_id) {
        Ok(rows) => {
            siv.pop_layer();
            siv.add_layer(views::Dialog::text(format!("Got {} rows", rows.len())));
        }
        Err(err) => {
            siv.add_layer(views::Dialog::around(build_query_error(&err)));
        }
    };
}

fn build_query_error(err: &anyhow::Error) -> impl cursive::view::View {
    views::LinearLayout::vertical()
        .child(views::TextView::new("Query Error"))
        .child(views::TextView::new(err.to_string()))
        .child(views::Button::new("OK", |s| {
            s.pop_layer();
        }))
}
