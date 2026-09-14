use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use clap::Parser;
use dbdrill_core::session::Session;

mod tui;

#[derive(Parser)]
#[command(name = "dbdrill")]
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
            "No DB DSN provided. Use either --db-dn or the DB_DSN environment variable to provide it."
        );
    };

    println!("Database DSN: {db_dsn}");
    println!("Resources file: {}", args.resources_file.display());

    let resources = Arc::new(
        dbdrill_core::config::load(&args.resources_file).context("error loading resources file")?,
    );

    println!("Connecting to the DB...");
    let session = Session::connect(&db_dsn, resources)?;

    tui::start(session);

    Ok(())
}
