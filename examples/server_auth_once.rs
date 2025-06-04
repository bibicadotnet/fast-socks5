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
use std::net::IpAddr;
use std::sync::Arc;
use structopt::StructOpt;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio::task;

/// # How to use it:
///
/// Listen on a local address, authentication-free:
///     `$ RUST_LOG=debug cargo run --example server -- --listen-addr 127.0.0.1:1337 no-auth`
///
/// Listen on a local address, with basic username/password requirement:
///     `$ RUST_LOG=debug cargo run --example server -- --listen-addr 127.0.0.1:1337 password --username admin --password password`
///
/// Listen with one-time authentication (IP whitelist after first auth):
///     `$ RUST_LOG=debug cargo run --example server -- --listen-addr 127.0.0.1:1337 --auth-once password --username admin --password password`
///
/// Same as above but with UDP support
///     `$ RUST_LOG=debug cargo run --example server -- --listen-addr 127.0.0.1:1337 --allow-udp --public-addr 127.0.0.1 password --username admin --password password`
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

    /// One-time authentication: once an IP authed successfully, it's whitelisted for no-auth
    #[structopt(long)]
    pub auth_once: bool,
}

/// Choose the authentication type
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
    authenticated_ips: Arc<RwLock<HashSet<IpAddr>>>,
}

impl AuthState {
    fn new() -> Self {
        Self {
            authenticated_ips: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    async fn is_authenticated(&self, ip: &IpAddr) -> bool {
        let auth_ips = self.authenticated_ips.read().await;
        auth_ips.contains(ip)
    }

    async fn add_authenticated_ip(&self, ip: IpAddr) {
        let mut auth_ips = self.authenticated_ips.write().await;
        auth_ips.insert(ip);
        debug!("Added IP {} to authenticated list", ip);
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

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
            "Can't use auth-once with no-auth mode. Use password authentication with auth-once.",
        ));
    }
    if opt.auth_once && opt.skip_auth {
        return Err(SocksError::ArgumentInputError(
            "Can't use auth-once with skip-auth flag.",
        ));
    }

    let listener = TcpListener::bind(&opt.listen_addr).await?;
    let auth_state = AuthState::new();

    info!("Listen for socks connections @ {}", &opt.listen_addr);
    if opt.auth_once {
        info!("Auth-once mode enabled: IPs will be whitelisted after first successful auth");
    }

    loop {
        match listener.accept().await {
            Ok((socket, client_addr)) => {
                let auth_state_clone = auth_state.clone();
                spawn_and_log_error(serve_socks5(opt, socket, client_addr.ip(), auth_state_clone));
            }
            Err(err) => {
                error!("accept error = {:?}", err);
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
    let is_whitelisted = opt.auth_once && auth_state.is_authenticated(&client_ip).await;

    let (proto, cmd, target_addr) = match &opt.auth {
        AuthMode::NoAuth if opt.skip_auth => {
            Socks5ServerProtocol::skip_auth_this_is_not_rfc_compliant(socket)
        }
        AuthMode::NoAuth => Socks5ServerProtocol::accept_no_auth(socket).await?,
        AuthMode::Password { username, password } => {
            let username_clone = username.clone();
            let password_clone = password.clone();

            let (proto, auth_success) = Socks5ServerProtocol::accept_password_auth(
                socket,
                move |user, pass| {
                    if is_whitelisted {
                        debug!("IP {} is whitelisted, skipping auth", client_ip);
                        true
                    } else {
                        let valid = user == username_clone && pass == password_clone;
                        if valid {
                            debug!("IP {} authenticated successfully", client_ip);
                        } else {
                            debug!("IP {} authentication failed", client_ip);
                        }
                        valid
                    }
                },
            )
            .await?;

            if auth_success && opt.auth_once && !is_whitelisted {
                auth_state.add_authenticated_ip(client_ip).await;
            }

            proto
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
            let reply_ip = opt.public_addr.context("invalid reply ip")?;
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
