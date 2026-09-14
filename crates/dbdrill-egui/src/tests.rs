//! End to end tests.
//!
//! Every test drives the whole window the way a user does it, through key
//! presses and clicks, and looks only at what the window shows back. Nothing
//! reaches inside the app: a test that has to read a field is a test of a
//! detail rather than of a flow.
//!
//! The database is a stand-in that answers out of canned results, so a test
//! reads as a walkthrough rather than as message passing.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};

use anyhow::{Result, anyhow};
use dbdrill_core::model::{ColumnExpression, Link, LinkCondition, Resource, Search, SearchParam};
use dbdrill_core::session::{QueryOutcome, Resources};
use dbdrill_core::value::{Column, ResultSet, Type, Value};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable, by};

use crate::app::{App, TRAIL_SEPARATOR};
use crate::db::{Db, Event, Request};
use crate::results::{SORTED_DOWN, SORTED_UP, UNSORTED};

/// The window the walkthroughs run in.
///
/// Wide enough for the whole of the longest trail they reach: a trail that
/// does not fit scrolls, and the steps of a scrolling trail cannot be clicked
/// at all.
const WINDOW: egui::Vec2 = egui::vec2(1000.0, 600.0);

/// What the table puts on a column header to say how the rows are ordered.
const SORT_MARKERS: [&str; 3] = [SORTED_UP, SORTED_DOWN, UNSORTED];

// ---------------------------------------------------------------------------
// The resources the tests browse.
// ---------------------------------------------------------------------------

fn search(params: &[&str]) -> Search {
    Search {
        query: "SELECT 1".to_owned(),
        params: params
            .iter()
            .map(|name| SearchParam {
                name: (*name).to_owned(),
                ty: None,
            })
            .collect(),
    }
}

fn link(search: &str, on: &str, condition: Option<LinkCondition>) -> Link {
    Link {
        kind: "post".to_owned(),
        search: search.to_owned(),
        search_params: vec![ColumnExpression::Name(on.to_owned())],
        condition,
    }
}

/// Two resources to browse between, and one with nothing in it.
fn resources() -> Arc<Resources> {
    let user = Resource {
        name: "User".to_owned(),
        search: HashMap::from([
            ("email".to_owned(), search(&["Email"])),
            ("everyone".to_owned(), search(&[])),
        ]),
        links: HashMap::from([
            ("posts".to_owned(), link("by author", "id", None)),
            (
                // Only offered on a banned user, which none of the canned rows
                // is, so it must stay out of the list.
                "bans".to_owned(),
                link(
                    "by author",
                    "id",
                    Some(LinkCondition::Eq(
                        ColumnExpression::Name("status".to_owned()),
                        "banned".to_owned(),
                    )),
                ),
            ),
        ]),
    };

    let post = Resource {
        name: "Post".to_owned(),
        search: HashMap::from([
            ("all".to_owned(), search(&[])),
            ("by author".to_owned(), search(&["Author"])),
        ]),
        links: HashMap::new(),
    };

    let empty = Resource {
        name: "Empty".to_owned(),
        search: HashMap::new(),
        links: HashMap::new(),
    };

    Arc::new(HashMap::from([
        ("user".to_owned(), user),
        ("post".to_owned(), post),
        ("empty".to_owned(), empty),
    ]))
}

// ---------------------------------------------------------------------------
// The rows the stand-in database answers with.
// ---------------------------------------------------------------------------

fn column(name: &str, ty: Type) -> Column {
    Column {
        name: name.to_owned(),
        ty,
    }
}

fn user_columns() -> Vec<Column> {
    vec![
        column("id", Type::INT4),
        column("email", Type::TEXT),
        column("status", Type::TEXT),
    ]
}

/// As many users as asked for, of the two there are.
fn users(count: usize) -> ResultSet {
    let rows: Vec<Vec<Value>> = ["a@example.com", "b@example.com"]
        .iter()
        .take(count)
        .enumerate()
        .map(|(idx, email)| {
            vec![
                Value::Int4(idx as i32 + 1),
                Value::Text((*email).to_owned()),
                Value::Text("active".to_owned()),
            ]
        })
        .collect();

    ResultSet::new(user_columns(), rows)
}

fn posts() -> ResultSet {
    ResultSet::new(
        vec![column("id", Type::INT4), column("title", Type::TEXT)],
        vec![
            vec![Value::Int4(10), Value::Text("Hello".to_owned())],
            vec![Value::Int4(11), Value::Text("World".to_owned())],
        ],
    )
}

fn outcome(resource_id: &str, title: &str, rows: ResultSet) -> Result<QueryOutcome> {
    Ok(QueryOutcome {
        resource_id: resource_id.to_owned(),
        title: title.to_owned(),
        rows,
    })
}

/// Answers the queries a walkthrough sends, the way the database would.
fn canned(request: &Request) -> Result<QueryOutcome> {
    match request {
        Request::Search {
            resource_id,
            search_id,
            params,
            ..
        } => match (resource_id.as_str(), search_id.as_str()) {
            ("user", "email") => outcome(
                "user",
                &format!("User / email (Email={})", params[0]),
                users(1),
            ),
            ("user", "everyone") => outcome("user", "User / everyone ()", users(2)),
            ("post", "all") => outcome("post", "Post / all ()", posts()),
            _ => Err(anyhow!("unknown search {resource_id}/{search_id}")),
        },
        Request::Link {
            resource_id,
            link_name,
            ..
        } => match (resource_id.as_str(), link_name.as_str()) {
            ("user", "posts") => outcome("post", "User (1) -> posts", posts()),
            _ => Err(anyhow!("unknown link {resource_id}/{link_name}")),
        },
    }
}

// ---------------------------------------------------------------------------
// The window under test.
// ---------------------------------------------------------------------------

/// What a query the window sent amounts to, for a test to read back.
fn asked(request: &Request) -> String {
    match request {
        Request::Search {
            resource_id,
            search_id,
            params,
            ..
        } => format!("search {resource_id}/{search_id}({})", params.join(", ")),
        Request::Link {
            resource_id,
            link_name,
            row,
            ..
        } => {
            let values: Vec<String> = row.values().iter().map(Value::to_string).collect();
            format!("link {resource_id}/{link_name} from {}", values.join(", "))
        }
    }
}

/// Answers a query the way the database would.
type Reply = dyn Fn(&Request) -> Result<QueryOutcome>;

const DISCONNECTED: &str = "Disconnected: ";
const CONNECTING: &str = "Connecting...";
const RUNNING: &str = "Running query...";
const QUERY_FAILED: &str = "Query failed:";

struct Ui {
    harness: Harness<'static, App>,
    /// Queries on their way to the database.
    incoming: Receiver<Request>,
    /// Stands in for the database thread.
    worker: Sender<Event>,
    reply: Box<Reply>,
    /// Every query the window sent, in the order it sent them.
    sent: Vec<String>,
    /// Queries taken but deliberately left unanswered.
    held: Vec<Request>,
    /// Whether queries are answered as they arrive.
    answering: bool,
    /// What the last thing the user did put on the clipboard. Kept because
    /// only the frame that handles the key reports it, not the ones that
    /// settle after it.
    copied: Option<String>,
}

impl Ui {
    /// A window that has connected and answers its queries: the normal case.
    fn new() -> Self {
        let mut ui = Ui::disconnected();
        ui.send(Event::Connected);
        ui
    }

    /// A window still waiting on its database connection.
    fn disconnected() -> Self {
        let (db, incoming, worker) = Db::detached();
        let mut harness =
            Harness::new_ui_state(|ui, app: &mut App| app.show(ui), App::new(resources(), db));

        harness.set_size(WINDOW);

        // Scrolling instantly rather than over a few frames, so that what the
        // window shows depends on what the user did rather than on how long a
        // test waited afterwards.
        harness.ctx.all_styles_mut(|style| {
            style.scroll_animation = egui::style::ScrollAnimation::none();
        });

        let mut ui = Ui {
            harness,
            incoming,
            worker,
            reply: Box::new(canned),
            sent: Vec::new(),
            held: Vec::new(),
            answering: true,
            copied: None,
        };

        ui.settle();
        ui
    }

    /// Answers with `reply` instead of out of the canned results, for the
    /// tests that need a query to go wrong or to come back empty.
    fn replying(mut self, reply: impl Fn(&Request) -> Result<QueryOutcome> + 'static) -> Self {
        self.reply = Box::new(reply);
        self
    }

    // -- the database -------------------------------------------------------

    fn send(&mut self, event: Event) {
        self.worker.send(event).expect("the window hung up");
        self.settle();
    }

    /// Stops answering, so that a test can look at a window with a query still
    /// running.
    fn hold(&mut self) {
        self.answering = false;
    }

    /// Answers everything held back, and goes back to answering as usual.
    fn release(&mut self) {
        self.answering = true;

        for request in std::mem::take(&mut self.held) {
            self.answer(&request);
        }

        self.settle();
    }

    fn answer(&mut self, request: &Request) {
        let id = match request {
            Request::Search { id, .. } | Request::Link { id, .. } => *id,
        };
        let outcome = (self.reply)(request);

        // The window hangs up when the test is over; nothing to answer then.
        let _ = self.worker.send(Event::Finished { id, outcome });
    }

    /// Takes the queries the window sent, and answers them unless held.
    fn pump(&mut self) {
        while let Ok(request) = self.incoming.try_recv() {
            self.sent.push(asked(&request));

            if self.answering {
                self.answer(&request);
            } else {
                self.held.push(request);
            }
        }
    }

    // -- driving ------------------------------------------------------------

    /// Draws a few frames, letting the last thing the user did, and whatever
    /// the database answers to it, work their way through.
    ///
    /// A fixed number of frames rather than [`Harness::run`], because a
    /// spinner asks to be repainted forever and would never settle.
    fn settle(&mut self) {
        for _ in 0..6 {
            self.pump();
            self.harness.step();
            self.capture();
        }
    }

    /// Notes anything the last frame put on the clipboard.
    fn capture(&mut self) {
        let copied = self
            .harness
            .output()
            .platform_output
            .commands
            .iter()
            .find_map(|command| match command {
                egui::OutputCommand::CopyText(text) => Some(text.clone()),
                _ => None,
            });

        if copied.is_some() {
            self.copied = copied;
        }
    }

    fn press(&mut self, key: egui::Key) {
        self.act(|harness| harness.key_press(key));
    }

    fn type_text(&mut self, text: &str) {
        self.act(|harness| harness.event(egui::Event::Text(text.to_owned())));
    }

    /// Presses the platform's copy shortcut the way the windowing layer
    /// delivers it: as an [`egui::Event::Copy`], never as a key.
    ///
    /// Returns what ended up on the clipboard, which is the only way to see
    /// from outside which cell is selected.
    fn copy(&mut self) -> String {
        self.act(|harness| harness.event(egui::Event::Copy));
        self.copied.clone().expect("nothing was copied")
    }

    /// Clicks whatever is labelled `label`, be it a button, a link, a list
    /// item or a cell of the table.
    fn click(&mut self, label: &str) {
        self.act(|harness| {
            harness
                .query_all_by_label(label)
                .next()
                .unwrap_or_else(|| panic!("nothing labelled {label:?} to click"))
                .click();
        });
    }

    /// Clicks the sort button of the `column`th column, which is marked with
    /// how the rows are ordered by it rather than labelled.
    fn click_sort(&mut self, column: usize) {
        self.act(|harness| {
            harness
                .query_all(by().predicate(|node| {
                    node.label()
                        .is_some_and(|label| SORT_MARKERS.contains(&label.as_str()))
                }))
                .nth(column)
                .expect("no sort button")
                .click();
        });
    }

    /// Does something to the window, and draws the frames that follow from it.
    fn act(&mut self, action: impl FnOnce(&Harness<'static, App>)) {
        self.copied = None;
        action(&self.harness);
        self.settle();
    }

    // -- reading ------------------------------------------------------------

    /// Everything the window drew that has any text on it, along with where it
    /// drew it, topmost and leftmost first.
    ///
    /// Only the window itself: an open row is drawn over it, in a layer of its
    /// own, and is read with [`Ui::open_row`].
    fn window(&self) -> Vec<(egui::Rect, String)> {
        let Some(window) = self.harness.root().children().next() else {
            return Vec::new();
        };

        let mut drawn: Vec<(egui::Rect, String)> = text_of(&window);

        drawn.sort_by(|(a, _), (b, _)| {
            a.top()
                .total_cmp(&b.top())
                .then(a.left().total_cmp(&b.left()))
        });

        drawn
    }

    /// The bar across the top of the window: the back button, the trail and
    /// the status, which are whatever shares a line with the topmost thing
    /// drawn.
    fn top_bar(&self) -> Vec<String> {
        let drawn = self.window();
        let Some((first, _)) = drawn.first() else {
            return Vec::new();
        };

        drawn
            .iter()
            .take_while(|(rect, _)| rect.top() < first.bottom())
            .map(|(_, label)| label.clone())
            .collect()
    }

    /// The steps of the trail, left to right, ending on the view we are on.
    fn trail(&self) -> Vec<String> {
        self.top_bar()
            .into_iter()
            .filter(|label| label != TRAIL_SEPARATOR && label != "Back" && !is_status(label))
            .collect()
    }

    /// What the status corner of the bar is saying, if anything.
    fn status(&self) -> String {
        self.top_bar()
            .into_iter()
            .find(|label| is_status(label))
            .unwrap_or_default()
    }

    /// Everything under the bar, gathered into the lines it was drawn on.
    fn body(&self) -> Vec<Vec<String>> {
        let drawn = self.window();
        let Some((first, _)) = drawn.first() else {
            return Vec::new();
        };
        let bar = first.bottom();

        let mut lines: Vec<(f32, Vec<(f32, String)>)> = Vec::new();

        for (rect, label) in drawn.iter().filter(|(rect, _)| rect.top() >= bar) {
            let middle = rect.center().y;

            // What is drawn side by side is drawn at the same height, so
            // anything overlapping the middle of a line belongs to it.
            match lines
                .iter_mut()
                .find(|(at, _)| (at - middle).abs() < rect.height() / 2.0)
            {
                Some((_, line)) => line.push((rect.left(), label.clone())),
                None => lines.push((middle, vec![(rect.left(), label.clone())])),
            }
        }

        lines
            .into_iter()
            .map(|(_, mut line)| {
                line.sort_by(|(a, _), (b, _)| a.total_cmp(b));
                line.into_iter().map(|(_, label)| label).collect()
            })
            .collect()
    }

    /// The items a picker is offering, in the order it offers them.
    fn items(&self) -> Vec<String> {
        self.body()
            .into_iter()
            .filter(|line| !is_error(line))
            .flatten()
            .collect()
    }

    /// The names of the columns the table is showing.
    fn columns(&self) -> Vec<String> {
        self.body()
            .into_iter()
            .next()
            .unwrap_or_default()
            .into_iter()
            .filter(|label| !SORT_MARKERS.contains(&label.as_str()))
            .collect()
    }

    /// The rows the table is showing, cell by cell.
    ///
    /// The first line under the bar names the columns and carries the sort
    /// buttons; every line after it is a row.
    fn rows(&self) -> Vec<Vec<String>> {
        self.body()
            .into_iter()
            .skip(1)
            .filter(|line| !is_error(line))
            .collect()
    }

    /// The column and value of each line of the row open over the table.
    ///
    /// An open row is drawn over the window rather than in it, which puts it
    /// in a layer of its own, headed by its title.
    fn open_row(&self) -> Vec<(String, String)> {
        let row = self
            .harness
            .root()
            .children()
            .map(|layer| text_of(&layer))
            .find(|texts| texts.first().is_some_and(|(_, text)| text == "Row"))
            .expect("no row is open");

        row.iter()
            .skip(1)
            .map(|(_, text)| text.clone())
            .filter(|text| text != "Copy row" && text != "Close")
            .collect::<Vec<String>>()
            .chunks(2)
            .map(|pair| (pair[0].clone(), pair[1].clone()))
            .collect()
    }

    /// What the last thing the user did put on the clipboard.
    fn copied(&self) -> Option<&str> {
        self.copied.as_deref()
    }

    /// What the panel at the bottom is reporting, if anything.
    fn error(&self) -> Option<String> {
        self.body()
            .into_iter()
            .find(|line| is_error(line))
            .and_then(|line| line.get(1).cloned())
    }

    fn shows(&self, label: &str) -> bool {
        self.harness.query_by_label(label).is_some()
    }
}

/// The text of everything drawn in `layer`, in the order it was drawn.
///
/// Nodes that carry no text of their own are left out, and so is the run of
/// text inside a label, which would otherwise show up twice: once as the
/// label, once as the text in it.
fn text_of(layer: &egui_kittest::Node<'_>) -> Vec<(egui::Rect, String)> {
    layer
        .query_all(by().label_contains(""))
        .filter_map(|node| {
            let accesskit = node.accesskit_node();

            // A label keeps its text in `value`; buttons and links keep theirs
            // in `label`.
            let text = accesskit.label().or_else(|| accesskit.value())?;

            // Containers are drawn nowhere in particular, and asking them
            // where they are is an error rather than a `None`.
            if text.is_empty() || accesskit.bounding_box().is_none() {
                return None;
            }

            Some((node.rect(), text))
        })
        .collect()
}

fn is_status(label: &str) -> bool {
    label == CONNECTING || label == RUNNING || label.starts_with(DISCONNECTED)
}

fn is_error(line: &[String]) -> bool {
    line.first().is_some_and(|label| label == QUERY_FAILED)
}

// ---------------------------------------------------------------------------
// The walkthroughs.
// ---------------------------------------------------------------------------

/// The whole way in: pick a resource, pick one of its searches, fill it in,
/// read what came back, and follow a row on to another resource.
#[test]
fn a_search_leads_to_rows_and_a_row_leads_on() {
    let mut ui = Ui::new();

    assert_eq!(ui.items(), ["Empty", "Post", "User"], "the resources");
    assert_eq!(ui.trail(), ["Resources"]);
    assert_eq!(
        ui.status(),
        "",
        "nothing has been asked of the database yet"
    );

    // The letter picked out in an item picks it outright.
    ui.type_text("u");

    assert_eq!(ui.items(), ["email", "everyone"], "the searches of User");
    assert_eq!(ui.trail(), ["Resources", "Search User by..."]);

    // A search that takes parameters asks for them before it runs.
    ui.type_text("e");

    assert!(ui.shows("Email"), "the parameter was not asked for");
    assert!(ui.sent.is_empty(), "ran before being filled in");

    ui.hold();
    ui.type_text("a@example.com");
    ui.press(egui::Key::Enter);

    assert_eq!(ui.sent, ["search user/email(a@example.com)"]);
    assert_eq!(ui.status(), RUNNING, "no sign of the query running");

    ui.release();

    assert_eq!(ui.status(), "", "the query is still said to be running");
    assert_eq!(ui.columns(), ["id", "email", "status"]);
    assert_eq!(ui.rows(), [["1", "a@example.com", "active"]]);
    assert_eq!(
        ui.trail(),
        [
            "Resources",
            "Search User by...",
            "Search User by email",
            "User / email (Email=a@example.com)"
        ]
    );

    // The selection starts on the first cell, and the arrows move it.
    assert_eq!(ui.copy(), "1");
    ui.press(egui::Key::ArrowRight);
    assert_eq!(ui.copy(), "a@example.com");

    // The row opens over the table, whole, and closes again.
    ui.press(egui::Key::Enter);

    assert_eq!(
        ui.open_row(),
        [
            ("id".to_owned(), "1".to_owned()),
            ("email".to_owned(), "a@example.com".to_owned()),
            ("status".to_owned(), "active".to_owned()),
        ]
    );

    ui.click("Copy row");
    assert_eq!(ui.copied(), Some("1\ta@example.com\tactive"));

    ui.press(egui::Key::Escape);

    assert!(!ui.shows("Row"), "the row did not close");
    assert_eq!(ui.rows(), [["1", "a@example.com", "active"]]);
    assert_eq!(ui.trail().len(), 4, "closing the row left the results");

    // Only the links that apply to the row are offered.
    ui.press(egui::Key::L);
    assert_eq!(ui.items(), ["posts"]);

    ui.type_text("p");

    assert_eq!(
        ui.sent.last().unwrap(),
        "link user/posts from 1, a@example.com, active"
    );
    assert_eq!(ui.columns(), ["id", "title"], "did not land on the posts");
    assert_eq!(ui.rows(), [["10", "Hello"], ["11", "World"]]);
    assert_eq!(ui.trail().len(), 6, "the trail did not follow the link");

    // The trail goes back as far as it is clicked, in one go.
    ui.click("Resources");

    assert_eq!(ui.trail(), ["Resources"]);
    assert_eq!(ui.items(), ["Empty", "Post", "User"]);
}

/// The same way round with the mouse, which has to reach everywhere the
/// keyboard does.
#[test]
fn the_window_can_be_driven_by_the_mouse_alone() {
    let mut ui = Ui::new();

    ui.click("User");
    assert_eq!(ui.trail(), ["Resources", "Search User by..."]);

    // A search without parameters runs straight away.
    ui.click("everyone");

    assert_eq!(ui.sent, ["search user/everyone()"]);
    assert_eq!(
        ui.rows(),
        [
            ["1", "a@example.com", "active"],
            ["2", "b@example.com", "active"]
        ]
    );

    // Clicking a cell selects it, wherever it is.
    ui.click("a@example.com");
    assert_eq!(ui.copy(), "a@example.com");

    // Clicking a column orders the rows by it, and clicking it again turns
    // them around. The selection stays on the row it was on.
    ui.click_sort(0);
    ui.click_sort(0);

    assert_eq!(
        ui.rows(),
        [
            ["2", "b@example.com", "active"],
            ["1", "a@example.com", "active"]
        ]
    );
    // The row that was selected was the first one and is now the last.
    assert_eq!(ui.copy(), "a@example.com", "the selection lost its row");

    // The row opens over the table and closes from its own button.
    ui.press(egui::Key::Enter);
    assert_eq!(
        ui.open_row()[1],
        ("email".to_owned(), "a@example.com".to_owned())
    );

    ui.click("Close");
    assert!(!ui.shows("Row"), "the row did not close");

    // Back goes one step, the way Escape does.
    ui.click("Back");
    assert_eq!(ui.items(), ["email", "everyone"]);

    ui.click("Resources");
    assert!(!ui.shows("Back"), "there is nowhere to go back to");

    // Parameters are filled in and sent off with the button.
    ui.click("User");
    ui.click("email");
    ui.type_text("a@example.com");
    ui.click("Search");

    assert_eq!(ui.sent.last().unwrap(), "search user/email(a@example.com)");
    assert_eq!(ui.rows(), [["1", "a@example.com", "active"]]);
}

/// Everything that can come back other than rows, and what the window makes
/// of it.
#[test]
fn what_goes_wrong_is_reported_rather_than_hidden() {
    // Connecting is reported until it is done, one way or the other.
    let mut ui = Ui::disconnected();
    assert_eq!(ui.status(), CONNECTING);

    ui.send(Event::ConnectionFailed(anyhow!("host unreachable")));
    assert_eq!(ui.status(), "Disconnected: host unreachable");

    // A query that fails says so, and leaves the window where it was.
    let mut ui = Ui::new().replying(|_| Err(anyhow!("relation does not exist")));
    ui.type_text("p");
    ui.type_text("a");

    assert_eq!(ui.error().as_deref(), Some("relation does not exist"));
    assert_eq!(ui.items(), ["all", "by author"], "left the search picker");

    ui.click("Dismiss");
    assert_eq!(ui.error(), None, "the error stayed up");

    // A query that matches nothing still says what it would have returned.
    let mut ui = Ui::new().replying(|_| {
        outcome(
            "user",
            "User / everyone ()",
            ResultSet::new(user_columns(), Vec::new()),
        )
    });
    ui.type_text("u");
    ui.type_text("v");

    assert!(ui.shows("No rows. Columns: id, email, status"));

    // A query that returns nothing at all is not left looking empty.
    let mut ui = Ui::new().replying(|_| {
        outcome(
            "post",
            "Post / all ()",
            ResultSet::new(Vec::new(), Vec::new()),
        )
    });
    ui.type_text("p");
    ui.type_text("a");

    assert!(ui.shows("This query returns no columns."));

    // A result the user has moved on from is dropped rather than shown.
    let mut ui = Ui::new();
    ui.hold();
    ui.type_text("p");
    ui.type_text("a");
    ui.press(egui::Key::Escape);
    ui.release();

    assert_eq!(
        ui.items(),
        ["Empty", "Post", "User"],
        "showed a result nobody was waiting for"
    );

    // A resource with nothing to search says so rather than showing a blank.
    let mut ui = Ui::new();
    ui.type_text("e");

    assert!(ui.shows("Nothing here"));
}
