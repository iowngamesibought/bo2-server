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
    let mut export_kind = "bundle";
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!(
                    "bo2-server --config PATH [--check-config | --write-bundle PATH | --write-profile PATH | --write-endpoints PATH]\nLocal input and database paths resolve from the config directory.\nOffline checks/exports never open a database or listener. Profile/endpoint exports require executable mode.\nExperimental bootstrap is not platform-account authentication."
                );
                return Ok(());
            }
            "--config" => {
                config_path = Some(PathBuf::from(args.next().ok_or("missing config path")?))
            }
            "--check-config" => check = true,
            "--write-bundle" | "--write-profile" | "--write-endpoints" => {
                if output.is_some() {
                    return Err("choose only one output".into());
                }
                export_kind = match arg.as_str() {
                    "--write-profile" => "profile",
                    "--write-endpoints" => "endpoints",
                    _ => "bundle",
                };
                output = Some(args.next().ok_or("missing output path")?.into())
            }
            _ => return Err(format!("unknown argument: {arg}; use --help").into()),
        }
    }
    if check && output.is_some() {
        return Err("choose check or output, not both".into());
    }
    let path =
        config_path.ok_or("--config PATH is required; copy bo2-server.example.toml first")?;
    let deployment = Deployment::load(&path)?;
    let client = deployment
        .executable
        .as_ref()
        .map(|path| bo2_server::client_data::ClientData::load(path))
        .transpose()?;
    let profile = if let Some(client) = &client {
        client.profile.clone()
    } else {
        bo2_server::profile::Profile::load(deployment.profile.as_ref().unwrap())?
    };
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
        let bytes = match export_kind {
            "bundle" => assets.bundle,
            _ => {
                let client = client
                    .as_ref()
                    .ok_or("profile/endpoint export requires executable mode")?;
                let value = if export_kind == "profile" {
                    &client.profile_json
                } else {
                    &client.endpoints
                };
                let mut bytes = serde_json::to_vec_pretty(value)?;
                bytes.push(b'\n');
                bytes
            }
        };
        // Export only to a new file; never replace an input, custom profile or database.
        use std::io::Write;
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?
            .write_all(&bytes)?;
        println!("{export_kind}.written bytes={}", bytes.len());
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
