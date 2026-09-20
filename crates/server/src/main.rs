//! The deployable Meridian API server.
//!
//! meridian-server --store /var/lib/meridian --token "$SECRET" --port 8080
//!
//! ponytail: binds loopback only. Put it behind Caddy or nginx for TLS and for reaching it from
//! anywhere else. One store, one token, one user; accounts are a separate project.

use std::path::PathBuf;

use clap::Parser;

use meridian_core::config::{Config, Scope};

#[derive(Parser)]
#[command(name = "meridian-server", version)]
struct Cli {
    /// Directory holding portfolios.json and quotes.json.
    #[arg(long)]
    store: Option<PathBuf>,
    /// Shared secret clients must send as X-Meridian-Token. Read from MERIDIAN_TOKEN if unset.
    #[arg(long)]
    token: Option<String>,
    #[arg(long, default_value_t = 8080)]
    port: u16,
    /// norway, scandinavia, nordics, europe or global.
    #[arg(long, default_value = "scandinavia")]
    scope: String,
}

fn main() {
    let cli = Cli::parse();
    let Some(scope) = Scope::parse(&cli.scope) else {
        eprintln!("meridian-server: unknown scope {:?}", cli.scope);
        std::process::exit(2);
    };
    // ponytail: the token comes from the environment by preference. argv is world-readable in ps.
    let token = cli
        .token
        .or_else(|| std::env::var("MERIDIAN_TOKEN").ok())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| {
            let t = meridian_server::new_token();
            eprintln!("meridian-server: no --token or MERIDIAN_TOKEN, generated one: {t}");
            t
        });
    let store = cli.store.unwrap_or_else(Config::default_store_dir);
    let cfg = Config::new(store.clone(), scope);
    match meridian_server::serve(cfg, cli.port, token) {
        Ok(port) => {
            println!(
                "meridian-server {} on 127.0.0.1:{port}, store {}",
                meridian_server::VERSION,
                store.display()
            );
            loop {
                std::thread::park();
            }
        }
        Err(e) => {
            eprintln!("meridian-server: could not serve: {e}");
            std::process::exit(1);
        }
    }
}
