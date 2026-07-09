use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::ExitCode,
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, bail};
use clap::{Args as ClapArgs, Parser, Subcommand};
use qiankui::{
    fujie::Token,
    jiandu::{self, ClientConfig},
    runtime::shutdown_signal,
    shutu::HttpConnectTransport,
    socks5,
};
use tokio::net::TcpListener;

#[derive(Parser)]
#[command(name = "qiankui", version, about = "潜逵近端")]
struct Args {
    /// 简牍所在；亦可取自 QIANKUI_CONFIG
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    #[command(flatten)]
    proxy: ProxyOptions,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Clone, Default, ClapArgs)]
struct ProxyOptions {
    /// SOCKS5 所守地址
    #[arg(long, global = true)]
    listen: Option<String>,

    /// 置邮 HTTPS 地址
    #[arg(long, global = true)]
    relay: Option<String>,

    /// 符节；宜用简牍或 QIANKUI_TOKEN，免现于命令史
    #[arg(long, global = true)]
    token: Option<String>,

    /// 自署 CA 或证书 PEM
    #[arg(long, global = true)]
    ca: Option<PathBuf>,

    /// 不验证 TLS 证书，仅供本地试验
    #[arg(long, global = true)]
    insecure: bool,

    /// 连接限时，单位毫秒
    #[arg(long, global = true)]
    connect_timeout: Option<u64>,
}

#[derive(Subcommand)]
enum Command {
    /// 依简牍起本地 SOCKS5；无子命令时亦行此令
    Run,
    /// 治本地简牍
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

#[derive(Subcommand)]
enum ConfigCommand {
    /// 新立简牍；缺符节时隐字问之
    Init {
        /// 从标准输入读取符节
        #[arg(long)]
        token_stdin: bool,

        /// 覆写既有简牍
        #[arg(long)]
        force: bool,
    },
    /// 示简牍而隐其符节
    Show,
    /// 示简牍所在
    Path,
}

#[tokio::main]
async fn main() -> ExitCode {
    match dispatch(Args::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("潜逵未起：{error:#}");
            ExitCode::FAILURE
        }
    }
}

async fn dispatch(args: Args) -> anyhow::Result<()> {
    let config_path = args.config.map(Ok).unwrap_or_else(jiandu::default_path)?;
    match args.command {
        Some(Command::Config { command }) => handle_config(command, &config_path, args.proxy),
        Some(Command::Run) | None => run_proxy(&config_path, args.proxy).await,
    }
}

fn handle_config(
    command: ConfigCommand,
    config_path: &Path,
    options: ProxyOptions,
) -> anyhow::Result<()> {
    match command {
        ConfigCommand::Init { token_stdin, force } => {
            let config = new_config(options, token_stdin)?;
            validate_transport(&config)?;
            config.save(config_path, force)?;
            println!("简牍已书 {}", config_path.display());
        }
        ConfigCommand::Show => {
            let config = ClientConfig::load(config_path)?;
            println!("简牍：{}", config_path.display());
            println!("所守：{}", config.listen);
            println!("置邮：{}", config.relay);
            println!("符节：[已藏]");
            match config.ca {
                Some(path) => println!("信证：{}", path.display()),
                None => println!("信证：系统根证书"),
            }
            println!("限时：{} 毫秒", config.connect_timeout_ms);
            if config.insecure {
                println!("验信：已关闭（仅宜试验）");
            } else {
                println!("验信：已开启");
            }
        }
        ConfigCommand::Path => println!("{}", config_path.display()),
    }
    Ok(())
}

fn new_config(options: ProxyOptions, token_stdin: bool) -> anyhow::Result<ClientConfig> {
    let relay = options
        .relay
        .context("relay is required; pass --relay when initializing config")?;
    let token = read_token(options.token, token_stdin)?;
    let ca = options
        .ca
        .map(|path| {
            fs::canonicalize(&path)
                .with_context(|| format!("could not resolve CA file {}", path.display()))
        })
        .transpose()?;

    Ok(ClientConfig {
        relay,
        token,
        listen: options
            .listen
            .unwrap_or_else(|| "127.0.0.1:1080".to_owned()),
        ca,
        insecure: options.insecure,
        connect_timeout_ms: options.connect_timeout.unwrap_or(10_000),
    })
}

fn read_token(argument: Option<String>, token_stdin: bool) -> anyhow::Result<String> {
    if argument.is_some() && token_stdin {
        bail!("--token and --token-stdin cannot be used together");
    }
    if token_stdin {
        let mut value = String::new();
        io::stdin()
            .read_to_string(&mut value)
            .context("could not read token from standard input")?;
        return Ok(value.trim_end_matches(['\r', '\n']).to_owned());
    }
    if let Some(value) = argument.or_else(|| std::env::var("QIANKUI_TOKEN").ok()) {
        return Ok(value);
    }
    rpassword::prompt_password("符节（输入不显）：").context("could not read token")
}

async fn run_proxy(config_path: &Path, options: ProxyOptions) -> anyhow::Result<()> {
    let config = resolve_config(config_path, options)?;
    if config.insecure {
        eprintln!("警：今未验置邮证书；此法惟宜本地试验。");
    }

    let transport = Arc::new(build_transport(&config)?);
    let listener = TcpListener::bind(&config.listen)
        .await
        .with_context(|| format!("could not listen on {}", config.listen))?;
    println!("潜逵已守 {}；置邮 {}", listener.local_addr()?, config.relay);

    tokio::select! {
        result = socks5::serve(listener, transport) => result?,
        _ = shutdown_signal() => println!("潜逵奉止。"),
    }
    Ok(())
}

fn resolve_config(path: &Path, options: ProxyOptions) -> anyhow::Result<ClientConfig> {
    let stored = path
        .exists()
        .then(|| ClientConfig::load(path))
        .transpose()?;
    let relay = options
        .relay
        .or_else(|| stored.as_ref().map(|config| config.relay.clone()))
        .with_context(|| {
            format!(
                "relay is required; run `qiankui config init --relay <url>` or pass --relay (looked for {})",
                path.display()
            )
        })?;
    let token = options
        .token
        .or_else(|| std::env::var("QIANKUI_TOKEN").ok())
        .or_else(|| stored.as_ref().map(|config| config.token.clone()))
        .context("token is required; initialize config, pass --token, or set QIANKUI_TOKEN")?;
    let config = ClientConfig {
        relay,
        token,
        listen: options
            .listen
            .or_else(|| stored.as_ref().map(|config| config.listen.clone()))
            .unwrap_or_else(|| "127.0.0.1:1080".to_owned()),
        ca: options
            .ca
            .or_else(|| stored.as_ref().and_then(|config| config.ca.clone())),
        insecure: options.insecure || stored.as_ref().is_some_and(|config| config.insecure),
        connect_timeout_ms: options
            .connect_timeout
            .or_else(|| stored.as_ref().map(|config| config.connect_timeout_ms))
            .unwrap_or(10_000),
    };
    config.validate()?;
    Ok(config)
}

fn validate_transport(config: &ClientConfig) -> anyhow::Result<()> {
    config.validate()?;
    build_transport(config).map(|_| ())
}

fn build_transport(config: &ClientConfig) -> anyhow::Result<HttpConnectTransport> {
    HttpConnectTransport::new(
        &config.relay,
        Token::parse(config.token.clone())?,
        config.ca.as_deref(),
        config.insecure,
        Duration::from_millis(config.connect_timeout_ms),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored_config(ca: PathBuf) -> ClientConfig {
        ClientConfig {
            relay: "https://stored.example:8443".to_owned(),
            token: "a-stored-token-with-enough-bytes".to_owned(),
            listen: "127.0.0.1:1080".to_owned(),
            ca: Some(ca),
            insecure: false,
            connect_timeout_ms: 10_000,
        }
    }

    #[test]
    fn command_line_overrides_stored_config() -> anyhow::Result<()> {
        let temporary = tempfile::tempdir()?;
        let ca = temporary.path().join("ca.pem");
        fs::write(&ca, "not parsed until transport construction")?;
        let path = temporary.path().join("config.toml");
        stored_config(ca).save(&path, false)?;

        let resolved = resolve_config(
            &path,
            ProxyOptions {
                relay: Some("https://override.example:9443".to_owned()),
                token: Some("an-override-token-with-enough-bytes".to_owned()),
                listen: Some("127.0.0.1:2080".to_owned()),
                connect_timeout: Some(2_000),
                ..ProxyOptions::default()
            },
        )?;
        assert_eq!(resolved.relay, "https://override.example:9443");
        assert_eq!(resolved.listen, "127.0.0.1:2080");
        assert_eq!(resolved.connect_timeout_ms, 2_000);
        assert_eq!(resolved.token, "an-override-token-with-enough-bytes");
        Ok(())
    }

    #[test]
    fn missing_config_explains_how_to_initialize() {
        let error = resolve_config(Path::new("/does/not/exist"), ProxyOptions::default())
            .expect_err("missing relay must fail");
        assert!(error.to_string().contains("config init"));
    }
}
