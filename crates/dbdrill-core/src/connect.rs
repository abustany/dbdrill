use anyhow::{Context, Result};

pub fn connect(db_dsn: &str) -> Result<postgres::Client> {
    let connector = native_tls::TlsConnector::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .context("error setting up TLS")?;
    let connector = postgres_native_tls::MakeTlsConnector::new(connector);

    postgres::Client::connect(db_dsn, connector).context("error connecting to DB")
}
