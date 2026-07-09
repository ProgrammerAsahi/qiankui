use std::{path::PathBuf, process::ExitCode, sync::Arc, time::Duration};

use anyhow::{Context, bail};
use clap::Parser;
use qiankui::{fujie::Token, runtime::shutdown_signal, shutu::HttpConnectTransport, socks5};
use tokio::net::TcpListener;

#[derive(Debug, Parser)]
#[command(name = "qiankui", version, about = "潜逵近端")]
struct Args {
    /// SOCKS5 所守地址
    #[arg(long, default_value = "127.0.0.1:1080")]
    listen: String,

    /// 置邮 HTTPS 地址
    #[arg(long)]
    relay: String,

    /// 符节；亦可取自 QIANKUI_TOKEN
    #[arg(long)]
    token: Option<String>,

    /// 自署 CA 或证书 PEM
    #[arg(long)]
    ca: Option<PathBuf>,

    /// 不验证 TLS 证书，仅供本地试验
    #[arg(long)]
    insecure: bool,

    /// 连接限时，单位毫秒
    #[arg(long, default_value_t = 10_000)]
    connect_timeout: u64,
}

#[tokio::main]
async fn main() -> ExitCode {
    match run(Args::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("潜逵未起：{error:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run(args: Args) -> anyhow::Result<()> {
    if !(1..=300_000).contains(&args.connect_timeout) {
        bail!("connect-timeout must be between 1 and 300000 milliseconds");
    }
    let token = Token::from_arg_or_env(args.token)?;
    if args.insecure {
        eprintln!("警：今未验置邮证书；此法惟宜本地试验。");
    }

    let transport = Arc::new(HttpConnectTransport::new(
        &args.relay,
        token,
        args.ca.as_deref(),
        args.insecure,
        Duration::from_millis(args.connect_timeout),
    )?);
    let listener = TcpListener::bind(&args.listen)
        .await
        .with_context(|| format!("could not listen on {}", args.listen))?;
    println!("潜逵已守 {}", listener.local_addr()?);

    tokio::select! {
        result = socks5::serve(listener, transport) => result?,
        _ = shutdown_signal() => println!("潜逵奉止。"),
    }
    Ok(())
}
