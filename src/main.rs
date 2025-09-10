use std::path::PathBuf;
use std::{collections::HashMap, fs};

use anyhow::{Context, Result};
use clap::Parser;

mod model;
use model::Resource;

mod sql_value_as_string;

mod tui;

#[derive(Parser)]
#[command(name = "dbdrill")]
#[command(about = "A PostgreSQL database drilling tool")]
#[command(version)]
struct Args {
    /// PostgreSQL database connection string (DSN)
    #[arg(
        help = "PostgreSQL database connection string (e.g., postgres://user:password@host:port/database)"
    )]
    db_dsn: String,

    /// Path to the TOML resources file
    #[arg(help = "Path to the TOML file containing resources configuration")]
    resources_file: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();

    println!("Database DSN: {}", args.db_dsn);
    println!("Resources file: {}", args.resources_file.display());

    let resources: HashMap<String, Resource> = toml::from_str(
        &fs::read_to_string(&args.resources_file).context("error opening resources file")?,
    )
    .context("error parsing resources files")?;

    model::validate_resources(&resources).context("error validating resources")?;

    println!("Connecting to the DB...");
    let db_connector = native_tls::TlsConnector::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .context("error setting up TLS")?;
    let db_connector = postgres_native_tls::MakeTlsConnector::new(db_connector);
    let db =
        postgres::Client::connect(&args.db_dsn, db_connector).context("error connecting to DB")?;

    tui::start(db, resources);

    Ok(())
}
