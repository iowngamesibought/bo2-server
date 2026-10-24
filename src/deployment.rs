//! Deployment configuration; examples contain no client-specific identifiers.
use serde::Deserialize;
use std::{
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deployment {
    pub bind: SocketAddr,
    pub database: PathBuf,
    pub executable: Option<PathBuf>,
    pub profile: Option<PathBuf>,
    pub experimental_bootstrap: bool,
}
impl Deployment {
    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        if std::fs::metadata(path)?.len() > 65_536 {
            return Err("config exceeds size limit".into());
        }
        let mut config: Self = toml::from_str(&std::fs::read_to_string(path)?)?;
        let local = |ip: IpAddr| matches!(ip, IpAddr::V4(a) if a.is_loopback() || a.is_private());
        if !config.experimental_bootstrap {
            return Err("explicit experimental_bootstrap=true required".into());
        }
        if !local(config.bind.ip()) || config.bind.port() == 0 {
            return Err("explicit private IPv4/loopback bind with nonzero port required".into());
        }
        if config.executable.is_some() == config.profile.is_some() {
            return Err("set exactly one of executable or profile".into());
        }
        let input = config
            .executable
            .as_mut()
            .or(config.profile.as_mut())
            .unwrap();
        if config.database.as_os_str().is_empty() || input.as_os_str().is_empty() {
            return Err("input and database paths must not be empty".into());
        }
        let base = path
            .canonicalize()?
            .parent()
            .ok_or("config has no directory")?
            .to_path_buf();
        if config.database.is_relative() {
            config.database = base.join(&config.database);
        }
        if input.is_relative() {
            *input = base.join(&*input);
        }
        if config.database == *input {
            return Err("database must differ from input".into());
        }
        Ok(config)
    }
}
