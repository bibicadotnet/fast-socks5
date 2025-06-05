#![forbid(unsafe_code)]

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
#[structopt(
    name = "socks5-server",
    about = "A simple implementation of a socks5-server."
)]
struct Opt {
    /// Bind on address address. eg. `127.0.0.1:1080`
    #[structopt(short, long)]
    pub listen_addr: String,

    /// Our external IP address to be sent in reply packets (required for UDP)
    #[structopt(long)]
    pub public_addr: Option<std::net::IpAddr>,

    /// Request timeout
    #[structopt(short = "t", long, default_value = "10")]
    pub request_timeout: u64,

    /// Choose authentication type
    #[structopt(subcommand, name = "auth")]
    pub auth: AuthMode,

    /// Don't perform the auth handshake, send directly the command request
    #[structopt(short = "k", long)]
    pub skip_auth: bool,

    /// Allow UDP proxying, requires public-addr to be set
    #[structopt(short = "U", long)]
    pub allow_udp: bool,

    /// Enable one-time authentication - IP addresses are whitelisted after successful auth
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
    spawn_socks_server().await
}

async fn spawn_socks_server() -> Result<()> {
    let opt: &'static Opt = Box::leak(Box::new(Opt::from_args()));

    if opt.allow_udp && opt.public_addr.is_none() {
        return Err(SocksError::ArgumentInputError(
            "Can't allow UDP if public-addr is not set",
        ));
    }
    if opt.skip_auth && opt.auth != AuthMode::NoAuth {
        return Err(SocksError::ArgumentInputError(
            "Can't use skip-auth flag and authentication altogether.",
        ));
    }
    if opt.auth_once && opt.auth == AuthMode::NoAuth {
        return Err(SocksError::ArgumentInputError(
            "Can't use auth-once with no-auth mode.",
        ));
    }

    let auth_state = AuthState::new();
    let listener = TcpListener::bind(&opt.listen_addr).await?;

    loop {
        match listener.accept().await {
            Ok((socket, client_addr)) => {
                let client_ip = client_addr.ip();
                spawn_and_log_error(serve_socks5(opt, socket, client_ip, auth_state.clone()));
            }
            Err(_) => {
                // silently ignore accept errors
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
        Socks5ServerProtocol::skip_auth_this_is_not_rfc_compliant(socket)
    } else {
        match &opt.auth {
            AuthMode::NoAuth if opt.skip_auth => {
                Socks5ServerProtocol::skip_auth_this_is_not_rfc_compliant(socket)
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

fn spawn_and_log_error<F>(fut: F) -> task::JoinHandle<()>
where
    F: Future<Output = Result<()>> + Send + 'static,
{
    task::spawn(async move {
        let _ = fut.await;
    })
}
