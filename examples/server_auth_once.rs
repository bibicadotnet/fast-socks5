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
/// Same as above but with UDP support
///     `$ RUST_LOG=debug cargo run --example server -- --listen-addr 127.0.0.1:1337 --allow-udp --public-addr 127.0.0.1 password --username admin --password password`
///
/// Listen with one-time authentication (IP whitelist after first auth):
///     `$ RUST_LOG=debug cargo run --example server -- --listen-addr 127.0.0.1:1337 --auth-once password --username admin --password password`
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

    /// Enable one-time authentication - IP whitelist after successful auth
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

/// Shared state for authenticated IPs
#[derive(Debug, Clone)]
struct AuthState {
    /// Set of IP addresses that have been successfully authenticated
    authenticated_ips: Arc<RwLock<HashSet<IpAddr>>>,
    /// Username for password authentication
    username: String,
    /// Password for password authentication
    password: String,
}

impl AuthState {
    fn new(username: String, password: String) -> Self {
        Self {
            authenticated_ips: Arc::new(RwLock::new(HashSet::new())),
            username,
            password,
        }
    }

    fn new_no_auth() -> Self {
        Self {
            authenticated_ips: Arc::new(RwLock::new(HashSet::new())),
            username: String::new(),
            password: String::new(),
        }
    }

    /// Check if an IP is already authenticated
    async fn is_authenticated(&self, ip: &IpAddr) -> bool {
        let ips = self.authenticated_ips.read().await;
        ips.contains(ip)
    }

    /// Add an IP to the authenticated list
    async fn add_authenticated_ip(&self, ip: IpAddr) {
        let mut ips = self.authenticated_ips.write().await;
        if ips.insert(ip) {
            info!("IP {} added to whitelist after successful authentication", ip);
        }
    }

    /// Get count of authenticated IPs (for logging)
    async fn authenticated_count(&self) -> usize {
        let ips = self.authenticated_ips.read().await;
        ips.len()
    }

    /// Validate credentials
    fn validate_credentials(&self, user: &str, pass: &str) -> bool {
        user == self.username && pass == self.password
    }
}

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
    
    // Validation checks
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
    
    // Initialize auth state based on auth mode
    let auth_state = match &opt.auth {
        AuthMode::NoAuth => AuthState::new_no_auth(),
        AuthMode::Password { username, password } => {
            AuthState::new(username.clone(), password.clone())
        }
    };

    info!("Listen for socks connections @ {}", &opt.listen_addr);
    if opt.auth_once {
        info!("One-time authentication enabled - IPs will be whitelisted after successful auth");
    }

    // Standard TCP loop
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
    auth_state: AuthState
) -> Result<(), SocksError> {
    
    // Determine authentication method based on auth_once status
    let is_whitelisted = if opt.auth_once {
        auth_state.is_authenticated(&client_ip).await
    } else {
        false
    };

    debug!("Client {} - Whitelisted: {}", client_ip, is_whitelisted);

    // Handle authentication based on current state and configuration
    let proto = match (&opt.auth, is_whitelisted) {
        // Case 1: No auth mode (regardless of whitelist status)
        (AuthMode::NoAuth, _) if opt.skip_auth => {
            debug!("Using skip-auth for {}", client_ip);
            Socks5ServerProtocol::skip_auth_this_is_not_rfc_compliant(socket)
        }
        (AuthMode::NoAuth, _) => {
            debug!("Using no-auth for {}", client_ip);
            Socks5ServerProtocol::accept_no_auth(socket).await?
        }
        
        // Case 2: Password auth with whitelisted IP (auth_once enabled)
        (AuthMode::Password { .. }, true) => {
            debug!("IP {} is whitelisted, using no-auth", client_ip);
            // For whitelisted IPs, we advertise and use no-auth
            Socks5ServerProtocol::accept_no_auth(socket).await?
        }
        
        // Case 3: Password auth with non-whitelisted IP or auth_once disabled
        (AuthMode::Password { .. }, false) => {
            debug!("IP {} requires password authentication", client_ip);
            
            // Clone auth_state for the closure
            let auth_state_for_closure = auth_state.clone();
            let client_ip_for_closure = client_ip;
            let auth_once = opt.auth_once;
            
            let (proto, _auth_result) = Socks5ServerProtocol::accept_password_auth(
                socket, 
                move |user: &str, pass: &str| -> bool {
                    let is_valid = auth_state_for_closure.validate_credentials(user, pass);
                    debug!("Authentication attempt for {} - Valid: {}", client_ip_for_closure, is_valid);
                    is_valid
                }
            ).await?;
            
            // If authentication was successful and auth_once is enabled, add IP to whitelist
            if auth_once {
                auth_state.add_authenticated_ip(client_ip).await;
                let count = auth_state.authenticated_count().await;
                info!("Authentication successful for {}. Total whitelisted IPs: {}", client_ip, count);
            }
            
            proto
        }
    };

    let (proto, cmd, target_addr) = proto
    .read_command()
    .await?
    .resolve_dns()
    .await?;

    match cmd {
        Socks5Command::TCPConnect => {
            debug!("Handling TCP connect for {} to {}", client_ip, target_addr);
            run_tcp_proxy(proto, &target_addr, opt.request_timeout, false).await?;
        }
        Socks5Command::UDPAssociate if opt.allow_udp => {
            debug!("Handling UDP associate for {} to {}", client_ip, target_addr);
            let reply_ip = opt.public_addr.context("invalid reply ip")?;
            run_udp_proxy(proto, &target_addr, None, reply_ip, None).await?;
        }
        _ => {
            warn!("Unsupported command from {}: {:?}", client_ip, cmd);
            proto.reply_error(&ReplyError::CommandNotSupported).await?;
            return Err(ReplyError::CommandNotSupported.into());
        }
    };
    
    debug!("Connection completed for {}", client_ip);
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
