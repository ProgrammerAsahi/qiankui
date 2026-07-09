use std::{path::PathBuf, process::ExitCode, sync::Arc, time::Duration};

use anyhow::{Context, bail};
use clap::Parser;
use qiankui::{
    fujie::{Policy, Token, parse_allowed_ports},
    runtime::shutdown_signal,
    zhiyou::{self, RelayConfig},
};
use tokio::net::TcpListener;

#[derive(Debug, Parser)]
#[command(name = "qiankui-relay", version, about = "潜逵置邮")]
struct Args {
    /// TLS 所守地址
    #[arg(long, default_value = "0.0.0.0:8443")]
    listen: String,

    /// TLS 证书 PEM
    #[arg(long)]
    cert: PathBuf,

    /// TLS 私钥 PEM
    #[arg(long)]
    key: PathBuf,

    /// 符节；亦可取自 QIANKUI_TOKEN
    #[arg(long)]
    token: Option<String>,

    /// 所许目的端口
    #[arg(long, default_value = "80,443")]
    ports: String,

    /// 允许私网、回环及保留地址
    #[arg(long)]
    allow_private: bool,

    /// 出口连接限时，单位毫秒
    #[arg(long, default_value_t = 10_000)]
    connect_timeout: u64,
}

#[tokio::main]
async fn main() -> ExitCode {
    match run(Args::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("置邮未起：{error:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run(args: Args) -> anyhow::Result<()> {
    if !(1..=300_000).contains(&args.connect_timeout) {
        bail!("connect-timeout must be between 1 and 300000 milliseconds");
    }
    let token = Token::from_arg_or_env(args.token)?;
    let allowed_ports = parse_allowed_ports(&args.ports)?;
    let policy = Policy::new(
        allowed_ports.clone(),
        args.allow_private,
        Duration::from_millis(args.connect_timeout),
    )?;
    let config = Arc::new(RelayConfig::new(&args.cert, &args.key, token, policy)?);
    let listener = TcpListener::bind(&args.listen)
        .await
        .with_context(|| format!("could not listen on {}", args.listen))?;

    println!("置邮已守 {}", listener.local_addr()?);
    let mut ports: Vec<_> = allowed_ports.into_iter().collect();
    ports.sort_unstable();
    println!(
        "所许目的端口：{}",
        ports
            .iter()
            .map(u16::to_string)
            .collect::<Vec<_>>()
            .join(",")
    );
    if args.allow_private {
        eprintln!("警：今许私网与保留地址；惟宜受控试验。");
    }

    tokio::select! {
        result = zhiyou::serve(listener, config) => result?,
        _ = shutdown_signal() => println!("置邮奉止。"),
    }
    Ok(())
}
