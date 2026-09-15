//! The bar of keyboard hints along the bottom of the window.
//!
//! Which keys work depends on where the user is, so rather than a help screen
//! listing all of them, the bar shows the ones that work right here. Each view
//! says what it answers to next to the code reading those keys, so that a key
//! cannot be added without its hint being right there to add too.

use egui::text::LayoutJob;

/// One shortcut, as the bar shows it.
pub struct Hint {
    /// The keys, spelled the way the user thinks of them: `j/k`, `Esc`, `a-z`.
    keys: String,
    /// What they do, as a phrase that follows the keys: `navigate`, `go back`.
    action: String,
}

impl Hint {
    pub fn new(keys: impl Into<String>, action: impl Into<String>) -> Self {
        Hint {
            keys: keys.into(),
            action: action.into(),
        }
    }
}

/// Between the keys and what they do.
const SEPARATOR: &str = ": ";

/// Space between one hint and the next, in points.
const GAP: f32 = 16.0;

/// The copy shortcut, spelled the way the platform spells it.
///
/// The keys themselves never reach us - the windowing layer turns them into an
/// [`egui::Event::Copy`] - but they are still what the user presses.
pub fn copy_keys(os: egui::os::OperatingSystem) -> &'static str {
    match os {
        egui::os::OperatingSystem::Mac => "Cmd-C",
        _ => "Ctrl-C",
    }
}

/// Makes the arrows in the keys render.
///
/// Of the fonts egui ships with, only the monospace one has the arrows in it.
/// Setting the keys in that font makes them look a size larger than the rest
/// of the bar, because it has a much bigger x-height at the same size; adding
/// it to the end of the proportional family instead leaves every other
/// character coming from where it already did, and borrows only the arrows.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    let monospace = fonts
        .families
        .get(&egui::FontFamily::Monospace)
        .cloned()
        .unwrap_or_default();

    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .extend(monospace);

    ctx.set_fonts(fonts);
}

/// Draws the hints.
///
/// They wrap onto another line rather than scrolling: a hint that has to be
/// scrolled into view is no more discoverable than no hint at all.
pub fn show(ui: &mut egui::Ui, hints: &[Hint]) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;

        for hint in hints {
            ui.label(job(ui, hint));
        }
    });
}

/// Renders one hint, with its keys picked out the way a picker picks out the
/// letter that chooses an item: by colour alone, so that the whole bar is set
/// in the one font at the one size.
fn job(ui: &egui::Ui, hint: &Hint) -> LayoutJob {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let weak = ui.visuals().weak_text_color();

    let mut job = LayoutJob::default();

    job.append(
        &hint.keys,
        0.0,
        egui::TextFormat::simple(font.clone(), ui.visuals().hyperlink_color),
    );
    job.append(SEPARATOR, 0.0, egui::TextFormat::simple(font.clone(), weak));
    job.append(&hint.action, 0.0, egui::TextFormat::simple(font, weak));

    job
}
