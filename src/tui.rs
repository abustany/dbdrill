use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};
use cursive::View;
use cursive::view::{Nameable, Resizable};
use cursive::views::{self};
use jsonpath_rust::JsonPath;

use crate::model::{LinkSearchParam, Resource, SearchParamType};
use crate::sql_value_as_string::SQLValueAsString;

struct AppData {
    resources: HashMap<String, Resource>,
    db: postgres::Client,
}

type AppDataPtr = Arc<Mutex<AppData>>;

pub fn start(db: postgres::Client, resources: HashMap<String, Resource>) {
    let mut siv = cursive::default();
    siv.add_global_callback('q', |s| s.quit());

    let app_data_ptr = Arc::new(Mutex::new(AppData { resources, db }));
    let router = Router::new(Arc::clone(&app_data_ptr));
    router.push(&mut siv, Box::new(RouteResourcePicker {}));
    // show_resource_picker_dialog(app_data_ptr, &mut siv);
    siv.run();
}

fn is_consonnant(c: char) -> bool {
    !matches!(c, 'a' | 'e' | 'i' | 'o' | 'u')
}

fn assign_shortcuts<'a>(strs: impl IntoIterator<Item = &'a str>) -> Vec<Option<(usize, char)>> {
    let mut assigned: HashSet<char> = HashSet::new();
    let mut res: Vec<Option<(usize, char)>> = Vec::new();

    'outer: for s in strs {
        let mut is_prev_alphabetic = false;
        let word_starts = s.chars().enumerate().filter(|(_, c)| {
            let is_alphabetic = c.is_alphabetic();
            let is_word_start = is_alphabetic && !is_prev_alphabetic;
            is_prev_alphabetic = is_alphabetic;
            is_word_start
        });
        let consonnants = s
            .chars()
            .enumerate()
            .filter(|(_, c)| c.is_alphabetic() && is_consonnant(*c));
        let all_alphas = s.chars().enumerate().filter(|(_, c)| c.is_alphabetic());

        for (idx, c) in word_starts.chain(consonnants).chain(all_alphas) {
            let c = c.to_lowercase().next().expect("error lowercasing");
            if assigned.contains(&c) {
                continue;
            }

            assigned.insert(c);
            res.push(Some((idx, c)));
            continue 'outer;
        }

        res.push(None);
    }

    res
}

fn build_shortcut_select_view<T: 'static + Send + Sync + Clone>(
    mut v: views::SelectView<T>,
    name: &str,
) -> impl cursive::view::View {
    let shortcuts = assign_shortcuts(v.iter().map(|(label, _)| label));

    for ((label, _), shortcut) in v.iter_mut().zip(shortcuts.iter()) {
        let Some((idx, _)) = shortcut else {
            continue;
        };
        let txt = label.source().to_owned();
        label.remove_spans(0..label.spans_raw().len());
        label.append_plain(String::from_iter(txt.chars().take(*idx)));
        label.append_styled(
            String::from_iter(txt.chars().skip(*idx).take(1)),
            Into::<cursive::style::Style>::into(cursive::style::Effect::Bold)
                .combine(cursive::style::PaletteColor::Highlight),
        );
        label.append_plain(String::from_iter(txt.chars().skip(idx + 1)));
    }

    let mut res = views::OnEventView::new(v.with_name(name));

    for (idx, shortcut) in shortcuts.iter().enumerate() {
        let Some((_, c)) = shortcut else {
            continue;
        };
        let name = name.to_owned();
        res.set_on_event(cursive::event::Event::Char(*c), move |s| {
            if let Some(Some(cb)) = s.call_on_name(&name, |v: &mut views::SelectView| {
                v.set_selection(idx);
                if let cursive::event::EventResult::Consumed(Some(cb)) =
                    v.on_event(cursive::event::Event::Key(cursive::event::Key::Enter))
                {
                    Some(cb.clone())
                } else {
                    None
                }
            }) {
                cb(s);
            }
        });
    }

    res
}

trait Route {
    fn mount(&self, app_data_ptr: AppDataPtr, siv: &mut cursive::Cursive, router: &Router);
    fn unmount(&self, app_data_ptr: AppDataPtr, siv: &mut cursive::Cursive, router: &Router);
}

struct RouterContextData {
    history: Vec<Box<dyn Route + Send>>,
}

struct Router {
    app_data_ptr: AppDataPtr,
    data: Arc<Mutex<RouterContextData>>,
}

impl Router {
    fn new(app_data_ptr: AppDataPtr) -> Self {
        Router {
            app_data_ptr,
            data: Arc::new(Mutex::new(RouterContextData {
                history: Vec::new(),
            })),
        }
    }

    fn push(&self, siv: &mut cursive::Cursive, route: Box<dyn Route + Send>) {
        let mut ctx = self.data.lock().unwrap();
        if let Some(mounted_route) = ctx.history.last() {
            mounted_route.unmount(Arc::clone(&self.app_data_ptr), siv, &self.clone());
        }
        route.mount(Arc::clone(&self.app_data_ptr), siv, &self.clone());
        ctx.history.push(route);
    }

    fn pop(&self, siv: &mut cursive::Cursive) {
        let mut ctx = self.data.lock().unwrap();
        if let Some(route) = ctx.history.pop() {
            route.unmount(Arc::clone(&self.app_data_ptr), siv, &self.clone());
        }
        if let Some(route) = ctx.history.last() {
            route.mount(Arc::clone(&self.app_data_ptr), siv, &self.clone());
        } else {
            siv.quit();
        }
    }
}

impl Clone for Router {
    fn clone(&self) -> Self {
        Self {
            app_data_ptr: Arc::clone(&self.app_data_ptr),
            data: Arc::clone(&self.data),
        }
    }
}

struct RouteResourcePicker {}

impl Route for RouteResourcePicker {
    fn mount(&self, app_data_ptr: AppDataPtr, siv: &mut cursive::Cursive, router: &Router) {
        let router = router.clone();
        siv.add_layer(views::Dialog::around(
            views::OnEventView::new(build_resource_picker(Arc::clone(&app_data_ptr), &router))
                .on_event(cursive::event::Key::Esc, move |siv| {
                    router.pop(siv);
                }),
        ));
    }

    fn unmount(&self, _app_data_ptr: AppDataPtr, siv: &mut cursive::Cursive, _router: &Router) {
        siv.pop_layer();
    }
}

fn build_resource_picker(app_data_ptr: AppDataPtr, router: &Router) -> impl cursive::view::View {
    let mut select_view = views::SelectView::new();
    {
        let app_data = app_data_ptr.lock().unwrap();

        for (k, v) in &app_data.resources {
            select_view.add_item(v.name.as_str(), k.to_owned());
        }
    };

    select_view.sort_by_label();
    let router = router.clone();
    select_view.set_on_submit(move |siv, resource_id: &str| {
        router.push(
            siv,
            Box::new(SearchPickerRoute {
                resource_id: resource_id.to_owned(),
            }),
        )
    });

    views::LinearLayout::vertical()
        .child(views::TextView::new("Resources"))
        .child(build_shortcut_select_view(select_view, "resource_picker"))
}

struct SearchPickerRoute {
    resource_id: String,
}

impl Route for SearchPickerRoute {
    fn mount(&self, app_data_ptr: AppDataPtr, siv: &mut cursive::Cursive, router: &Router) {
        let router = router.clone();
        siv.add_layer(views::Dialog::around(
            views::OnEventView::new(build_search_picker(
                Arc::clone(&app_data_ptr),
                &router,
                &self.resource_id,
            ))
            .on_event(cursive::event::Key::Esc, move |siv| {
                router.pop(siv);
            }),
        ));
    }

    fn unmount(&self, _app_data_ptr: AppDataPtr, siv: &mut cursive::Cursive, _router: &Router) {
        siv.pop_layer();
    }
}

fn build_search_picker(
    app_data_ptr: AppDataPtr,
    router: &Router,
    resource_id: &str,
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

    for search in r.search.keys() {
        select_view.add_item_str(search);
    }

    select_view.sort_by_label();

    {
        let resource_id = resource_id.to_owned();
        let router = router.clone();
        select_view.set_on_submit(move |siv, search_id: &str| {
            router.push(
                siv,
                Box::new(QueryRoute {
                    resource_id: resource_id.clone(),
                    search_id: search_id.to_owned(),
                }),
            );
        });
    }

    let title = format!("Search {} by...", &r.name);

    views::LinearLayout::vertical()
        .child(views::TextView::new(&title))
        .child(build_shortcut_select_view(select_view, "search_picker"))
}

struct QueryRoute {
    resource_id: String,
    search_id: String,
}

impl Route for QueryRoute {
    fn mount(&self, app_data_ptr: AppDataPtr, siv: &mut cursive::Cursive, router: &Router) {
        let router = router.clone();
        siv.add_layer(views::Dialog::around(
            views::OnEventView::new(build_query(
                Arc::clone(&app_data_ptr),
                &router,
                &self.resource_id,
                &self.search_id,
            ))
            .on_event(cursive::event::Key::Esc, move |siv| {
                router.pop(siv);
            }),
        ));
    }

    fn unmount(&self, _app_data_ptr: AppDataPtr, siv: &mut cursive::Cursive, _router: &Router) {
        siv.pop_layer();
    }
}

fn build_query(
    app_data_ptr: AppDataPtr,
    router: &Router,
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
        let router = router.clone();

        layout.add_child(views::Button::new("Search", move |s| {
            on_query(
                Arc::clone(&app_data_ptr),
                s,
                &router,
                &resource_id,
                &search_id,
            )
        }));
    }

    layout
}

fn gather_query_parameter_strings(siv: &mut cursive::Cursive, param_names: &[&str]) -> Vec<String> {
    param_names
        .iter()
        .map(|name| {
            siv.call_on_name(name, |view: &mut views::EditView| view.get_content())
                .expect("missing param view")
                .as_ref()
                .clone()
        })
        .collect()
}

fn on_query_helper(
    app_data_ptr: AppDataPtr,
    resource_id: &str,
    search_id: &str,
    params_str_values: &[String],
) -> Result<(String, Vec<postgres::Row>)> {
    let r = {
        let app_data = app_data_ptr.lock().unwrap();
        app_data
            .resources
            .get(resource_id)
            .expect("invalid resource id")
            .clone()
    };
    let s = r.search.get(search_id).expect("invalid search id");
    let mut title = String::new();
    let mut param_values: Vec<Box<dyn postgres::types::ToSql + Sync>> = Vec::new();

    write!(&mut title, "{} / {} (", &r.name, search_id)?;

    for (idx, (param, str_val)) in s.params.iter().zip(params_str_values.iter()).enumerate() {
        if idx > 0 {
            write!(&mut title, ", ")?;
        }

        write!(&mut title, "{}={}", &param.name, &str_val)?;

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
            Some(SearchParamType::TextArray) => {
                let array_val: Vec<String> = str_val.split(',').map(|s| s.to_string()).collect();
                Box::new(array_val)
            }
        };
        param_values.push(val);
    }

    write!(&mut title, ")")?;

    let param_values_ref: Vec<&(dyn postgres::types::ToSql + Sync)> =
        param_values.iter().map(|v| v.as_ref()).collect();

    let mut app_data = app_data_ptr.lock().unwrap();
    let rows = app_data
        .db
        .query(&s.query, &param_values_ref)
        .context("error running SQL query")?;

    Ok((title, rows))
}

fn on_query(
    app_data_ptr: AppDataPtr,
    siv: &mut cursive::Cursive,
    router: &Router,
    resource_id: &str,
    search_id: &str,
) {
    let r = {
        let app_data = app_data_ptr.lock().unwrap();
        app_data
            .resources
            .get(resource_id)
            .expect("invalid resource id")
            .clone()
    };
    let s = r.search.get(search_id).expect("invalid search id");
    let param_names: Vec<&str> = s.params.iter().map(|p| p.name.as_str()).collect();

    match on_query_helper(
        Arc::clone(&app_data_ptr),
        resource_id,
        search_id,
        gather_query_parameter_strings(siv, param_names.as_slice()).as_slice(),
    ) {
        Ok((title, rows)) => {
            router.push(
                siv,
                Box::new(QueryResultsRoute {
                    resource_id: resource_id.to_owned(),
                    title,
                    rows,
                }),
            );
        }
        Err(err) => {
            eprintln!("Error running query: {err:?}");
            siv.add_layer(views::Dialog::around(build_query_error(&err)));
        }
    };
}

#[derive(Clone)]
struct ResultRow(postgres::Row);

type IndexedRow = (usize, ResultRow);

impl cursive_table_view::TableViewItem<TableColumn> for IndexedRow {
    fn to_column(&self, column: TableColumn) -> String {
        match column {
            TableColumn::Idx => self.0.to_string(),
            TableColumn::DBCol(column) => {
                let val: SQLValueAsString = self
                    .1
                    .0
                    .try_get(column)
                    .unwrap_or_else(|err| SQLValueAsString::new(err.to_string()));
                val.take_string()
            }
        }
    }

    fn cmp(&self, other: &Self, column: TableColumn) -> std::cmp::Ordering
    where
        Self: Sized,
    {
        match column {
            TableColumn::Idx => self.0.cmp(&other.0),
            TableColumn::DBCol(_) => {
                let self_val = self.to_column(column);
                let other_val = other.to_column(column);
                self_val.cmp(&other_val)
            }
        }
    }
}

fn col_size<'a>(rows: &'a [postgres::Row], col: usize) -> usize {
    let name_size = rows
        .first()
        .map(|row| row.columns()[col].name().len())
        .unwrap_or(0);
    let max_col_size = rows
        .iter()
        .map(|row| {
            row.try_get::<'a, usize, SQLValueAsString>(col)
                .map(|v| v.take_string().len())
                .unwrap_or(0)
        })
        .max()
        .unwrap_or(0);

    std::cmp::min(
        32, // clip to 32 chars
        std::cmp::max(name_size, max_col_size),
    )
}

struct QueryResultsRoute {
    resource_id: String,
    title: String,
    rows: Vec<postgres::Row>,
}

impl Route for QueryResultsRoute {
    fn mount(&self, app_data_ptr: AppDataPtr, siv: &mut cursive::Cursive, router: &Router) {
        let router = router.clone();
        siv.add_layer(views::Dialog::around(
            views::OnEventView::new(build_query_results(
                Arc::clone(&app_data_ptr),
                &router,
                &self.resource_id,
                &self.title,
                &self.rows,
            ))
            .on_event(cursive::event::Key::Esc, move |siv| {
                router.pop(siv);
            }),
        ));
    }

    fn unmount(&self, _app_data_ptr: AppDataPtr, siv: &mut cursive::Cursive, _router: &Router) {
        siv.pop_layer();
    }
}

#[derive(Clone, Copy, Hash, Eq, PartialEq)]
enum TableColumn {
    Idx,
    DBCol(usize),
}

fn build_query_results(
    app_data_ptr: AppDataPtr,
    router: &Router,
    resource_id: &str,
    title: &str,
    rows: &[postgres::Row],
) -> impl cursive::view::View {
    let mut table = cursive_table_view::TableView::<(usize, ResultRow), TableColumn>::new();

    if !rows.is_empty() {
        let first = &rows[0];

        table.add_column(TableColumn::Idx, "#", |col| {
            col.width((rows.len().ilog10() + 1) as usize)
        });

        for (idx, col) in first.columns().iter().enumerate() {
            table.add_column(TableColumn::DBCol(idx), col.name(), |col| {
                col.width(col_size(rows, idx))
            });
        }

        table.set_items(
            rows.iter()
                .enumerate()
                .map(|(idx, r)| (idx, ResultRow(r.clone())))
                .collect(),
        );
        table.set_on_submit(|siv: &mut cursive::Cursive, _row: usize, index: usize| {
            let (_, row) = siv
                .call_on_name(
                    "results",
                    |table: &mut cursive_table_view::TableView<IndexedRow, TableColumn>| {
                        table.borrow_item(index).unwrap().clone()
                    },
                )
                .expect("missing results view");
            siv.add_layer(views::Dialog::around(build_row_view(&row)));
        });
    }

    let table_with_events = {
        let resource_id = resource_id.to_owned();
        let router = router.clone();
        views::OnEventView::new(table.with_name("results")).on_event('l', move |siv| {
            if let Some((_, row)) = siv
                .call_on_name(
                    "results",
                    |table: &mut cursive_table_view::TableView<IndexedRow, TableColumn>| {
                        table
                            .item()
                            .map(|idx| table.borrow_item(idx).unwrap().clone())
                    },
                )
                .expect("missing results view")
            {
                on_show_links(Arc::clone(&app_data_ptr), siv, &router, &resource_id, &row);
            }
        })
    };

    views::LinearLayout::vertical()
        .child(views::TextView::new(format!("Query results: {title}")))
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
            Ok(v) => cursive::views::TextView::new(v.as_str()),
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
    router: &Router,
    resource_id: &str,
    row: &ResultRow,
) {
    siv.add_layer(views::Dialog::around(
        views::OnEventView::new(build_link_picker(
            Arc::clone(&app_data_ptr),
            router,
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
    router: &Router,
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

    for link in r.links.keys() {
        select_view.add_item_str(link);
    }

    select_view.sort_by_label();

    {
        let resource_id = resource_id.to_owned();
        let row = row.clone();
        let router = router.clone();
        select_view.set_on_submit(move |s, link_name| {
            on_pick_link(
                Arc::clone(&app_data_ptr),
                s,
                &router,
                &resource_id,
                link_name,
                &row,
            )
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
) -> Result<(String, String, Vec<postgres::Row>)> {
    let r = {
        let app_data = app_data_ptr.lock().unwrap();
        app_data
            .resources
            .get(resource_id)
            .expect("invalid resource id")
            .clone()
    };
    let links = r.links;
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

    let mut title = String::new();
    let mut param_values: Vec<Box<dyn postgres::types::ToSql + Sync>> = Vec::new();

    write!(&mut title, "{} (", &r.name)?;

    for (idx, (param, target_param)) in link
        .search_params
        .iter()
        .zip(link_search.params.iter())
        .enumerate()
    {
        let (param_value, title_item) = match param {
            LinkSearchParam::Name(name) => {
                let col = row
                    .0
                    .columns()
                    .iter()
                    .find(|col| col.name() == name)
                    .expect("invalid column name");
                let col_ty = col.type_();

                let val_title: SQLValueAsString = row
                    .0
                    .try_get(name.as_str())
                    .unwrap_or_else(|err| SQLValueAsString::new(err.to_string()));

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

                (val, val_title.take_string())
            }
            LinkSearchParam::JsonPath {
                col_and_path: (col_name, path),
            } => {
                let col_value_title: SQLValueAsString = row
                    .0
                    .try_get(col_name.as_str())
                    .unwrap_or_else(|err| SQLValueAsString::new(err.to_string()));
                let col_value: serde_json::Value = row
                    .0
                    .try_get(col_name.as_str())
                    .context("error parsing value as JSON")?;
                let results = col_value.query(path).context("error dereferencing value")?;

                let val: Box<dyn postgres::types::ToSql + Sync> = match target_param.ty {
                    Some(SearchParamType::Integer) => Box::new(
                        TryInto::<i32>::try_into(extract_single_value(&results)?
                                                                .as_i64()
                                                                .with_context(|| {
                                                                    format!("dereferenced value {:?} is not a number", results[0])
                                                                })?).with_context(|| format!("integer values overflows target type: {:?}", results[0]))?
                                                        )  ,
                    Some(SearchParamType::TextArray) => Box::new(
                                            results.into_iter().map(|val| val.as_str().with_context(|| {
                                                    format!("dereferenced value {val:?} is not a string", )
                                                }).map(|x| x.to_owned())
                                            ).collect::<Result<Vec<String>>>()?,
                                                                            ),
                    None /* text */ =>  Box::new(
                        extract_single_value(&results)?
                                            .as_str()
                                            .with_context(|| {
                                                format!("dereferenced value {:?} is not a string", results[0])
                                            })?
                                            .to_owned(),
                                    ) ,
                };

                (val, format!("{path}={}", col_value_title.take_string()))
            }
        };

        if idx > 0 {
            write!(&mut title, ", ")?;
        }

        write!(&mut title, "{title_item}")?;

        param_values.push(param_value);
    }

    write!(&mut title, ") → {link_name}")?;

    let param_values_ref: Vec<&(dyn postgres::types::ToSql + Sync)> =
        param_values.iter().map(|v| v.as_ref()).collect();

    let mut app_data = app_data_ptr.lock().unwrap();

    let rows = app_data
        .db
        .query(&link_search.query, &param_values_ref)
        .context("error running SQL query")?;

    Ok((link.kind.clone(), title, rows))
}

fn on_pick_link(
    app_data_ptr: AppDataPtr,
    siv: &mut cursive::Cursive,
    router: &Router,
    resource_id: &str,
    link_name: &str,
    row: &ResultRow,
) {
    siv.pop_layer(); // close the link picker
    match on_pick_link_helper(Arc::clone(&app_data_ptr), resource_id, link_name, row) {
        Ok((target_resource_id, title, rows)) => router.push(
            siv,
            Box::new(QueryResultsRoute {
                resource_id: target_resource_id,
                title,
                rows,
            }),
        ),
        Err(err) => {
            eprintln!("Error running link query: {err:?}");
            siv.add_layer(views::Dialog::around(build_query_error(&err)));
        }
    };
}

fn extract_single_value<'a>(vals: &[&'a serde_json::Value]) -> Result<&'a serde_json::Value> {
    match vals {
        [value] => Ok(value),
        _ => {
            bail!("expected 1 result, got {}", vals.len())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_assign_shortcuts() {
        let items: Vec<&str> = vec![
            "Case",
            "Case list",
            "Case list item",
            "Presentation",
            "Slide",
            "Space",
            "User",
        ];
        assert_eq!(
            assign_shortcuts(items),
            vec![
                Some((0, 'c')),
                Some((5, 'l')),
                Some((10, 'i')),
                Some((0, 'p')),
                Some((0, 's')),
                Some((2, 'a')),
                Some((0, 'u')),
            ]
        );
    }
}
