use std::sync::{Arc, Mutex};

use cursive::View;
use cursive::view::{Nameable, Resizable};
use cursive::views::{self};

use dbdrill_core::model::Resource;
use dbdrill_core::session::{QueryOutcome, Session, evaluate_link_condition};
use dbdrill_core::shortcuts::assign_shortcuts;
use dbdrill_core::value::{ResultSet, Row};

type AppDataPtr = Arc<Mutex<Session>>;

pub fn start(session: Session) {
    let mut siv = cursive::default();
    siv.add_global_callback('q', |s| s.quit());

    let app_data_ptr = Arc::new(Mutex::new(session));
    let router = Router::new(Arc::clone(&app_data_ptr));
    router.push(&mut siv, Box::new(RouteResourcePicker {}));
    // show_resource_picker_dialog(app_data_ptr, &mut siv);
    siv.run();
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

fn get_resource(app_data_ptr: &AppDataPtr, resource_id: &str) -> Resource {
    let session = app_data_ptr.lock().unwrap();
    session
        .resources()
        .get(resource_id)
        .expect("invalid resource id")
        .clone()
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
        let session = app_data_ptr.lock().unwrap();

        for (k, v) in session.resources().iter() {
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

    let r = get_resource(&app_data_ptr, resource_id);

    for search in r.search.keys() {
        select_view.add_item_str(search);
    }

    select_view.sort_by_label();

    {
        let resource_id = resource_id.to_owned();
        let router = router.clone();
        select_view.set_on_submit(move |siv, search_id: &str| {
            let r = get_resource(&app_data_ptr, &resource_id);
            let s = r.search.get(search_id).expect("invalid search id");

            if s.params.is_empty() {
                on_query(
                    Arc::clone(&app_data_ptr),
                    siv,
                    &router,
                    &resource_id,
                    search_id,
                );
            } else {
                router.push(
                    siv,
                    Box::new(QueryRoute {
                        resource_id: resource_id.clone(),
                        search_id: search_id.to_owned(),
                    }),
                );
            }
        });
    }

    let title = format!("Search {} by...", r.name);

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
    let r = get_resource(&app_data_ptr, resource_id);
    let s = r.search.get(search_id).expect("invalid search id");

    let title = format!("Search {} by {}", r.name, search_id);
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

fn on_query(
    app_data_ptr: AppDataPtr,
    siv: &mut cursive::Cursive,
    router: &Router,
    resource_id: &str,
    search_id: &str,
) {
    let r = get_resource(&app_data_ptr, resource_id);
    let s = r.search.get(search_id).expect("invalid search id");
    let param_names: Vec<&str> = s.params.iter().map(|p| p.name.as_str()).collect();
    let params = gather_query_parameter_strings(siv, param_names.as_slice());

    let outcome =
        app_data_ptr
            .lock()
            .unwrap()
            .run_search(resource_id, search_id, params.as_slice());

    on_query_outcome(siv, router, outcome);
}

/// Shows the rows a query returned, or the reason it failed.
fn on_query_outcome(
    siv: &mut cursive::Cursive,
    router: &Router,
    outcome: anyhow::Result<QueryOutcome>,
) {
    match outcome {
        Ok(outcome) => router.push(siv, Box::new(QueryResultsRoute { outcome })),
        Err(err) => {
            eprintln!("Error running query: {err:?}");
            siv.add_layer(views::Dialog::around(build_query_error(&err)));
        }
    }
}

type IndexedRow = (usize, Row);

impl cursive_table_view::TableViewItem<TableColumn> for IndexedRow {
    fn to_column(&self, column: TableColumn) -> String {
        match column {
            TableColumn::Idx => self.0.to_string(),
            TableColumn::DBCol(column) => self
                .1
                .get(column)
                .map(ToString::to_string)
                .unwrap_or_default(),
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

/// Width of the row number column, wide enough for the largest row number.
fn index_col_width(row_count: usize) -> usize {
    // `ilog10` is undefined on 0, and an empty result still needs a column.
    (row_count.max(1).ilog10() + 1) as usize
}

fn col_size(rows: &ResultSet, col: usize) -> usize {
    let name_size = rows.columns().get(col).map(|c| c.name.len()).unwrap_or(0);
    let max_col_size = rows
        .rows()
        .iter()
        .map(|row| row.get(col).map(|v| v.to_string().len()).unwrap_or(0))
        .max()
        .unwrap_or(0);

    std::cmp::min(
        32, // clip to 32 chars
        std::cmp::max(name_size, max_col_size),
    )
}

struct QueryResultsRoute {
    outcome: QueryOutcome,
}

impl Route for QueryResultsRoute {
    fn mount(&self, app_data_ptr: AppDataPtr, siv: &mut cursive::Cursive, router: &Router) {
        let router = router.clone();
        siv.add_layer(views::Dialog::around(
            views::OnEventView::new(build_query_results(
                Arc::clone(&app_data_ptr),
                &router,
                &self.outcome.resource_id,
                &self.outcome.title,
                &self.outcome.rows,
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
    rows: &ResultSet,
) -> impl cursive::view::View {
    let mut table = cursive_table_view::TableView::<IndexedRow, TableColumn>::new();

    if !rows.columns().is_empty() {
        table.add_column(TableColumn::Idx, "#", |col| {
            col.width(index_col_width(rows.len()))
        });

        for (idx, col) in rows.columns().iter().enumerate() {
            table.add_column(TableColumn::DBCol(idx), col.name.as_str(), |col| {
                col.width(col_size(rows, idx))
            });
        }

        table.set_items(rows.rows().iter().cloned().enumerate().collect());
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

fn build_row_view(row: &Row) -> impl cursive::view::View {
    let mut values = views::LinearLayout::vertical();

    for (col, value) in row.columns().iter().zip(row.values()) {
        let view = cursive::views::TextView::new(value.to_string());
        values.add_child(views::Panel::new(view).title(col.name.as_str()));
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
    row: &Row,
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
    row: &Row,
) -> impl cursive::view::View {
    let mut select_view = views::SelectView::new();

    let r = get_resource(&app_data_ptr, resource_id);

    for (link_name, link) in &r.links {
        if !evaluate_link_condition(link.condition.as_ref(), row).unwrap_or_else(|err| {
            eprintln!("Error evaluating condition for link {link_name}: {err}");
            true
        }) {
            continue;
        }

        select_view.add_item_str(link_name.as_str());
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
        .child(build_shortcut_select_view(select_view, "link_picker"))
}

fn on_pick_link(
    app_data_ptr: AppDataPtr,
    siv: &mut cursive::Cursive,
    router: &Router,
    resource_id: &str,
    link_name: &str,
    row: &Row,
) {
    siv.pop_layer(); // close the link picker

    let outcome = app_data_ptr
        .lock()
        .unwrap()
        .follow_link(resource_id, link_name, row);

    on_query_outcome(siv, router, outcome);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_col_width_fits_the_largest_row_number() {
        // An empty result still renders its columns, so a width of 0 rows must
        // not be a special case.
        assert_eq!(index_col_width(0), 1);
        assert_eq!(index_col_width(1), 1);
        assert_eq!(index_col_width(9), 1);
        assert_eq!(index_col_width(10), 2);
        assert_eq!(index_col_width(999), 3);
        assert_eq!(index_col_width(1000), 4);
    }
}
