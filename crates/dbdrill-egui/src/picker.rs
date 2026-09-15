//! A keyboard driven list picker.
//!
//! Picking from a list is dbdrill's main interaction, so it stays as close to
//! the terminal UI as a window allows: every item gets a highlighted letter,
//! and pressing that letter picks it outright. The lists are short enough that
//! this reaches any item in one key, so there is nothing to filter.

use dbdrill_core::shortcuts::assign_shortcuts;
use egui::text::LayoutJob;

use crate::hints::Hint;

/// What the user did with the list this frame.
pub enum Action {
    None,
    /// The index, in the list handed to [`Picker::show`], of the picked item.
    Picked(usize),
    /// Escape was pressed.
    Dismissed,
}

#[derive(Default)]
pub struct Picker {
    selected: usize,
    /// Set when the keyboard moved the selection, to scroll it back into view.
    follow_selection: bool,
}

impl Picker {
    /// What the picker answers to, for the bar at the bottom of the window.
    ///
    /// `pick` names what the letters choose here, which is the only thing that
    /// changes from one list to the next.
    ///
    /// Kept next to [`Picker::handle_keys`], which is what it describes. Escape
    /// is left out: whether there is anywhere to go back to is not the
    /// picker's to know.
    pub fn hints(pick: &str) -> Vec<Hint> {
        vec![
            Hint::new("a-z", pick),
            Hint::new("\u{2191}/\u{2193}", "move"),
            Hint::new("Enter", "choose"),
        ]
    }

    pub fn show(&mut self, ui: &mut egui::Ui, items: &[String]) -> Action {
        self.selected = self.selected.min(items.len().saturating_sub(1));

        let shortcuts = assign_shortcuts(items.iter().map(String::as_str));

        match self.handle_keys(ui, &shortcuts, items.len()) {
            Action::None => {}
            action => return action,
        }

        if items.is_empty() {
            ui.weak("Nothing here");
            return Action::None;
        }

        let mut picked = None;

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (idx, (item, shortcut)) in items.iter().zip(&shortcuts).enumerate() {
                    let selected = idx == self.selected;
                    let job = label_job(ui, item, *shortcut, selected);
                    let response = ui.selectable_label(selected, job);

                    if response.clicked() {
                        self.selected = idx;
                        picked = Some(idx);
                    }

                    if selected && self.follow_selection {
                        response.scroll_to_me(None);
                    }
                }
            });

        self.follow_selection = false;

        match picked {
            Some(idx) => Action::Picked(idx),
            None => Action::None,
        }
    }

    fn handle_keys(
        &mut self,
        ui: &egui::Ui,
        shortcuts: &[Option<(usize, char)>],
        len: usize,
    ) -> Action {
        let (up, down, enter, escape) = ui.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)
                    || i.consume_key(egui::Modifiers::CTRL, egui::Key::P),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)
                    || i.consume_key(egui::Modifiers::CTRL, egui::Key::N),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
            )
        });

        if up {
            self.move_selection(-1, len);
        }

        if down {
            self.move_selection(1, len);
        }

        if escape {
            return Action::Dismissed;
        }

        if enter && self.selected < len {
            return Action::Picked(self.selected);
        }

        // A bare letter picks its item outright.
        let typed = ui.input(|i| {
            i.events.iter().find_map(|event| match event {
                egui::Event::Text(text) => text.chars().next(),
                _ => None,
            })
        });

        let Some(typed) = typed else {
            return Action::None;
        };

        let typed = typed.to_lowercase().next().unwrap_or(typed);
        let picked = shortcuts
            .iter()
            .position(|shortcut| matches!(shortcut, Some((_, c)) if *c == typed));

        match picked {
            Some(idx) => Action::Picked(idx),
            None => Action::None,
        }
    }

    fn move_selection(&mut self, delta: isize, len: usize) {
        if len == 0 {
            return;
        }

        let last = len - 1;
        self.selected = match delta {
            d if d < 0 => self.selected.checked_sub(1).unwrap_or(last),
            _ if self.selected >= last => 0,
            _ => self.selected + 1,
        };

        self.follow_selection = true;
    }
}

/// Renders a label with its shortcut letter picked out.
fn label_job(
    ui: &egui::Ui,
    label: &str,
    shortcut: Option<(usize, char)>,
    selected: bool,
) -> LayoutJob {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let plain = if selected {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().text_color()
    };

    let mut job = LayoutJob::default();

    let Some((at, _)) = shortcut else {
        job.append(label, 0.0, egui::TextFormat::simple(font, plain));
        return job;
    };

    let mut section = |text: String, color| {
        if !text.is_empty() {
            job.append(&text, 0.0, egui::TextFormat::simple(font.clone(), color));
        }
    };

    section(label.chars().take(at).collect(), plain);
    section(
        label.chars().skip(at).take(1).collect(),
        ui.visuals().hyperlink_color,
    );
    section(label.chars().skip(at + 1).collect(), plain);

    job
}
