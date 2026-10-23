//! `af_server`: see `--help` (crate::config::USAGE).

use std::net::TcpListener;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use af_core::prelude::*;
use af_server::accounts::{Accounts, MemoryAccounts, Supabase};
use af_server::ServerConfig;

fn main() {
    let config = match ServerConfig::parse(std::env::args().skip(1), |k| std::env::var(k).ok()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(if e.starts_with("af_server") { 0 } else { 2 });
        }
    };
    let data_dir = match &config.data_dir {
        Some(d) => std::path::PathBuf::from(d),
        None => match af_data::find_data_dir() {
            Some(d) => d,
            None => {
                eprintln!("af_server: no assets/data folder found (pass --data DIR or run next to the game's assets)");
                std::process::exit(2);
            }
        },
    };
    let data = match GameData::load_dir(&data_dir) {
        Ok(d) => Arc::new(d),
        Err(e) => {
            eprintln!("af_server: game content failed to load\n{e}");
            std::process::exit(2);
        }
    };
    let content_hash = match af_data::content_hash(&data_dir) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("af_server: could not hash {}: {e}", data_dir.display());
            std::process::exit(2);
        }
    };
    let accounts: Option<Arc<dyn Accounts>> = match (&config.supabase, config.ranked_memory) {
        (Some(s), _) => Some(Arc::new(Supabase::new(&s.url, &s.anon_key, &s.service_key))),
        (None, true) => Some(Arc::new(MemoryAccounts::default())),
        (None, false) => None,
    };
    let listener = match TcpListener::bind(&config.bind) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("af_server: cannot listen on {}: {e}", config.bind);
            std::process::exit(2);
        }
    };
    println!(
        "af_server listening on {} | content {:016x} | ranked: {}",
        config.bind,
        content_hash,
        match (&config.supabase, config.ranked_memory) {
            (Some(s), _) => format!("Supabase at {}", s.url),
            (None, true) => "in-memory accounts".into(),
            (None, false) => "off".into(),
        }
    );
    af_server::run(config, listener, data, content_hash, accounts, Arc::new(AtomicBool::new(false)));
}
