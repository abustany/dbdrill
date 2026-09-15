//! The results table.
//!
//! Rows are virtualised, so a result of any size costs the same to draw. The
//! table itself only scrolls vertically, so it sits inside a horizontal scroll
//! area to let wide rows be reached sideways.

use std::cmp::Ordering;

use dbdrill_core::value::{ResultSet, Row, Value};
use egui_extras::{Column, TableBuilder};

use crate::hints::{self, Hint};

/// Widest a column is to start with, in characters. Columns can be dragged
/// wider, so this only decides what is worth showing before anyone touches it.
const MAX_INITIAL_COL_CHARS: usize = 60;

/// Narrowest a column can be dragged, in points.
const MIN_COL_WIDTH: f32 = 40.0;

/// What the table is asking the rest of the app to do.
#[derive(PartialEq, Eq)]
pub enum Action {
    None,
    /// Go back to where the results were opened from.
    Dismissed,
    /// Show where the selected row can be followed to.
    ShowLinks,
}

/// Which column the rows are ordered by.
#[derive(Clone, Copy)]
struct Sort {
    column: usize,
    ascending: bool,
}

/// Keys the table acts on, taken before anything else can swallow them.
impl Action {
    fn dismissed_if(dismissed: bool) -> Self {
        if dismissed {
            Action::Dismissed
        } else {
            Action::None
        }
    }
}

#[derive(Default)]
struct Keys {
    up: bool,
    down: bool,
    left: bool,
    right: bool,
    first: bool,
    last: bool,
    page_up: bool,
    page_down: bool,
    open: bool,
    close: bool,
    copy: bool,
    links: bool,
}

fn read_keys(ui: &egui::Ui) -> Keys {
    use egui::{Key, Modifiers};

    ui.input_mut(|i| Keys {
        // j and k move like the arrows, the way a pager does it.
        up: i.consume_key(Modifiers::NONE, Key::ArrowUp) | i.consume_key(Modifiers::NONE, Key::K),
        down: i.consume_key(Modifiers::NONE, Key::ArrowDown)
            | i.consume_key(Modifiers::NONE, Key::J),
        left: i.consume_key(Modifiers::NONE, Key::ArrowLeft),
        right: i.consume_key(Modifiers::NONE, Key::ArrowRight),
        first: i.consume_key(Modifiers::NONE, Key::Home),
        last: i.consume_key(Modifiers::NONE, Key::End),
        page_up: i.consume_key(Modifiers::NONE, Key::PageUp),
        page_down: i.consume_key(Modifiers::NONE, Key::PageDown),
        open: i.consume_key(Modifiers::NONE, Key::Enter),
        close: i.consume_key(Modifiers::NONE, Key::Escape),
        copy: consume_copy(i),
        links: i.consume_key(Modifiers::NONE, Key::L),
    })
}

/// Takes the platform's copy shortcut, if it was pressed.
///
/// It never arrives as a key press: the windowing layer turns Cmd-C, Ctrl-C,
/// and whatever else the platform uses into [`egui::Event::Copy`] and drops the
/// keystroke, so watching for the keys themselves would never fire.
fn consume_copy(input: &mut egui::InputState) -> bool {
    let mut copy = false;

    input.events.retain(|event| {
        let is_copy = matches!(event, egui::Event::Copy);
        copy |= is_copy;
        !is_copy
    });

    copy
}

pub struct Table {
    /// The selected cell: where its row sits on screen, and which column.
    cursor: (usize, usize),
    /// Where each row on screen sits in the result set. Sorting reorders this
    /// rather than the rows themselves.
    order: Vec<usize>,
    sort: Option<Sort>,
    /// A row to bring into view on the next frame.
    scroll_to: Option<usize>,
    /// Whether the full row is open over the table.
    detail: bool,
    /// Rows that fit on screen, for PageUp and PageDown.
    page: usize,
}

impl Table {
    pub fn new(rows: &ResultSet) -> Self {
        Table {
            cursor: (0, 0),
            order: (0..rows.len()).collect(),
            sort: None,
            scroll_to: None,
            detail: false,
            page: 1,
        }
    }

    /// What the table answers to, for the bar at the bottom of the window.
    ///
    /// Kept next to [`read_keys`], which is what it describes. Escape is left
    /// out unless the table takes it for itself, which it does only while a
    /// row is open; the rest of the time it means going back, which is not the
    /// table's to say.
    pub fn hints(&self, ui: &egui::Ui, rows: &ResultSet, links: bool) -> Vec<Hint> {
        // An open row swallows every other key, so nothing else is on offer.
        if self.detail {
            return vec![Hint::new("Esc", "close the row")];
        }

        // Nothing to move around in, and nothing to copy or follow.
        if rows.is_empty() || rows.columns().is_empty() {
            return Vec::new();
        }

        let mut hints = vec![
            Hint::new("j/k", "navigate"),
            Hint::new("\u{2190}/\u{2192}", "columns"),
            Hint::new("Enter", "open the row"),
            Hint::new(hints::copy_keys(ui.ctx().os()), "copy current cell"),
        ];

        if links {
            hints.push(Hint::new("l", "links"));
        }

        hints
    }

    /// Whether the full row is open over the table.
    pub fn detail_open(&self) -> bool {
        self.detail
    }

    /// Draws the table, and reports what the user asked for.
    pub fn show(&mut self, ui: &mut egui::Ui, rows: &ResultSet) -> Action {
        if rows.columns().is_empty() {
            ui.weak("This query returns no columns.");
            return Action::dismissed_if(read_keys(ui).close);
        }

        let keys = read_keys(ui);

        // While a row is open it takes the keys, so the table stays where it is
        // underneath rather than moving about behind the row.
        if !self.detail {
            self.apply_keys(ui, rows, &keys);
        }

        if rows.is_empty() {
            // The columns are known even so, because the query is prepared
            // before it runs.
            ui.weak(format!("No rows. Columns: {}", column_names(rows)));
            return Action::dismissed_if(keys.close);
        }

        // `TableBuilder` scrolls vertically on its own, but not sideways.
        //
        // Without `auto_shrink` the area is only as tall as its rows, which puts
        // the horizontal scroll bar directly under the last row instead of at
        // the bottom of the window.
        egui::ScrollArea::horizontal()
            .auto_shrink([false, false])
            .show(ui, |ui| self.show_table(ui, rows));

        // Drawn last, so it sits over the table rather than instead of it.
        if self.detail {
            self.show_detail(ui, rows);

            if keys.close {
                self.detail = false;
            }

            // Escape closed the row; it does not also leave the results.
            return Action::None;
        }

        if keys.links {
            return Action::ShowLinks;
        }

        Action::dismissed_if(keys.close)
    }

    fn apply_keys(&mut self, ui: &egui::Ui, rows: &ResultSet, keys: &Keys) {
        let last_row = self.order.len().saturating_sub(1);
        let last_col = rows.columns().len().saturating_sub(1);
        let (row, col) = self.cursor;

        let moved = match keys {
            k if k.up => Some((row.saturating_sub(1), col)),
            k if k.down => Some((last_row.min(row + 1), col)),
            k if k.first => Some((0, col)),
            k if k.last => Some((last_row, col)),
            k if k.page_up => Some((row.saturating_sub(self.page), col)),
            k if k.page_down => Some((last_row.min(row + self.page), col)),
            k if k.left => Some((row, col.saturating_sub(1))),
            k if k.right => Some((row, last_col.min(col + 1))),
            _ => None,
        };

        if let Some(cursor) = moved {
            self.cursor = cursor;
            self.scroll_to = Some(cursor.0);
        }

        if keys.open && !rows.is_empty() {
            self.detail = true;
        }

        if keys.copy
            && let Some(value) = self.selected_value(rows)
        {
            ui.ctx().copy_text(value.to_string());
        }
    }

    pub fn selected_row<'a>(&self, rows: &'a ResultSet) -> Option<&'a Row> {
        rows.rows().get(*self.order.get(self.cursor.0)?)
    }

    fn selected_value<'a>(&self, rows: &'a ResultSet) -> Option<&'a Value> {
        self.selected_row(rows)?.get(self.cursor.1)
    }

    fn show_table(&mut self, ui: &mut egui::Ui, rows: &ResultSet) {
        let row_height = ui
            .text_style_height(&egui::TextStyle::Monospace)
            .max(ui.spacing().interact_size.y);
        let available = ui.available_height();

        self.page = ((available / row_height) as usize).max(1);

        // Copied out so the closures below do not have to borrow `self`.
        let (selected_row, selected_col) = self.cursor;
        let sort = self.sort;

        let mut table = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .sense(egui::Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .min_scrolled_height(0.0)
            .max_scroll_height(available);

        for width in initial_widths(rows) {
            table = table.column(
                Column::initial(width)
                    .at_least(MIN_COL_WIDTH)
                    .clip(true)
                    .resizable(true),
            );
        }

        if let Some(row) = self.scroll_to.take() {
            table = table.scroll_to_row(row, None);
        }

        let mut sort_by = None;
        let mut clicked = None;

        table
            .header(row_height, |mut header| {
                for (idx, column) in rows.columns().iter().enumerate() {
                    header.col(|ui| {
                        egui::Sides::new().show(
                            ui,
                            |ui| {
                                ui.strong(&column.name).on_hover_text(column.ty.to_string());
                            },
                            |ui| {
                                if ui.button(sort_marker(sort, idx)).clicked() {
                                    sort_by = Some(idx);
                                }
                            },
                        );
                    });
                }
            })
            .body(|body| {
                body.rows(row_height, self.order.len(), |mut row| {
                    let position = row.index();
                    let values = rows.rows()[self.order[position]].values();

                    row.set_selected(position == selected_row);

                    for (idx, value) in values.iter().enumerate() {
                        let (_, response) = row.col(|ui| {
                            cell(ui, value, position == selected_row && idx == selected_col)
                        });

                        if response.clicked() {
                            clicked = Some((position, idx));
                        }
                    }
                });
            });

        if let Some(cursor) = clicked {
            self.cursor = cursor;
        }

        if let Some(column) = sort_by {
            self.sort_by(column, rows);
        }
    }

    /// Orders the rows by `column`, turning the order around if they already
    /// were.
    fn sort_by(&mut self, column: usize, rows: &ResultSet) {
        let ascending = match self.sort {
            Some(sort) if sort.column == column => !sort.ascending,
            _ => true,
        };

        self.sort = Some(Sort { column, ascending });

        let selected = self.order.get(self.cursor.0).copied();
        let cell = |idx: usize| rows.rows()[idx].get(column);

        self.order.sort_by(|&a, &b| {
            let order = match (cell(a), cell(b)) {
                (Some(a), Some(b)) => a.compare(b),
                _ => Ordering::Equal,
            };

            if ascending { order } else { order.reverse() }
        });

        // Follow the row that was selected to wherever it went.
        if let Some(row) = selected
            && let Some(position) = self.order.iter().position(|&idx| idx == row)
        {
            self.cursor.0 = position;
            self.scroll_to = Some(position);
        }
    }

    fn show_detail(&mut self, ui: &mut egui::Ui, rows: &ResultSet) {
        let Some(row) = self.selected_row(rows) else {
            self.detail = false;
            return;
        };

        let mut close = false;

        let modal = egui::Modal::new(egui::Id::new("dbdrill_row")).show(ui.ctx(), |ui| {
            ui.set_width(ROW_WIDTH);
            ui.heading("Row");
            ui.separator();

            show_row_values(ui, row);

            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Copy row").clicked() {
                    ui.ctx().copy_text(row_as_tsv(row));
                }

                if ui.button("Close").clicked() {
                    close = true;
                }
            });
        });

        if close || modal.backdrop_response.clicked() {
            self.detail = false;
        }
    }
}

/// How wide an open row is, in points.
const ROW_WIDTH: f32 = 640.0;

/// How tall the values in an open row get before they start scrolling.
const ROW_MAX_HEIGHT: f32 = 480.0;

/// Draws the values of a row, one under the other, wrapped rather than clipped.
fn show_row_values(ui: &mut egui::Ui, row: &Row) {
    egui::ScrollArea::vertical()
        .max_height(ROW_MAX_HEIGHT)
        // Left to itself the area is only as wide as its content, which for a
        // row of short values leaves its scroll bar sitting near the middle of
        // the row rather than at the edge.
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (column, value) in row.columns().iter().zip(row.values()) {
                ui.strong(&column.name);
                ui.add(
                    egui::Label::new(egui::RichText::new(value.to_pretty_string()).monospace())
                        .wrap_mode(egui::TextWrapMode::Wrap),
                );
                ui.add_space(8.0);
            }
        });
}

/// Draws one cell, with the whole value on hover when it does not fit.
fn cell(ui: &mut egui::Ui, value: &Value, selected: bool) {
    let text = value.to_string();
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let color = if selected {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().text_color()
    };

    let width = ui.available_width();
    let fits = ui
        .ctx()
        .fonts_mut(|fonts| fonts.layout_no_wrap(text.clone(), font.clone(), color))
        .size()
        .x
        <= width;

    let label = ui.add(
        egui::Label::new(egui::RichText::new(&text).monospace().color(color))
            .wrap_mode(egui::TextWrapMode::Truncate)
            .selectable(false),
    );

    if !fits {
        label.on_hover_text(value.to_pretty_string());
    }
}

/// Markers on the header buttons saying how the rows are ordered.
///
/// Picked because the font egui ships with actually has them: the arrows the
/// official table demo uses come out as empty boxes here.
pub(crate) const SORTED_UP: &str = "\u{23F6}";
pub(crate) const SORTED_DOWN: &str = "\u{23F7}";
pub(crate) const UNSORTED: &str = "\u{00B7}";

fn sort_marker(sort: Option<Sort>, column: usize) -> &'static str {
    match sort {
        Some(sort) if sort.column == column && sort.ascending => SORTED_UP,
        Some(sort) if sort.column == column => SORTED_DOWN,
        _ => UNSORTED,
    }
}

fn row_as_tsv(row: &Row) -> String {
    let cells: Vec<String> = row.values().iter().map(Value::to_string).collect();
    cells.join("\t")
}

fn column_names(rows: &ResultSet) -> String {
    let names: Vec<&str> = rows.columns().iter().map(|col| col.name.as_str()).collect();
    names.join(", ")
}

/// Column widths to start with, wide enough for what they hold.
fn initial_widths(rows: &ResultSet) -> Vec<f32> {
    // Monospace, so every character is the same width; close enough for a
    // starting point, and the columns can be dragged from there.
    const POINTS_PER_CHAR: f32 = 7.9;
    const PADDING: f32 = 24.0;

    (0..rows.columns().len())
        .map(|idx| {
            let name = rows.columns()[idx].name.chars().count();
            let widest = rows
                .rows()
                .iter()
                .filter_map(|row| row.get(idx))
                .map(|value| value.to_string().chars().count())
                .max()
                .unwrap_or(0);

            let chars = name.max(widest).min(MAX_INITIAL_COL_CHARS);

            (chars as f32 * POINTS_PER_CHAR + PADDING).max(MIN_COL_WIDTH)
        })
        .collect()
}
