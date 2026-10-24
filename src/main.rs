use bo2_server::{
    deployment::Deployment,
    league_config,
    league_store::Store,
    profile::PublisherAssets,
    server::{Config, serve},
};
use std::{
    net::TcpListener,
    path::PathBuf,
    sync::{Arc, Mutex, atomic::AtomicBool},
    time::Duration,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config_path = None;
    let mut check = false;
    let mut output: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!(
                    "bo2-server --config PATH [--check-config | --write-bundle PATH]\nLocal profile and database paths resolve from the config directory.\n--check-config validates and renders startup data without opening the database or a socket.\nExperimental bootstrap is not platform-account authentication."
                );
                return Ok(());
            }
            "--config" => {
                config_path = Some(PathBuf::from(args.next().ok_or("missing config path")?))
            }
            "--check-config" => check = true,
            "--write-bundle" => {
                output = Some(args.next().ok_or("missing bundle output path")?.into())
            }
            _ => return Err(format!("unknown argument: {arg}; use --help").into()),
        }
    }
    if check && output.is_some() {
        return Err("choose check or bundle output, not both".into());
    }
    let path =
        config_path.ok_or("--config PATH is required; copy bo2-server.example.toml first")?;
    let deployment = Deployment::load(&path)?;
    let profile = bo2_server::profile::Profile::load(&deployment.profile)?;
    let assets = PublisherAssets::from_profile(&profile, league_config::WAD_TIMESTAMP)?;
    if check {
        println!(
            "config.valid leagues={} startup_bytes={} database_not_opened=true listener_not_started=true",
            profile.leagues.len(),
            assets.bundle.len()
        );
        return Ok(());
    }
    if let Some(path) = output {
        std::fs::write(&path, &assets.bundle)?;
        println!("bundle.written bytes={}", assets.bundle.len());
        return Ok(());
    }
    if let Some(parent) = deployment.database.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut store = Store::open(&deployment.database)?;
    league_config::seed(&mut store, &profile)?;
    let mut config = Config::diagnostic();
    config.league = Some(Arc::new(Mutex::new(store)));
    config.publisher = Arc::new(assets);
    config.serve_time = true;
    config.idle_timeout = Duration::from_secs(300);
    config.session_limit = Duration::from_secs(6 * 3600);
    config.max_tasks = 100_000;
    let listener = TcpListener::bind(deployment.bind)?;
    println!(
        "server.listening address={} mode=league experimental=true",
        listener.local_addr()?
    );
    serve(listener, config, Arc::new(AtomicBool::new(false)))?;
    Ok(())
}
