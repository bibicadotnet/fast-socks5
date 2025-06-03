#[forbid(unsafe_code)]
#[macro_use]
extern crate log;

use anyhow::Context;
use fast_socks5::{
    server::{run_tcp_proxy, run_udp_proxy, DnsResolveHelper as _, Socks5ServerProtocol},
    ReplyError, Result, Socks5Command, SocksError,
};
use std::collections::HashSet;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use structopt::StructOpt;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio::task;

/// # How to use it:
///
/// Listen with auth-once mode:
///     `$ cargo run -- --listen-addr 127.0.0.1:1080 --username user --password pass --auth-once`
///
/// Normal password auth:
///     `$ cargo run -- --listen-addr 127.0.0.1:1080 --username user --password pass`
///
/// No authentication:
///     `$ cargo run -- --listen-addr 127.0.0.1:1080`
#[derive(Debug, StructOpt)]
#[structopt(
    name = "socks5-server",
    about = "A SOCKS5 server with auth-once capability."
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

    /// Username for authentication
    #[structopt(short, long)]
    pub username: Option<String>,

    /// Password for authentication
    #[structopt(short, long)]
    pub password: Option<String>,

    /// Enable auth-once mode (remember authenticated IPs)
    #[structopt(long)]
    pub auth_once: bool,

    /// Don't perform the auth handshake, send directly the command request
    #[structopt(short = "k", long)]
    pub skip_auth: bool,

    /// Allow UDP proxying, requires public-addr to be set
    #[structopt(short = "U", long)]
    pub allow_udp: bool,
}

#[derive(Debug, PartialEq)]
enum AuthMode {
    NoAuth,
    Password {
        username: String,
        password: String,
    },
    AuthOnce {
        username: String,
        password: String,
    },
}

impl Opt {
    fn get_auth_mode(&self) -> Result<AuthMode> {
        match (&self.username, &self.password, self.auth_once) {
            (Some(u), Some(p), true) => Ok(AuthMode::AuthOnce {
                username: u.clone(),
                password: p.clone(),
            }),
            (Some(u), Some(p), false) => Ok(AuthMode::Password {
                username: u.clone(),
                password: p.clone(),
            }),
            (None, None, false) => Ok(AuthMode::NoAuth),
            _ => Err(SocksError::ArgumentInputError(
                "Username and password must be provided together",
            )),
        }
    }
}

#[derive(Clone)]
struct AuthenticatedIPs {
    ips: Arc<RwLock<HashSet<IpAddr>>>,
}

impl AuthenticatedIPs {
    fn new() -> Self {
        AuthenticatedIPs {
            ips: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    async fn add(&self, ip: IpAddr) {
        let mut ips = self.ips.write().await;
        ips.insert(ip);
    }

    async fn contains(&self, ip: &IpAddr) -> bool {
        let ips = self.ips.read().await;
        ips.contains(ip)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let authenticated_ips = AuthenticatedIPs::new();
    spawn_socks_server(authenticated_ips).await
}

async fn spawn_socks_server(authenticated_ips: AuthenticatedIPs) -> Result<()> {
    let opt: &'static Opt = Box::leak(Box::new(Opt::from_args()));
    if opt.allow_udp && opt.public_addr.is_none() {
        return Err(SocksError::ArgumentInputError(
            "Can't allow UDP if public-addr is not set",
        ));
    }

    let auth_mode = opt.get_auth_mode()?;
    if opt.skip_auth && auth_mode != AuthMode::NoAuth {
        return Err(SocksError::ArgumentInputError(
            "Can't use skip-auth flag with authentication",
        ));
    }

    let listener = TcpListener::bind(&opt.listen_addr).await?;
    info!("Listening for SOCKS connections @ {}", &opt.listen_addr);

    loop {
        match listener.accept().await {
            Ok((socket, client_addr)) => {
                let ips = authenticated_ips.clone();
                spawn_and_log_error(serve_socks5(opt, auth_mode, socket, client_addr, ips));
            }
            Err(err) => error!("Accept error: {:?}", err),
        }
    }
}

async fn serve_socks5(
    opt: &Opt,
    auth_mode: AuthMode,
    socket: tokio::net::TcpStream,
    client_addr: SocketAddr,
    authenticated_ips: AuthenticatedIPs,
) -> Result<(), SocksError> {
    let client_ip = client_addr.ip();
    let is_pre_authenticated = authenticated_ips.contains(&client_ip).await;

    let proto = match (auth_mode, is_pre_authenticated) {
        (AuthMode::NoAuth, _) if opt.skip_auth => {
            Socks5ServerProtocol::skip_auth_this_is_not_rfc_compliant(socket)
        }
        (AuthMode::NoAuth, _) => Socks5ServerProtocol::accept_no_auth(socket).await?,
        (AuthMode::Password { username, password }, _) => {
            Socks5ServerProtocol::accept_password_auth(socket, |user, pass| {
                user == username && pass == password
            })
            .await?
            .0
        }
        (AuthMode::AuthOnce { username, password }, true) => {
            Socks5ServerProtocol::accept_no_auth(socket).await?
        }
        (AuthMode::AuthOnce { username, password }, false) => {
            let (proto, success) = Socks5ServerProtocol::accept_password_auth(
                socket,
                |user, pass| user == username && pass == password,
            )
            .await?;
            
            if success {
                authenticated_ips.add(client_ip).await;
                info!("Added {} to authenticated IPs", client_ip);
            }
            proto
        }
    };

    let (proto, cmd, target_addr) = proto.read_command().await?.resolve_dns().await?;

    match cmd {
        Socks5Command::TCPConnect => {
            run_tcp_proxy(proto, &target_addr, opt.request_timeout, false).await?;
        }
        Socks5Command::UDPAssociate if opt.allow_udp => {
            let reply_ip = opt.public_addr.context("Invalid reply IP")?;
            run_udp_proxy(proto, &target_addr, None, reply_ip, None).await?;
        }
        _ => {
            proto.reply_error(&ReplyError::CommandNotSupported).await?;
            return Err(ReplyError::CommandNotSupported.into());
        }
    };

    Ok(())
}

fn spawn_and_log_error<F>(fut: F) -> task::JoinHandle<()>
where
    F: Future<Output = Result<()>> + Send + 'static,
{
    task::spawn(async move {
        match fut.await {
            Ok(()) => {}
            Err(err) => error!("{:#}", &err),
        }
    })
}
