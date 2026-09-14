use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use clap::Parser;

mod app;
mod db;
#[cfg(target_os = "macos")]
mod macos_menu;
mod picker;
mod results;
#[cfg(test)]
mod tests;

#[derive(Parser)]
#[command(name = "dbdrillui")]
#[command(about = "A PostgreSQL database drilling tool")]
#[command(version)]
struct Args {
    /// PostgreSQL database connection string (DSN)
    #[arg(
        long,
        env = "DB_DSN",
        help = "PostgreSQL database connection string (e.g., postgres://user:password@host:port/database)"
    )]
    db_dsn: Option<String>,

    /// Path to the TOML resources file
    #[arg(help = "Path to the TOML file containing resources configuration")]
    resources_file: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let Some(db_dsn) = args.db_dsn else {
        bail!(
            "No DB DSN provided. Use either --db-dsn or the DB_DSN environment variable to provide it."
        );
    };

    let resources = Arc::new(
        dbdrill_core::config::load(&args.resources_file).context("error loading resources file")?,
    );

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1024.0, 720.0])
            .with_title("dbdrill"),
        ..Default::default()
    };

    // Connecting happens on the database thread, so that an unreachable host
    // shows up as a message in the window rather than as a hang before it.
    eframe::run_native(
        "dbdrill",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_theme(egui::ThemePreference::Dark);

            // A menu bar we cannot install is not worth failing to start over.
            #[cfg(target_os = "macos")]
            if let Err(err) = macos_menu::install() {
                eprintln!("error installing the menu bar: {err}");
            }

            let ctx = cc.egui_ctx.clone();
            let db = db::Db::connect(db_dsn, Arc::clone(&resources), move || {
                ctx.request_repaint()
            });

            Ok(Box::new(app::App::new(resources, db)))
        }),
    )
    .map_err(|err| anyhow::anyhow!("error starting the UI: {err}"))
}
