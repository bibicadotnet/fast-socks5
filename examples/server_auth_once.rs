#![forbid(unsafe_code)]

use anyhow::Context;
use fast_socks5::{
    server::{run_tcp_proxy, run_udp_proxy, DnsResolveHelper as _, Socks5ServerProtocol},
    ReplyError, Result, Socks5Command, SocksError,
};
use std::collections::HashSet;
use std::net::IpAddr;
use std::sync::Arc;
use structopt::StructOpt;
use tokio::net::TcpListener;
use tokio::sync::RwLock;

#[derive(Debug, StructOpt, Clone)]
#[structopt(
    name = "socks5-server",
    about = "A simple implementation of a socks5-server."
)]
struct Opt {
    /// Bind address, e.g. 127.0.0.1:1080
    #[structopt(short, long)]
    listen_addr: String,

    /// Our external IP address (required for UDP)
    #[structopt(long)]
    public_addr: Option<IpAddr>,

    /// Request timeout (seconds)
    #[structopt(short = "t", long, default_value = "10")]
    request_timeout: u64,

    /// Authentication method
    #[structopt(subcommand)]
    auth: AuthMode,

    /// Skip authentication handshake
    #[structopt(short = "k", long)]
    skip_auth: bool,

    /// Allow UDP proxying (requires public_addr)
    #[structopt(short = "U", long)]
    allow_udp: bool,

    /// One-time authentication; whitelist IP after auth
    #[structopt(long)]
    auth_once: bool,
}

#[derive(Debug, StructOpt, PartialEq, Clone)]
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
        let ips = self.whitelisted_ips.read().await;
        ips.contains(&ip)
    }

    async fn add_ip(&self, ip: IpAddr) {
        let mut ips = self.whitelisted_ips.write().await;
        ips.insert(ip);
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let opt = Opt::from_args();

    if opt.allow_udp && opt.public_addr.is_none() {
        anyhow::bail!("Can't allow UDP if public_addr is not set");
    }
    if opt.skip_auth && opt.auth != AuthMode::NoAuth {
        anyhow::bail!("Can't use skip_auth flag and authentication together");
    }
    if opt.auth_once && opt.auth == AuthMode::NoAuth {
        anyhow::bail!("Can't use auth_once with no_auth mode");
    }

    let auth_state = AuthState::new();
    let listener = TcpListener::bind(&opt.listen_addr).await?;

    println!("Listening for SOCKS5 connections on {}", &opt.listen_addr);
    if opt.auth_once {
        println!("One-time authentication enabled; IPs will be whitelisted after successful auth");
    }

    loop {
        match listener.accept().await {
            Ok((socket, client_addr)) => {
                let client_ip = client_addr.ip();
                let opt_clone = opt.clone();
                let auth_clone = auth_state.clone();

                // Spawn a task to serve this client
                tokio::spawn(async move {
                    if let Err(e) = serve_socks5(&opt_clone, socket, client_ip, auth_clone).await {
                        eprintln!("Error serving client {}: {:?}", client_ip, e);
                    }
                });
            }
            Err(e) => {
                eprintln!("Failed to accept connection: {:?}", e);
            }
        }
    }
}

async fn serve_socks5(
    opt: &Opt,
    socket: tokio::net::TcpStream,
    client_ip: IpAddr,
    auth_state: AuthState,
) -> Result<(), SocksError> {
    let proto = if opt.auth_once && auth_state.is_whitelisted(client_ip).await {
        Socks5ServerProtocol::skip_auth_this_is_not_rfc_compliant(socket).await
    } else {
        match &opt.auth {
            AuthMode::NoAuth if opt.skip_auth => {
                Socks5ServerProtocol::skip_auth_this_is_not_rfc_compliant(socket).await
            }
            AuthMode::NoAuth => Socks5ServerProtocol::accept_no_auth(socket).await?,
            AuthMode::Password { username, password } => {
                let (proto, auth_success) =
                    Socks5ServerProtocol::accept_password_auth(socket, |user, pass| {
                        user == *username && pass == *password
                    })
                    .await?;

                if opt.auth_once && auth_success {
                    auth_state.add_ip(client_ip).await;
                }

                proto
            }
        }
    };

    let (proto, cmd, target_addr) = proto.read_command().await?.resolve_dns().await?;

    match cmd {
        Socks5Command::TcpConnect => {
            run_tcp_proxy(proto, &target_addr, opt.request_timeout, false).await?;
        }
        Socks5Command::UdpAssociate if opt.allow_udp => {
            let reply_ip = opt.public_addr.context("invalid reply ip")?;
            run_udp_proxy(proto, &target_addr, None, reply_ip, None).await?;
        }
        _ => {
            proto.reply_error(&ReplyError::CommandNotSupported).await?;
            return Err(ReplyError::CommandNotSupported.into());
        }
    }

    Ok(())
}
