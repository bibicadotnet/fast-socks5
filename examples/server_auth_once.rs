#![forbid(unsafe_code)]
#[macro_use]
extern crate log;

use anyhow::Context;
use fast_socks5::{
    server::{run_tcp_proxy, run_udp_proxy, DnsResolveHelper as _, Socks5ServerProtocol},
    ReplyError, Result, Socks5Command, SocksError,
};
use std::collections::HashSet;
use std::future::Future;
use std::net::IpAddr;
use std::sync::Arc;
use structopt::StructOpt;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio::task;

#[derive(Debug, StructOpt)]
#[structopt(name = "socks5-server", about = "A simple socks5-server implementation.")]
struct Opt {
    #[structopt(short, long)]
    pub listen_addr: String,

    #[structopt(long)]
    pub public_addr: Option<IpAddr>,

    #[structopt(short = "t", long, default_value = "10")]
    pub request_timeout: u64,

    #[structopt(subcommand, name = "auth")]
    pub auth: AuthMode,

    #[structopt(short = "k", long)]
    pub skip_auth: bool,

    #[structopt(short = "U", long)]
    pub allow_udp: bool,

    #[structopt(long)]
    pub auth_once: bool,
}

#[derive(StructOpt, Debug, PartialEq)]
enum AuthMode {
    NoAuth,
    Password {
        #[structopt(short, long)]
        username: String,
        #[structopt(short, long)]
        password: String,
    },
}

#[derive(Debug, Clone)]
struct AuthState {
    whitelisted_ips: Arc<RwLock<HashSet<IpAddr>>>,
}

impl AuthState {
    fn new() -> Self {
        Self {
            whitelisted_ips: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    async fn is_whitelisted(&self, ip: IpAddr) -> bool {
        self.whitelisted_ips.read().await.contains(&ip)
    }

    async fn add_ip(&self, ip: IpAddr) {
        let mut ips = self.whitelisted_ips.write().await;
        if ips.insert(ip) {
            info!("Whitelisted IP: {}", ip);
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let opt: &'static Opt = Box::leak(Box::new(Opt::from_args()));

    if opt.allow_udp && opt.public_addr.is_none() {
        return Err(SocksError::ArgumentInputError("UDP requires --public-addr"));
    }
    if opt.skip_auth && opt.auth != AuthMode::NoAuth {
        return Err(SocksError::ArgumentInputError("skip-auth cannot be used with auth mode"));
    }
    if opt.auth_once && opt.auth == AuthMode::NoAuth {
        return Err(SocksError::ArgumentInputError("auth-once requires password auth"));
    }

    let auth_state = AuthState::new();
    let listener = TcpListener::bind(&opt.listen_addr).await?;
    info!("Listening on {}", opt.listen_addr);

    loop {
        match listener.accept().await {
            Ok((socket, addr)) => {
                let ip = addr.ip();
                spawn_and_log_error(serve(opt, socket, ip, auth_state.clone()));
            }
            Err(e) => error!("accept failed: {e}"),
        }
    }
}

async fn serve(opt: &Opt, socket: tokio::net::TcpStream, ip: IpAddr, auth_state: AuthState) -> Result<()> {
    let (proto, cmd, target_addr) = match &opt.auth {
        AuthMode::NoAuth if opt.skip_auth => {
            Socks5ServerProtocol::skip_auth_this_is_not_rfc_compliant(socket)
        }
        AuthMode::NoAuth => Socks5ServerProtocol::accept_no_auth(socket).await?,
        AuthMode::Password { username, password } => {
            if opt.auth_once && auth_state.is_whitelisted(ip).await {
                let (proto, _) = Socks5ServerProtocol::accept_password_auth(socket, |_u, _p| true).await?;
                proto
            } else {
                let (proto, ok) = Socks5ServerProtocol::accept_password_auth(socket, |u, p| {
                    u == *username && p == *password
                }).await?;

                if ok && opt.auth_once {
                    auth_state.add_ip(ip).await;
                }

                proto
            }
        }
    }
    .read_command()
    .await?
    .resolve_dns()
    .await?;

    match cmd {
        Socks5Command::TCPConnect => {
            run_tcp_proxy(proto, &target_addr, opt.request_timeout, false).await?;
        }
        Socks5Command::UDPAssociate if opt.allow_udp => {
            let reply_ip = opt.public_addr.context("missing public-addr")?;
            run_udp_proxy(proto, &target_addr, None, reply_ip, None).await?;
        }
        _ => {
            proto.reply_error(&ReplyError::CommandNotSupported).await?;
            return Err(ReplyError::CommandNotSupported.into());
        }
    }

    Ok(())
}

fn spawn_and_log_error<F>(fut: F) -> task::JoinHandle<()> 
where
    F: Future<Output = Result<()>> + Send + 'static,
{
    task::spawn(async move {
        if let Err(e) = fut.await {
            error!("{:#}", e);
        }
    })
}
