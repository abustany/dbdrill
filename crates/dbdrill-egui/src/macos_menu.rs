//! Native macOS menu bar.
//!
//! winit installs a minimal menu bar holding only the application menu, so the
//! standard shortcuts that AppKit drives from menu items - Ctrl-Cmd-F for full
//! screen, Cmd-M to minimize, Cmd-W to close - do nothing. This installs a
//! proper menu bar in its place.
//!
//! No Edit menu: the predefined copy and paste items go through the responder
//! chain, which the winit view does not answer, so they would show up greyed
//! out. egui handles those shortcuts itself anyway.

use std::cell::RefCell;

use muda::{Menu, PredefinedMenuItem, Submenu};

thread_local! {
    /// Dropping the menu releases the underlying `NSMenu`, so it is kept here
    /// for the lifetime of the process.
    static MENU: RefCell<Option<Menu>> = const { RefCell::new(None) };
}

/// Installs the menu bar, replacing the one winit set up. Must be called on
/// the main thread, once the event loop is running.
pub fn install() -> muda::Result<()> {
    let menubar = Menu::new();

    let app = Submenu::new("dbdrill", true);
    app.append_items(&[
        &PredefinedMenuItem::about(None, None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::services(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::hide(None),
        &PredefinedMenuItem::hide_others(None),
        &PredefinedMenuItem::show_all(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::quit(None),
    ])?;

    let view = Submenu::new("View", true);
    view.append_items(&[&PredefinedMenuItem::fullscreen(None)])?;

    let window = Submenu::new("Window", true);
    window.append_items(&[
        &PredefinedMenuItem::minimize(None),
        &PredefinedMenuItem::maximize(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::close_window(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::bring_all_to_front(None),
    ])?;

    menubar.append_items(&[&app, &view, &window])?;
    menubar.init_for_nsapp();
    // Gives the menu the window list and the standard window commands.
    window.set_as_windows_menu_for_nsapp();

    MENU.with(|menu| *menu.borrow_mut() = Some(menubar));

    Ok(())
}
