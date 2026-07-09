use std::{
    fmt,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

use crate::fujie::Token;

const DEFAULT_LISTEN: &str = "127.0.0.1:1080";
const DEFAULT_CONNECT_TIMEOUT_MS: u64 = 10_000;

/// 近端所用之简牍。符节仅存于使用者本机，不应入库。
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientConfig {
    pub relay: String,
    pub token: String,
    #[serde(default = "default_listen")]
    pub listen: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ca: Option<PathBuf>,
    #[serde(default)]
    pub insecure: bool,
    #[serde(default = "default_connect_timeout_ms")]
    pub connect_timeout_ms: u64,
}

impl ClientConfig {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("could not read config {}", path.display()))?;
        let mut config: Self = toml::from_str(&contents)
            .with_context(|| format!("could not parse config {}", path.display()))?;
        config.resolve_relative_paths(path);
        config.validate()?;
        Ok(config)
    }

    pub fn save(&self, path: &Path, force: bool) -> anyhow::Result<()> {
        self.validate()?;
        if path.exists() && !force {
            bail!(
                "config {} already exists; pass --force to replace it",
                path.display()
            );
        }

        let parent = path
            .parent()
            .context("config path must have a parent directory")?;
        fs::create_dir_all(parent)
            .with_context(|| format!("could not create config directory {}", parent.display()))?;
        secure_directory(parent)?;

        let temporary = path.with_extension(format!("tmp.{}", std::process::id()));
        let serialized = toml::to_string_pretty(self).context("could not serialize config")?;
        let result = (|| -> anyhow::Result<()> {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temporary).with_context(|| {
                format!("could not create temporary config {}", temporary.display())
            })?;
            file.write_all(serialized.as_bytes())?;
            file.sync_all()?;

            #[cfg(windows)]
            if force && path.exists() {
                fs::remove_file(path)?;
            }
            fs::rename(&temporary, path)
                .with_context(|| format!("could not install config {}", path.display()))?;
            secure_file(path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.relay.trim().is_empty() {
            bail!("relay is required");
        }
        if self.listen.trim().is_empty()
            || self.listen.bytes().any(|byte| byte.is_ascii_whitespace())
        {
            bail!("listen address is invalid");
        }
        Token::parse(self.token.clone())?;
        if !(1..=300_000).contains(&self.connect_timeout_ms) {
            bail!("connect-timeout must be between 1 and 300000 milliseconds");
        }
        if let Some(path) = &self.ca
            && !path.is_file()
        {
            bail!("CA file {} does not exist or is not a file", path.display());
        }
        Ok(())
    }

    fn resolve_relative_paths(&mut self, config_path: &Path) {
        if let Some(path) = &self.ca
            && path.is_relative()
            && let Some(parent) = config_path.parent()
        {
            self.ca = Some(parent.join(path));
        }
    }
}

impl fmt::Debug for ClientConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClientConfig")
            .field("relay", &self.relay)
            .field("token", &"[REDACTED]")
            .field("listen", &self.listen)
            .field("ca", &self.ca)
            .field("insecure", &self.insecure)
            .field("connect_timeout_ms", &self.connect_timeout_ms)
            .finish()
    }
}

pub fn default_path() -> anyhow::Result<PathBuf> {
    if let Some(path) = std::env::var_os("QIANKUI_CONFIG")
        && !path.is_empty()
    {
        return Ok(PathBuf::from(path));
    }
    let home = std::env::var_os("HOME").context("HOME is not set; pass --config")?;
    Ok(PathBuf::from(home)
        .join(".config")
        .join("qiankui")
        .join("config.toml"))
}

fn default_listen() -> String {
    DEFAULT_LISTEN.to_owned()
}

fn default_connect_timeout_ms() -> u64 {
    DEFAULT_CONNECT_TIMEOUT_MS
}

#[cfg(unix)]
fn secure_directory(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn secure_directory(_path: &Path) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn secure_file(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn secure_file(_path: &Path) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_loads_and_redacts_config() -> anyhow::Result<()> {
        let temporary = tempfile::tempdir()?;
        let ca_path = temporary.path().join("ca.pem");
        fs::write(&ca_path, "certificate")?;
        let path = temporary.path().join("nested/config.toml");
        let config = ClientConfig {
            relay: "https://relay.example:8443".to_owned(),
            token: "a-valid-token-with-enough-bytes".to_owned(),
            listen: DEFAULT_LISTEN.to_owned(),
            ca: Some(ca_path.clone()),
            insecure: false,
            connect_timeout_ms: DEFAULT_CONNECT_TIMEOUT_MS,
        };

        config.save(&path, false)?;
        let loaded = ClientConfig::load(&path)?;
        assert_eq!(loaded.relay, config.relay);
        assert_eq!(loaded.token, config.token);
        assert_eq!(loaded.ca, Some(ca_path));
        assert!(!format!("{loaded:?}").contains(&config.token));
        assert!(config.save(&path, false).is_err());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
        }
        Ok(())
    }

    #[test]
    fn resolves_ca_relative_to_config() -> anyhow::Result<()> {
        let temporary = tempfile::tempdir()?;
        let ca_path = temporary.path().join("ca.pem");
        fs::write(&ca_path, "certificate")?;
        let path = temporary.path().join("config.toml");
        fs::write(
            &path,
            "relay = \"https://relay.example:8443\"\n\
             token = \"a-valid-token-with-enough-bytes\"\n\
             ca = \"ca.pem\"\n",
        )?;
        assert_eq!(ClientConfig::load(&path)?.ca, Some(ca_path));
        Ok(())
    }
}
