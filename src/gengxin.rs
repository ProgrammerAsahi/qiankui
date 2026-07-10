use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs::{self, File},
    io::Write,
    path::Path,
    process::Command,
    time::Duration,
};

use anyhow::{Context, ensure};
use minisign_verify::{PublicKey, Signature};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use url::Url;

pub const DEFAULT_MANIFEST_URL: &str =
    "https://github.com/ProgrammerAsahi/qiankui/releases/latest/download/qiankui-update.json";

const RELEASE_PUBLIC_KEY: &str = include_str!("../packaging/qiankui-release.pub");
const MANIFEST_LIMIT: u64 = 64 * 1024;
const SIGNATURE_LIMIT: u64 = 16 * 1024;
const BINARY_LIMIT: u64 = 100 * 1024 * 1024;
const USER_AGENT: &str = concat!("qiankui/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Eq, PartialEq)]
pub enum UpdateStatus {
    UpToDate { current: Version, latest: Version },
    Available { current: Version, latest: Version },
    ManagedByHomebrew { current: Version, latest: Version },
    Updated { previous: Version, current: Version },
}

#[derive(Debug, Deserialize)]
struct UpdateManifest {
    schema_version: u32,
    channel: String,
    version: String,
    published_at: String,
    artifacts: BTreeMap<String, ManifestArtifact>,
}

#[derive(Debug, Deserialize)]
struct ManifestArtifact {
    url: String,
    sha256: String,
    size: u64,
}

#[derive(Debug)]
struct ValidatedUpdate {
    version: Version,
    artifact: ValidatedArtifact,
}

#[derive(Debug)]
struct ValidatedArtifact {
    url: Url,
    sha256: String,
    size: u64,
}

pub fn update(check_only: bool) -> anyhow::Result<UpdateStatus> {
    let current = Version::parse(env!("CARGO_PKG_VERSION"))
        .context("the built-in client version is invalid")?;
    let target = supported_target()?;
    let manifest_url = parse_https_url(DEFAULT_MANIFEST_URL, "update manifest URL")?;
    let agent = http_agent();
    let update = fetch_update(&agent, &manifest_url, target)?;

    if update.version <= current {
        return Ok(UpdateStatus::UpToDate {
            current,
            latest: update.version,
        });
    }
    if check_only {
        return Ok(UpdateStatus::Available {
            current,
            latest: update.version,
        });
    }

    let executable = std::env::current_exe().context("could not locate the running qiankui")?;
    if is_homebrew_managed(&executable) {
        return Ok(UpdateStatus::ManagedByHomebrew {
            current,
            latest: update.version,
        });
    }

    let binary = fetch_bytes(&agent, &update.artifact.url, update.artifact.size + 1)
        .context("could not download the client update")?;
    verify_artifact(&binary, &update.artifact)?;
    install_candidate(&executable, &binary, &update.version)?;

    Ok(UpdateStatus::Updated {
        previous: current,
        current: update.version,
    })
}

fn http_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .https_only(true)
        .max_redirects(5)
        .timeout_global(Some(Duration::from_secs(90)))
        .build()
        .into()
}

fn fetch_update(
    agent: &ureq::Agent,
    manifest_url: &Url,
    target: &str,
) -> anyhow::Result<ValidatedUpdate> {
    let manifest = fetch_bytes(agent, manifest_url, MANIFEST_LIMIT)
        .context("could not download the update manifest")?;
    let signature_url = signature_url(manifest_url)?;
    let signature = fetch_bytes(agent, &signature_url, SIGNATURE_LIMIT)
        .context("could not download the update manifest signature")?;
    let signature = std::str::from_utf8(&signature).context("manifest signature is not UTF-8")?;

    verify_signature(&manifest, signature, RELEASE_PUBLIC_KEY)?;
    validate_manifest(&manifest, target)
}

fn fetch_bytes(agent: &ureq::Agent, url: &Url, limit: u64) -> anyhow::Result<Vec<u8>> {
    let mut response = agent
        .get(url.as_str())
        .header("User-Agent", USER_AGENT)
        .call()
        .with_context(|| format!("GET {url} failed"))?;
    response
        .body_mut()
        .with_config()
        .limit(limit)
        .read_to_vec()
        .with_context(|| format!("could not read {url}"))
}

fn verify_signature(content: &[u8], encoded: &str, public_key: &str) -> anyhow::Result<()> {
    let public_key = PublicKey::decode(public_key).context("release public key is invalid")?;
    let signature = Signature::decode(encoded).context("manifest signature is malformed")?;
    public_key
        .verify(content, &signature, false)
        .context("manifest signature verification failed")
}

fn validate_manifest(content: &[u8], target: &str) -> anyhow::Result<ValidatedUpdate> {
    let mut manifest: UpdateManifest =
        serde_json::from_slice(content).context("update manifest is invalid JSON")?;
    ensure!(
        manifest.schema_version == 1,
        "unsupported update manifest schema {}",
        manifest.schema_version
    );
    ensure!(
        manifest.channel == "stable",
        "manifest channel is not stable"
    );
    ensure!(
        !manifest.published_at.trim().is_empty(),
        "manifest publication time is missing"
    );

    let version = Version::parse(&manifest.version).context("manifest version is invalid")?;
    ensure!(
        version.pre.is_empty() && version.build.is_empty(),
        "stable manifest version must not contain prerelease or build metadata"
    );
    let artifact = manifest
        .artifacts
        .remove(target)
        .with_context(|| format!("manifest has no client for {target}"))?;
    ensure!(
        artifact.size > 0 && artifact.size <= BINARY_LIMIT,
        "manifest artifact size is outside the allowed range"
    );
    ensure!(
        artifact.sha256.len() == 64
            && artifact
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "manifest artifact SHA-256 is not canonical"
    );

    Ok(ValidatedUpdate {
        version,
        artifact: ValidatedArtifact {
            url: parse_https_url(&artifact.url, "artifact URL")?,
            sha256: artifact.sha256,
            size: artifact.size,
        },
    })
}

fn parse_https_url(value: &str, label: &str) -> anyhow::Result<Url> {
    let url = Url::parse(value).with_context(|| format!("{label} is invalid"))?;
    ensure!(url.scheme() == "https", "{label} must use HTTPS");
    ensure!(url.host_str().is_some(), "{label} has no host");
    ensure!(
        url.username().is_empty() && url.password().is_none(),
        "{label} must not contain credentials"
    );
    Ok(url)
}

fn signature_url(manifest_url: &Url) -> anyhow::Result<Url> {
    ensure!(
        !manifest_url.path().ends_with('/'),
        "manifest URL must name a file"
    );
    let mut url = manifest_url.clone();
    url.set_path(&format!("{}.minisig", manifest_url.path()));
    Ok(url)
}

fn verify_artifact(binary: &[u8], artifact: &ValidatedArtifact) -> anyhow::Result<()> {
    ensure!(
        binary.len() as u64 == artifact.size,
        "downloaded client size differs from the signed manifest"
    );
    ensure!(
        sha256_hex(binary) == artifact.sha256,
        "downloaded client SHA-256 differs from the signed manifest"
    );
    Ok(())
}

fn sha256_hex(content: &[u8]) -> String {
    Sha256::digest(content)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(unix)]
fn install_candidate(path: &Path, binary: &[u8], version: &Version) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let parent = path
        .parent()
        .context("the running client has no parent directory")?;
    let mode = fs::metadata(path)
        .with_context(|| format!("could not inspect {}", path.display()))?
        .permissions()
        .mode();
    let mut candidate = tempfile::Builder::new()
        .prefix(".qiankui-update-")
        .tempfile_in(parent)
        .with_context(|| format!("could not stage an update in {}", parent.display()))?;
    candidate
        .as_file()
        .set_permissions(fs::Permissions::from_mode(mode))?;
    candidate.write_all(binary)?;
    candidate.as_file().sync_all()?;
    let candidate = candidate.into_temp_path();

    let output = Command::new(&candidate)
        .arg("--version")
        .output()
        .context("downloaded client could not be executed")?;
    ensure!(
        output.status.success(),
        "downloaded client failed its version check"
    );
    let reported = std::str::from_utf8(&output.stdout)
        .context("downloaded client returned a non-UTF-8 version")?
        .trim();
    ensure!(
        reported == format!("qiankui {version}"),
        "downloaded client reports an unexpected version: {reported}"
    );

    candidate
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("could not replace {} atomically", path.display()))?;
    if let Ok(directory) = File::open(parent) {
        let _ = directory.sync_all();
    }
    Ok(())
}

#[cfg(not(unix))]
fn install_candidate(_path: &Path, _binary: &[u8], _version: &Version) -> anyhow::Result<()> {
    anyhow::bail!("self-update is not supported on this operating system")
}

fn is_homebrew_managed(path: &Path) -> bool {
    let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    canonical
        .components()
        .any(|component| component.as_os_str() == OsStr::new("Cellar"))
}

fn supported_target() -> anyhow::Result<&'static str> {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        Ok("aarch64-apple-darwin")
    }
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    {
        anyhow::bail!("v0.1 updates are available only for macOS arm64")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_PUBLIC_KEY: &str = "untrusted comment: minisign public key E7620F1842B4E81F\n\
RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3\n";
    const TEST_SIGNATURE: &str = "untrusted comment: signature from minisign secret key\n\
RUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=\n\
trusted comment: timestamp:1556193335\tfile:test\n\
y/rUw2y8/hOUYjZU71eHp/Wo1KZ40fGy2VJEDl34XMJM+TX48Ss/17u3IvIfbVR1FkZZSNCisQbuQY+bHwhEBg==\n";

    fn manifest() -> Vec<u8> {
        br#"{
          "schema_version": 1,
          "channel": "stable",
          "version": "0.2.0",
          "published_at": "2026-07-10T00:00:00Z",
          "artifacts": {
            "aarch64-apple-darwin": {
              "url": "https://github.com/ProgrammerAsahi/qiankui/releases/download/v0.2.0/qiankui-aarch64-apple-darwin",
              "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
              "size": 123
            }
          }
        }"#
        .to_vec()
    }

    #[test]
    fn verifies_prehashed_minisign_signature() -> anyhow::Result<()> {
        verify_signature(b"test", TEST_SIGNATURE, TEST_PUBLIC_KEY)?;
        assert!(verify_signature(b"tampered", TEST_SIGNATURE, TEST_PUBLIC_KEY).is_err());
        Ok(())
    }

    #[test]
    fn release_public_key_is_well_formed() -> anyhow::Result<()> {
        PublicKey::decode(RELEASE_PUBLIC_KEY)?;
        Ok(())
    }

    #[test]
    fn validates_stable_target_artifact() -> anyhow::Result<()> {
        let update = validate_manifest(&manifest(), "aarch64-apple-darwin")?;
        assert_eq!(update.version, Version::new(0, 2, 0));
        assert_eq!(update.artifact.size, 123);
        Ok(())
    }

    #[test]
    fn rejects_missing_target_and_non_https_artifact() {
        assert!(validate_manifest(&manifest(), "x86_64-apple-darwin").is_err());
        let insecure = String::from_utf8(manifest())
            .unwrap()
            .replace("https://github.com", "http://github.com");
        assert!(validate_manifest(insecure.as_bytes(), "aarch64-apple-darwin").is_err());
    }

    #[test]
    fn recognizes_homebrew_cellar() {
        assert!(is_homebrew_managed(Path::new(
            "/opt/homebrew/Cellar/qiankui/0.1.0/bin/qiankui"
        )));
        assert!(!is_homebrew_managed(Path::new(
            "/Users/example/.cargo/bin/qiankui"
        )));
    }

    #[test]
    fn derives_signature_url_and_hash() -> anyhow::Result<()> {
        let manifest = Url::parse(DEFAULT_MANIFEST_URL)?;
        assert_eq!(
            signature_url(&manifest)?.as_str(),
            "https://github.com/ProgrammerAsahi/qiankui/releases/latest/download/qiankui-update.json.minisig"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn atomically_installs_a_version_checked_candidate() -> anyhow::Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let temporary = tempfile::tempdir()?;
        let destination = temporary.path().join("qiankui");
        fs::write(&destination, b"old client")?;
        fs::set_permissions(&destination, fs::Permissions::from_mode(0o755))?;
        let candidate = b"#!/bin/sh\nprintf 'qiankui 0.2.0\\n'\n";

        install_candidate(&destination, candidate, &Version::new(0, 2, 0))?;

        assert_eq!(fs::read(&destination)?, candidate);
        assert_eq!(
            fs::metadata(&destination)?.permissions().mode() & 0o777,
            0o755
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn failed_candidate_check_preserves_current_client() -> anyhow::Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let temporary = tempfile::tempdir()?;
        let destination = temporary.path().join("qiankui");
        fs::write(&destination, b"old client")?;
        fs::set_permissions(&destination, fs::Permissions::from_mode(0o755))?;
        let wrong_version = b"#!/bin/sh\nprintf 'qiankui 9.9.9\\n'\n";

        assert!(install_candidate(&destination, wrong_version, &Version::new(0, 2, 0)).is_err());
        assert_eq!(fs::read(&destination)?, b"old client");
        Ok(())
    }
}
