use std::sync::Arc;

use dbdrill_core::session::{QueryOutcome, Resources, evaluate_link_condition};
use dbdrill_core::value::Row;

use crate::db::{Db, Event, RequestId};
use crate::picker::{Action, Picker};
use crate::results;

/// Where the database connection is at.
enum Connection {
    Connecting,
    Ready,
    Failed(String),
}

/// One step of the browsing history.
enum View {
    Resources {
        picker: Picker,
    },
    Searches {
        resource_id: String,
        picker: Picker,
    },
    Params {
        resource_id: String,
        search_id: String,
        names: Vec<String>,
        values: Vec<String>,
        /// Set on the frame the view opens, to focus the first field.
        opening: bool,
    },
    Results {
        outcome: QueryOutcome,
        table: results::Table,
    },
    Links {
        /// The resource the row belongs to, whose links we are offering.
        resource_id: String,
        row: Row,
        /// Names of the links that apply to this row.
        names: Vec<String>,
        picker: Picker,
    },
}

/// Shown between the steps of the trail.
pub const TRAIL_SEPARATOR: &str = "›";

/// Shown in place of what was cut off a trail step too long to fit.
pub const ELLIPSIS: &str = "…";

/// Longest a step of the trail gets before it is cut short, in characters.
const MAX_TRAIL_STEP: usize = 40;

/// Cuts a trail step down so that a long query title cannot push the rest of
/// the trail off the bar. The whole title stays available on hover.
fn shorten(title: &str) -> String {
    if title.chars().count() <= MAX_TRAIL_STEP {
        return title.to_owned();
    }

    title
        .chars()
        .take(MAX_TRAIL_STEP - 1)
        .chain(ELLIPSIS.chars())
        .collect()
}

pub struct App {
    resources: Arc<Resources>,
    db: Db,
    connection: Connection,
    /// The browsing history, innermost last. Never empty.
    stack: Vec<View>,
    /// The query this tab is waiting for, if any.
    pending: Option<RequestId>,
    /// How many steps the trail had last frame, to notice when it changes.
    trail_depth: usize,
    error: Option<String>,
}

impl App {
    pub fn new(resources: Arc<Resources>, db: Db) -> Self {
        App {
            resources,
            db,
            connection: Connection::Connecting,
            stack: vec![View::Resources {
                picker: Picker::default(),
            }],
            pending: None,
            trail_depth: 0,
            error: None,
        }
    }

    fn take_events(&mut self) {
        for event in self.db.poll() {
            match event {
                Event::Connected => self.connection = Connection::Ready,
                Event::ConnectionFailed(err) => {
                    self.connection = Connection::Failed(format!("{err:#}"));
                    self.pending = None;
                }
                // A result we are no longer waiting for: the user moved on.
                Event::Finished { id, .. } if self.pending != Some(id) => {}
                Event::Finished { outcome, .. } => {
                    self.pending = None;

                    match outcome {
                        Ok(outcome) => self.stack.push(View::Results {
                            table: results::Table::new(&outcome.rows),
                            outcome,
                        }),
                        Err(err) => self.error = Some(format!("{err:#}")),
                    }
                }
            }
        }
    }

    /// Sorted identifiers of every resource, with the labels to show for them.
    fn resource_items(&self) -> (Vec<String>, Vec<String>) {
        let mut resources: Vec<(&String, &String)> = self
            .resources
            .iter()
            .map(|(id, resource)| (id, &resource.name))
            .collect();

        resources.sort_by_key(|(_, name)| (*name).clone());

        resources
            .into_iter()
            .map(|(id, name)| (id.clone(), name.clone()))
            .unzip()
    }

    fn search_items(&self, resource_id: &str) -> Vec<String> {
        let Some(resource) = self.resources.get(resource_id) else {
            return Vec::new();
        };

        let mut searches: Vec<String> = resource.search.keys().cloned().collect();
        searches.sort();
        searches
    }

    /// Opens the search `search_id`, asking for its parameters first if it
    /// takes any.
    fn open_search(&mut self, resource_id: &str, search_id: &str) {
        let params: Vec<String> = self
            .resources
            .get(resource_id)
            .and_then(|resource| resource.search.get(search_id))
            .map(|search| search.params.iter().map(|p| p.name.clone()).collect())
            .unwrap_or_default();

        if params.is_empty() {
            self.run_search(resource_id, search_id, Vec::new());
            return;
        }

        self.stack.push(View::Params {
            resource_id: resource_id.to_owned(),
            search_id: search_id.to_owned(),
            values: vec![String::new(); params.len()],
            names: params,
            opening: true,
        });
    }

    /// Opens the list of links that apply to `row`.
    ///
    /// A link whose condition cannot be worked out is offered anyway: better a
    /// link that turns out to lead nowhere than one silently missing.
    fn open_links(&mut self, resource_id: &str, row: Row) {
        let Some(resource) = self.resources.get(resource_id) else {
            return;
        };

        let mut names: Vec<String> = resource
            .links
            .iter()
            .filter(|(name, link)| {
                evaluate_link_condition(link.condition.as_ref(), &row).unwrap_or_else(|err| {
                    eprintln!("error checking whether link {name} applies: {err:#}");
                    true
                })
            })
            .map(|(name, _)| name.clone())
            .collect();

        names.sort();

        self.stack.push(View::Links {
            resource_id: resource_id.to_owned(),
            row,
            names,
            picker: Picker::default(),
        });
    }

    fn follow_link(&mut self, resource_id: &str, link_name: &str, row: Row) {
        self.error = None;
        self.pending = Some(self.db.follow_link(resource_id, link_name, row));
    }

    fn run_search(&mut self, resource_id: &str, search_id: &str, params: Vec<String>) {
        self.error = None;
        self.pending = Some(self.db.search(resource_id, search_id, params));
    }

    fn pop(&mut self) {
        // The root view stays: a window is closed with its own shortcut, not by
        // backing out of it.
        if self.can_go_back() {
            self.stack.pop();
            self.pending = None;
            self.error = None;
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}

impl App {
    /// Draws one frame.
    ///
    /// Kept apart from [`eframe::App::ui`] so that tests can drive the whole UI
    /// without an eframe window behind it.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        self.take_events();

        egui::Panel::top("status").show(ui, |ui| {
            // Everything in the bar lines up on its middle, so the button sits
            // level with the text next to it.
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                // Popped before the trail is drawn, so that it already ends at
                // the view we went back to.
                if self.show_back(ui) {
                    self.pop();
                }

                // The status takes what it needs from the right first; the
                // trail then scrolls within whatever is left between them.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    self.show_status(ui);

                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        if let Some(depth) = self.show_trail(ui) {
                            self.stack.truncate(depth + 1);
                            self.pending = None;
                            self.error = None;
                        }
                    });
                });
            });
        });

        if let Some(error) = self.error.clone() {
            egui::Panel::bottom("error").show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(ui.visuals().error_fg_color, "Query failed:");
                    ui.label(error);

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Dismiss").clicked() {
                            self.error = None;
                        }
                    });
                });
            });
        }

        egui::CentralPanel::default().show(ui, |ui| self.show_view(ui));
    }

    /// Draws the back button, and reports whether it was pressed.
    ///
    /// Does the same as Escape. On the first view there is nowhere to go back
    /// to, so it is left out rather than shown greyed.
    fn show_back(&self, ui: &mut egui::Ui) -> bool {
        self.can_go_back() && ui.button("Back").on_hover_text("Esc").clicked()
    }

    fn can_go_back(&self) -> bool {
        self.stack.len() > 1
    }

    /// Draws the trail of views leading here, and reports the one clicked.
    ///
    /// Going back a long way means pressing Escape as many times as it took to
    /// get there; the trail turns that into one click.
    fn show_trail(&mut self, ui: &mut egui::Ui) -> Option<usize> {
        let titles: Vec<String> = self.stack.iter().map(|view| self.title(view)).collect();
        let last = titles.len() - 1;

        // Following a link makes the trail longer than the bar is wide, so bring
        // the step just reached into view whenever the length changes. Asking
        // the step itself rather than pinning the area to its right edge, so a
        // user who has scrolled the trail is left alone.
        let follow = self.trail_depth != titles.len();
        self.trail_depth = titles.len();

        let mut clicked = None;

        egui::ScrollArea::horizontal()
            .id_salt("trail")
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for (depth, title) in titles.iter().enumerate() {
                    if depth > 0 {
                        ui.weak(TRAIL_SEPARATOR);
                    }

                    // The view we are on is where the trail ends, so it is not
                    // a step to go back to.
                    if depth == last {
                        let here = ui.strong(shorten(title)).on_hover_text(title);

                        if follow {
                            here.scroll_to_me(Some(egui::Align::Max));
                        }

                        continue;
                    }

                    if ui.link(shorten(title)).on_hover_text(title).clicked() {
                        clicked = Some(depth);
                    }
                }
            });

        clicked
    }

    fn title(&self, view: &View) -> String {
        match view {
            View::Resources { .. } => "Resources".to_owned(),
            View::Searches { resource_id, .. } => match self.resources.get(resource_id) {
                Some(resource) => format!("Search {} by...", resource.name),
                None => "Search".to_owned(),
            },
            View::Params {
                resource_id,
                search_id,
                ..
            } => match self.resources.get(resource_id) {
                Some(resource) => format!("Search {} by {search_id}", resource.name),
                None => format!("Search by {search_id}"),
            },
            View::Results { outcome, .. } => outcome.title.clone(),
            View::Links { resource_id, .. } => match self.resources.get(resource_id) {
                Some(resource) => format!("{} links", resource.name),
                None => "Links".to_owned(),
            },
        }
    }

    fn show_status(&self, ui: &mut egui::Ui) {
        match &self.connection {
            Connection::Failed(err) => {
                ui.colored_label(ui.visuals().error_fg_color, format!("Disconnected: {err}"));
            }
            Connection::Connecting => {
                ui.spinner();
                ui.weak("Connecting...");
            }
            Connection::Ready if self.pending.is_some() => {
                ui.spinner();
                ui.weak("Running query...");
            }
            Connection::Ready => {}
        }
    }

    fn show_view(&mut self, ui: &mut egui::Ui) {
        // Taken here so that a view further down cannot swallow it.
        let mut view = self.stack.pop().expect("the view stack is never empty");
        let mut dismissed = false;

        match &mut view {
            View::Resources { picker } => {
                let (ids, names) = self.resource_items();

                match picker.show(ui, &names) {
                    Action::Picked(idx) => {
                        self.stack.push(view);
                        self.stack.push(View::Searches {
                            resource_id: ids[idx].clone(),
                            picker: Picker::default(),
                        });
                        return;
                    }
                    Action::Dismissed => dismissed = true,
                    Action::None => {}
                }
            }

            View::Searches {
                resource_id,
                picker,
            } => {
                let searches = self.search_items(resource_id);

                match picker.show(ui, &searches) {
                    Action::Picked(idx) => {
                        let (resource_id, search_id) = (resource_id.clone(), searches[idx].clone());
                        self.stack.push(view);
                        self.open_search(&resource_id, &search_id);
                        return;
                    }
                    Action::Dismissed => dismissed = true,
                    Action::None => {}
                }
            }

            View::Params {
                resource_id,
                search_id,
                names,
                values,
                opening,
            } => match show_params(ui, names, values, opening) {
                Params::Submitted => {
                    let (resource_id, search_id) = (resource_id.clone(), search_id.clone());
                    let values = values.clone();
                    self.stack.push(view);
                    self.run_search(&resource_id, &search_id, values);
                    return;
                }
                Params::Dismissed => dismissed = true,
                Params::Editing => {}
            },

            View::Results { outcome, table } => match table.show(ui, &outcome.rows) {
                results::Action::ShowLinks => {
                    let row = table.selected_row(&outcome.rows).cloned();
                    let resource_id = outcome.resource_id.clone();
                    self.stack.push(view);

                    if let Some(row) = row {
                        self.open_links(&resource_id, row);
                    }

                    return;
                }
                results::Action::Dismissed => dismissed = true,
                results::Action::None => {}
            },

            View::Links {
                resource_id,
                row,
                names,
                picker,
            } => match picker.show(ui, names) {
                Action::Picked(idx) => {
                    let (resource_id, link_name) = (resource_id.clone(), names[idx].clone());
                    let row = row.clone();
                    self.stack.push(view);
                    self.follow_link(&resource_id, &link_name, row);
                    return;
                }
                Action::Dismissed => dismissed = true,
                Action::None => {}
            },
        }

        self.stack.push(view);

        if dismissed {
            self.pop();
        }
    }
}

enum Params {
    Editing,
    Submitted,
    Dismissed,
}

fn show_params(
    ui: &mut egui::Ui,
    names: &[String],
    values: &mut [String],
    opening: &mut bool,
) -> Params {
    let (submit, escape) = ui.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
            i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
        )
    });

    if escape {
        return Params::Dismissed;
    }

    let mut focus_next = std::mem::take(opening);

    for (name, value) in names.iter().zip(values.iter_mut()) {
        ui.label(name);
        let response = ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY));

        if std::mem::take(&mut focus_next) {
            response.request_focus();
        }
    }

    ui.add_space(8.0);

    if ui.button("Search").clicked() || submit {
        return Params::Submitted;
    }

    Params::Editing
}
