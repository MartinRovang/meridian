//! The Meridian desktop app.
//!
//! By default it runs the API in-process on a loopback port with a fresh token, so there is one
//! file to install and nothing to configure. Given --api-url it connects to a server you host
//! instead, and starts nothing locally.

use clap::Parser;

use meridian_core::config::{Config, Scope};

mod shell;

#[derive(Parser)]
#[command(name = "meridian", version)]
struct Cli {
    /// Connect to a hosted API instead of running one in-process, e.g. https://box.example.
    #[arg(long)]
    api_url: Option<String>,
    /// Shared secret for a hosted API. Read from MERIDIAN_TOKEN if unset.
    #[arg(long)]
    token: Option<String>,
    /// Port for the in-process server. 0 picks a free one.
    #[arg(long, default_value_t = 0)]
    port: u16,
    /// Market scope for the in-process server: norway, scandinavia, nordics, europe or global.
    #[arg(long, default_value = "scandinavia")]
    scope: String,
}

fn main() {
    #[cfg(target_os = "linux")]
    if std::env::var_os("GST_PLUGIN_FEATURE_RANK").is_none() {
        // ponytail: same WebKitGTK decoder problem gitdashy hit on hybrid laptops.
        unsafe {
            std::env::set_var(
                "GST_PLUGIN_FEATURE_RANK",
                "nvvp8dec:0,nvvp9dec:0,nvh264dec:0,nvh265dec:0,nvav1dec:0",
            );
        }
    }
    let cli = Cli::parse();
    let (base, token) = match &cli.api_url {
        Some(url) => {
            let token = cli
                .token
                .clone()
                .or_else(|| std::env::var("MERIDIAN_TOKEN").ok())
                .unwrap_or_default();
            if token.is_empty() {
                eprintln!("meridian: --api-url needs --token or MERIDIAN_TOKEN");
                std::process::exit(2);
            }
            (url.trim_end_matches('/').to_string(), token)
        }
        None => {
            let Some(scope) = Scope::parse(&cli.scope) else {
                eprintln!("meridian: unknown scope {:?}", cli.scope);
                std::process::exit(2);
            };
            let token = meridian_server::new_token();
            let cfg = Config::new(Config::default_store_dir(), scope);
            match meridian_server::serve(cfg, cli.port, token.clone()) {
                Ok(port) => (format!("http://127.0.0.1:{port}"), token),
                Err(e) => {
                    eprintln!("meridian: could not start the local api: {e}");
                    std::process::exit(1);
                }
            }
        }
    };
    shell::run(&base, &token);
}
