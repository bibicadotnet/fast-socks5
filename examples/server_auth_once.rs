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
    about = "A SOCKS5 server with MicroSocks-style auth-once implementation."
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

// Custom authentication methods enum to match MicroSocks behavior
#[derive(Debug, PartialEq)]
enum AuthMethod {
    NoAuth = 0x00,
    UserPass = 0x02,
    Invalid = 0xFF,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    spawn_socks_server().await
}

async fn spawn_socks_server() -> Result<()> {
    let opt: &'static Opt = Box::leak(Box::new(Opt::from_args()));
    
    // Validation logic similar to original
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
                spawn_and_log_error(serve_socks5_microsocks_style(
                    opt, 
                    socket, 
                    client_addr.ip(), 
                    auth_state_clone
                ));
            }
            Err(err) => {
                error!("accept error = {:?}", err);
            }
        }
    }
}

// MicroSocks-style authentication check
async fn check_auth_method(
    opt: &Opt,
    client_ip: &IpAddr,
    auth_state: &AuthState,
    client_methods: &[u8],
) -> AuthMethod {
    let has_no_auth = client_methods.contains(&(AuthMethod::NoAuth as u8));
    let has_userpass = client_methods.contains(&(AuthMethod::UserPass as u8));
    
    match &opt.auth {
        AuthMode::NoAuth => {
            if has_no_auth {
                return AuthMethod::NoAuth;
            }
        }
        AuthMode::Password { .. } => {
            // MicroSocks logic: Check NO_AUTH first if auth-once is enabled
            if has_no_auth && opt.auth_once {
                // Check if IP is already authenticated
                if auth_state.is_authenticated(client_ip).await {
                    debug!("IP {} is whitelisted, allowing NO_AUTH", client_ip);
                    return AuthMethod::NoAuth;
                }
            }
            
            // If not whitelisted or NO_AUTH not supported, require UserPass
            if has_userpass {
                return AuthMethod::UserPass;
            }
        }
    }
    
    AuthMethod::Invalid
}

async fn serve_socks5_microsocks_style(
    opt: &Opt,
    socket: tokio::net::TcpStream,
    client_ip: IpAddr,
    auth_state: AuthState,
) -> Result<(), SocksError> {
    match &opt.auth {
        AuthMode::NoAuth if opt.skip_auth => {
            // Skip auth entirely - direct to command phase
            let proto = Socks5ServerProtocol::skip_auth_this_is_not_rfc_compliant(socket);
            handle_socks_command(opt, proto).await
        }
        AuthMode::NoAuth => {
            // Simple no-auth case
            let proto = Socks5ServerProtocol::accept_no_auth(socket).await?;
            handle_socks_command(opt, proto).await
        }
        AuthMode::Password { username, password } => {
            // MicroSocks-style flexible authentication
            serve_with_flexible_auth(opt, socket, client_ip, auth_state, username, password).await
        }
    }
}

async fn serve_with_flexible_auth(
    opt: &Opt,
    socket: tokio::net::TcpStream,
    client_ip: IpAddr,
    auth_state: AuthState,
    username: &str,
    password: &str,
) -> Result<(), SocksError> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    
    let mut socket = socket;
    
    // Step 1: Read client auth methods
    let mut buf = [0u8; 257];
    let n = socket.read(&mut buf).await.map_err(|_| SocksError::Io(std::io::Error::new(
        std::io::ErrorKind::UnexpectedEof, "Failed to read auth methods"
    )))?;
    
    if n < 2 || buf[0] != 5 {
        return Err(SocksError::InvalidVersion);
    }
    
    let n_methods = buf[1] as usize;
    if n < 2 + n_methods {
        return Err(SocksError::InvalidAuthNMethods);
    }
    
    let client_methods = &buf[2..2 + n_methods];
    
    // Step 2: Determine auth method using MicroSocks logic
    let chosen_method = check_auth_method(opt, &client_ip, &auth_state, client_methods).await;
    
    // Step 3: Send auth method response
    let response = [5u8, chosen_method as u8];
    socket.write_all(&response).await.map_err(|e| SocksError::Io(e))?;
    
    if chosen_method == AuthMethod::Invalid {
        return Err(SocksError::NoAcceptableAuthMethods);
    }
    
    // Step 4: Handle authentication based on chosen method
    match chosen_method {
        AuthMethod::NoAuth => {
            // IP is whitelisted, proceed directly to command phase
            debug!("IP {} using NO_AUTH (whitelisted)", client_ip);
            let proto = Socks5ServerProtocol::new(socket);
            handle_socks_command(opt, proto).await
        }
        AuthMethod::UserPass => {
            // Perform username/password authentication
            debug!("IP {} using USERNAME/PASSWORD auth", client_ip);
            
            // Read username/password
            let n = socket.read(&mut buf).await.map_err(|_| SocksError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof, "Failed to read credentials"
            )))?;
            
            if n < 3 || buf[0] != 1 {
                return Err(SocksError::InvalidSubnegotiationVersion);
            }
            
            let username_len = buf[1] as usize;
            if n < 2 + username_len + 1 {
                return Err(SocksError::InvalidAuthNMethods);
            }
            
            let password_len = buf[2 + username_len] as usize;
            if n < 2 + username_len + 1 + password_len {
                return Err(SocksError::InvalidAuthNMethods);
            }
            
            let provided_username = String::from_utf8_lossy(&buf[2..2 + username_len]);
            let provided_password = String::from_utf8_lossy(&buf[2 + username_len + 1..2 + username_len + 1 + password_len]);
            
            // Validate credentials
            let auth_success = provided_username == username && provided_password == password;
            
            // Send auth result
            let auth_response = [1u8, if auth_success { 0 } else { 1 }];
            socket.write_all(&auth_response).await.map_err(|e| SocksError::Io(e))?;
            
            if !auth_success {
                debug!("IP {} authentication failed", client_ip);
                return Err(SocksError::AuthenticationFailed);
            }
            
            debug!("IP {} authenticated successfully", client_ip);
            
            // Add to whitelist if auth-once is enabled
            if opt.auth_once {
                auth_state.add_authenticated_ip(client_ip).await;
            }
            
            // Proceed to command phase
            let proto = Socks5ServerProtocol::new(socket);
            handle_socks_command(opt, proto).await
        }
        AuthMethod::Invalid => {
            unreachable!("Should have been caught earlier");
        }
    }
}

async fn handle_socks_command(
    opt: &Opt,
    proto: Socks5ServerProtocol<tokio::net::TcpStream>,
) -> Result<(), SocksError> {
    let (proto, cmd, target_addr) = proto
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
