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
    #[structopt(subcommand, name = "auth")] // Note that we mark a field as a subcommand
    pub auth: AuthMode,

    /// Don't perform the auth handshake, send directly the command request
    #[structopt(short = "k", long)]
    pub skip_auth: bool,

    /// Allow UDP proxying, requires public-addr to be set
    #[structopt(short = "U", long)]
    pub allow_udp: bool,

    /// Enable one-time authentication (IP whitelist after first successful auth)
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

// Shared state for authenticated IPs
type AuthenticatedIPs = Arc<RwLock<HashSet<IpAddr>>>;

/// Useful read 1. https://blog.yoshuawuyts.com/rust-streams/
/// Useful read 2. https://blog.yoshuawuyts.com/futures-concurrency/
/// Useful read 3. https://blog.yoshuawuyts.com/streams-concurrency/
/// error-libs benchmark: https://blog.yoshuawuyts.com/error-handling-survey/
///
/// TODO: Write functional tests: https://github.com/ark0f/async-socks5/blob/master/src/lib.rs#L762
/// TODO: Write functional tests with cURL?
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
            "Can't use auth-once flag without authentication.",
        ));
    }

    let listener = TcpListener::bind(&opt.listen_addr).await?;
    let authenticated_ips: AuthenticatedIPs = Arc::new(RwLock::new(HashSet::new()));

    info!("Listen for socks connections @ {}", &opt.listen_addr);
    if opt.auth_once {
        info!("One-time authentication enabled - IPs will be whitelisted after first successful auth");
    }

    // Standard TCP loop
    loop {
        match listener.accept().await {
            Ok((socket, client_addr)) => {
                let authenticated_ips = authenticated_ips.clone();
                spawn_and_log_error(serve_socks5(opt, socket, client_addr.ip(), authenticated_ips));
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
    authenticated_ips: AuthenticatedIPs,
) -> Result<(), SocksError> {
    let (proto, cmd, target_addr) = match &opt.auth {
        AuthMode::NoAuth if opt.skip_auth => {
            Socks5ServerProtocol::skip_auth_this_is_not_rfc_compliant(socket)
        }
        AuthMode::NoAuth => Socks5ServerProtocol::accept_no_auth(socket).await?,
        AuthMode::Password { username, password } => {
            // Check if auth_once is enabled and IP is already authenticated
            if opt.auth_once {
                let is_authenticated = {
                    let ips = authenticated_ips.read().await;
                    ips.contains(&client_ip)
                };
                
                if is_authenticated {
                    info!("IP {} already authenticated, skipping auth", client_ip);
                    // Use custom auth method that allows both no-auth and password
                    Socks5ServerProtocol::accept_auth_once(socket, client_ip, authenticated_ips.clone()).await?
                } else {
                    info!("IP {} not authenticated, requiring password auth", client_ip);
                    let (proto, _) = Socks5ServerProtocol::accept_password_auth(socket, |user, pass| {
                        user == *username && pass == *password
                    })
                    .await?;
                    
                    // Add IP to authenticated list after successful auth
                    {
                        let mut ips = authenticated_ips.write().await;
                        ips.insert(client_ip);
                        info!("IP {} authenticated and added to whitelist", client_ip);
                    }
                    
                    proto
                }
            } else {
                Socks5ServerProtocol::accept_password_auth(socket, |user, pass| {
                    user == *username && pass == *password
                })
                .await?
                .0
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

// Extension trait for Socks5ServerProtocol to handle auth_once
trait Socks5ServerProtocolExt {
    async fn accept_auth_once(
        socket: tokio::net::TcpStream,
        client_ip: IpAddr,
        authenticated_ips: AuthenticatedIPs,
    ) -> Result<Socks5ServerProtocol, SocksError>;
}

impl Socks5ServerProtocolExt for Socks5ServerProtocol {
    async fn accept_auth_once(
        socket: tokio::net::TcpStream,
        client_ip: IpAddr,
        authenticated_ips: AuthenticatedIPs,
    ) -> Result<Socks5ServerProtocol, SocksError> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        
        let mut socket = socket;
        
        // Read client's authentication methods
        let mut buf = [0u8; 2];
        socket.read_exact(&mut buf).await?;
        
        let version = buf[0];
        let nmethods = buf[1] as usize;
        
        if version != 5 {
            return Err(SocksError::UnsupportedSocksVersion);
        }
        
        let mut methods = vec![0u8; nmethods];
        socket.read_exact(&mut methods).await?;
        
        // Check if IP is in whitelist
        let is_authenticated = {
            let ips = authenticated_ips.read().await;
            ips.contains(&client_ip)
        };
        
        // Apply pseudo-code logic for method selection
        let selected_method = if is_authenticated {
            // IP is whitelisted, prefer no-auth if available
            if methods.contains(&0x00) {
                0x00 // NO_AUTH
            } else if methods.contains(&0x02) {
                0x02 // USERNAME_PASSWORD
            } else {
                0xFF // NO_ACCEPTABLE_METHODS
            }
        } else {
            // IP not whitelisted, require authentication
            if methods.contains(&0x02) {
                0x02 // USERNAME_PASSWORD
            } else {
                0xFF // NO_ACCEPTABLE_METHODS
            }
        };
        
        // Send selected method
        socket.write_all(&[5, selected_method]).await?;
        
        match selected_method {
            0x00 => {
                // No authentication required
                Ok(Socks5ServerProtocol::new(socket))
            }
            0x02 => {
                // Username/password authentication required
                // This should be handled by the caller with proper credentials
                Err(SocksError::AuthMethodUnacceptable)
            }
            _ => Err(SocksError::AuthMethodUnacceptable),
        }
    }
}
