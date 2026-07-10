# Qiankui (潜逵)

**English** | [文言](README.wy.md)

> Hidden ways converge; different paths reach the same destination.

Qiankui is a small, self-hosted private networking relay written in Rust. A local client accepts SOCKS5 connections, forwards each TCP stream through TLS 1.3 and HTTP `CONNECT`, and asks a remote relay to connect to the destination after authenticating the client.

Version 0.1.0 deliberately keeps the system small and inspectable. It supports TCP, one TLS connection per proxied stream, a single bearer token, and a replaceable transport boundary. It does not yet provide UDP, TUN, system-wide routing, PAC rules, multiplexing, or a control plane.

## Architecture

```text
Local application
  | SOCKS5
  v
qiankui                         runs on your computer
  | TLS 1.3 + HTTP CONNECT
  v
qiankui-relay                   Azure Container Apps or your own server
  | TCP
  v
Destination service
```

Only `qiankui` belongs on the client computer. Only `qiankui-relay` belongs on the server. Neither binary needs Cargo or a Rust toolchain at runtime.

The current client is an application-level SOCKS5 proxy, not a global or smart-routing proxy. Configure individual applications to use `127.0.0.1:1080`; applications that do not use that SOCKS5 endpoint keep their normal network path.

## Install

The packaged client currently supports Apple Silicon Macs:

```sh
brew install ProgrammerAsahi/qiankui/qiankui
```

You may also download `qiankui-aarch64-apple-darwin` from [GitHub Releases](https://github.com/ProgrammerAsahi/qiankui/releases). Server binaries are not included in the client package.

Check for updates:

```sh
qiankui update --check
```

Clients installed directly from GitHub can update themselves:

```sh
qiankui update
```

Homebrew installations remain managed by Homebrew. When a newer release exists, the client directs you to:

```sh
brew upgrade ProgrammerAsahi/qiankui/qiankui
```

Update manifests are signed with Minisign. The client verifies the manifest signature, artifact size, SHA-256 digest, and reported binary version before atomically replacing an executable. See [Release process](docs/RELEASE.md) for the complete publishing model.

## Quick Start

### Included Azure deployment

If you deployed Qiankui with the included Azure infrastructure and your Azure account can read its Key Vault, the repository helper performs the client setup in one command:

```sh
az login
deploy/azure/configure-client.sh --region japaneast
```

The helper discovers the regional Container App endpoint, downloads the private CA certificate and relay token from Key Vault, and writes the client configuration to `~/.config/qiankui/config.toml`. It never prints the token. The current helper installs the client from the checked-out source, so this path requires Git and the pinned Rust toolchain.

Start the proxy:

```sh
qiankui
```

Test it from another terminal:

```sh
curl --proxy socks5h://127.0.0.1:1080 https://example.com
```

Use `socks5h://` when possible so destination DNS resolution happens through the relay.

### Any self-hosted relay

Initialize the client once. If `--token` is omitted, Qiankui prompts for it without echoing it:

```sh
qiankui config init \
  --relay https://relay.example.com:8443
```

For a relay certificate issued by a private CA:

```sh
qiankui config init \
  --relay https://relay.example.com:8443 \
  --ca ~/.config/qiankui/relay-ca.pem
```

After initialization, daily use is simply:

```sh
qiankui
```

Inspect the effective stored configuration without revealing the token:

```sh
qiankui config show
qiankui config path
```

The default configuration file is `~/.config/qiankui/config.toml`. Qiankui creates the directory with mode `0700` and the file with mode `0600`. `--config` or `QIANKUI_CONFIG` selects another file. Command-line options and `QIANKUI_TOKEN` can temporarily override stored values without rewriting the file.

## Client Options

```text
config init            Create or replace the local configuration
config show            Show configuration with the token redacted
config path            Print the configuration path
run                    Start the SOCKS5 listener; also the default command
update                 Verify and install a newer signed client release
update --check         Check for an update without installing it
--config <toml>        Use a different configuration file
--listen <host:port>   SOCKS5 listener; default 127.0.0.1:1080
--relay <https-url>    Relay endpoint
--token <secret>       Bearer token; prefer the config file or QIANKUI_TOKEN
--ca <pem>             Private CA certificate or self-signed certificate
--insecure             Disable TLS verification for local testing only
--connect-timeout <ms> Connection timeout; default 10000
```

## Local End-to-End Test

Create a temporary development certificate:

```sh
make dev-cert
```

Start a local relay:

```sh
export QIANKUI_TOKEN='change-this-token-at-least-16-bytes'
cargo run --bin qiankui-relay -- \
  --listen 127.0.0.1:8443 \
  --cert var/dev-cert.pem \
  --key var/dev-key.pem \
  --ports 80,443
```

In another terminal, start the client:

```sh
export QIANKUI_TOKEN='change-this-token-at-least-16-bytes'
cargo run --bin qiankui -- \
  --listen 127.0.0.1:1080 \
  --relay https://127.0.0.1:8443 \
  --ca var/dev-cert.pem
```

Then test the complete path:

```sh
curl --proxy socks5h://127.0.0.1:1080 https://example.com
```

`--insecure` exists only for short-lived local debugging. Do not use it for normal operation.

## Relay Deployment

Qiankui does not depend on a specific cloud provider. The repository contains two deployment paths:

- A conventional Linux host with a systemd unit: [Deployment guide](docs/DEPLOYMENT.md)
- Multi-region Azure Container Apps with shared ACR, Key Vault, managed identities, and GitHub OIDC: [Azure guide](docs/AZURE.md)

The Azure layout builds the relay image once, then deploys it to every configured region. Shared resources live in `qiankui-shared`; regional resources use `qiankui-<region>` and `qiankui-infra-<region>` naming.

Relay options:

```text
--listen <host:port>   Listener; default 0.0.0.0:8443
--cert <pem>           TLS certificate
--key <pem>            TLS private key
--token <secret>       Bearer token; may come from QIANKUI_TOKEN
--ports <list>         Allowed destination ports; default 80,443
--allow-private        Permit private, loopback, and reserved destinations
--connect-timeout <ms> Outbound connection timeout; default 10000
```

## Security Boundaries

The relay always authenticates a token. By default it allows only destination ports 80 and 443 and rejects private, loopback, link-local, and reserved addresses. This prevents an accidentally exposed relay from becoming an unauthenticated open proxy or an internal-network probe.

Qiankui does not log destination hostnames. Operational logs cover startup, shutdown, and failures needed to run the service.

Run relays only on systems and networks you own or are explicitly authorized to use. Do not expose anonymous relays, and review the laws and provider terms that apply to your deployment.

## Build and Test

Qiankui requires Rust 1.97.0 for development:

```sh
make check
make test
make release
```

Release binaries are written to:

```text
target/release/qiankui
target/release/qiankui-relay
```

Build server binaries on the target operating system and architecture. A macOS binary cannot run on a Linux server.

## Names

The source tree uses terms drawn from classical Chinese literature:

- **Qiankui / 潜逵**: the project; from Guo Pu's *Rhapsody on the Yangtze*, “潜逵傍通”.
- **Shutu / 殊途** (`src/shutu`): replaceable transport paths.
- **Zhiyou / 置邮** (`src/zhiyou.rs`): the remote relay, named after ancient courier stations.
- **Fujie / 符节** (`src/fujie`): authentication credentials and egress policy.
- **Jing / 径** (`src/socks5.rs`): the local SOCKS5 entrance.

## Roadmap

1. Replace one-TLS-connection-per-stream transport with HTTP/2 `CONNECT` multiplexing.
2. Stabilize transport capabilities and error semantics.
3. Add short-lived credentials and automated certificate rotation.
4. Explore HTTP/3, MASQUE, and UDP without inventing custom cryptography.
5. Add TUN, graphical clients, and multi-node orchestration after the transport core is mature.

## License

Licensed under the [Apache License 2.0](LICENSE).
