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
    #[structopt(subcommand, name = "auth")]
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

    /// Time in seconds to cache whitelist status (0 = no expiry)
    #[structopt(long, default_value = "0")]
    pub whitelist_ttl: u64,
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

/// Whitelist entry with optional expiry
#[derive(Debug, Clone)]
struct WhitelistEntry {
    added_at: std::time::Instant,
    ttl_seconds: Option<u64>,
}

impl WhitelistEntry {
    fn new(ttl_seconds: Option<u64>) -> Self {
        Self {
            added_at: std::time::Instant::now(),
            ttl_seconds,
        }
    }

    fn is_expired(&self) -> bool {
        if let Some(ttl) = self.ttl_seconds {
            self.added_at.elapsed().as_secs() > ttl
        } else {
            false
        }
    }
}

/// Shared state for authenticated IPs
#[derive(Debug, Clone)]
struct AuthState {
    /// Map of IP addresses to their whitelist entries
    authenticated_ips: Arc<RwLock<std::collections::HashMap<IpAddr, WhitelistEntry>>>,
}

impl AuthState {
    fn new() -> Self {
        Self {
            authenticated_ips: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    /// Check if an IP is currently authenticated (and not expired)
    async fn is_authenticated(&self, ip: &IpAddr) -> bool {
        let mut ips = self.authenticated_ips.write().await;
        
        if let Some(entry) = ips.get(ip) {
            if entry.is_expired() {
                info!("IP {} whitelist entry expired, removing from cache", ip);
                ips.remove(ip);
                false
            } else {
                true
            }
        } else {
            false
        }
    }

    /// Add an IP to the authenticated list with optional TTL
    async fn add_authenticated_ip(&self, ip: IpAddr, ttl_seconds: Option<u64>) {
        let mut ips = self.authenticated_ips.write().await;
        let entry = WhitelistEntry::new(ttl_seconds);
        ips.insert(ip, entry);
        
        if let Some(ttl) = ttl_seconds {
            info!("IP {} added to whitelist (expires in {} seconds)", ip, ttl);
        } else {
            info!("IP {} added to permanent whitelist", ip);
        }
    }

    /// Get count of authenticated IPs (cleaning expired ones)
    async fn authenticated_count(&self) -> usize {
        let mut ips = self.authenticated_ips.write().await;
        
        // Clean expired entries
        let expired_ips: Vec<IpAddr> = ips
            .iter()
            .filter_map(|(ip, entry)| if entry.is_expired() { Some(*ip) } else { None })
            .collect();
            
        for ip in expired_ips {
            debug!("Cleaning expired whitelist entry for IP {}", ip);
            ips.remove(&ip);
        }
        
        ips.len()
    }

    /// Clean expired entries periodically
    async fn cleanup_expired(&self) {
        let mut ips = self.authenticated_ips.write().await;
        let before_count = ips.len();
        
        ips.retain(|ip, entry| {
            if entry.is_expired() {
                debug!("Removing expired whitelist entry for IP {}", ip);
                false
            } else {
                true
            }
        });
        
        let after_count = ips.len();
        if before_count != after_count {
            info!("Cleaned {} expired whitelist entries", before_count - after_count);
        }
    }
}

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
    let auth_state = AuthState::new();

    info!("Listen for socks connections @ {}", &opt.listen_addr);
    if opt.auth_once {
        if opt.whitelist_ttl > 0 {
            info!("One-time authentication enabled - IPs will be whitelisted for {} seconds", opt.whitelist_ttl);
        } else {
            info!("One-time authentication enabled - IPs will be permanently whitelisted");
        }
    }

    // Spawn cleanup task if TTL is enabled
    if opt.auth_once && opt.whitelist_ttl > 0 {
        let auth_state_cleanup = auth_state.clone();
        let cleanup_interval = std::cmp::max(opt.whitelist_ttl / 4, 30); // Clean every 1/4 of TTL or 30s minimum
        
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(cleanup_interval));
            loop {
                interval.tick().await;
                auth_state_cleanup.cleanup_expired().await;
            }
        });
        
        info!("Started whitelist cleanup task (interval: {} seconds)", cleanup_interval);
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
    
    // Pre-check whitelist status for auth_once mode
    let is_whitelisted = if opt.auth_once {
        auth_state.is_authenticated(&client_ip).await
    } else {
        false
    };

    // Choose authentication method based on whitelist status
    let (proto, cmd, target_addr) = if is_whitelisted {
        // IP is whitelisted - use NO AUTH for maximum performance
        debug!("IP {} is whitelisted, using no-auth method", client_ip);
        Socks5ServerProtocol::accept_no_auth(socket).await?
    } else {
        // IP not whitelisted - use configured auth method
        match &opt.auth {
            AuthMode::NoAuth if opt.skip_auth => {
                debug!("Using skip-auth method for {}", client_ip);
                Socks5ServerProtocol::skip_auth_this_is_not_rfc_compliant(socket)
            }
            AuthMode::NoAuth => {
                debug!("Using no-auth method for {}", client_ip);
                Socks5ServerProtocol::accept_no_auth(socket).await?
            }
            AuthMode::Password { username, password } => {
                debug!("Requiring password authentication for {}", client_ip);
                
                let start_time = std::time::Instant::now();
                let result = Socks5ServerProtocol::accept_password_auth(socket, |user, pass| {
                    let auth_success = user == *username && pass == *password;
                    if auth_success {
                        debug!("Authentication successful for user: {} from IP: {}", user, client_ip);
                    } else {
                        warn!("Authentication failed for user: {} from IP: {}", user, client_ip);
                    }
                    auth_success
                })
                .await?;
                
                let auth_duration = start_time.elapsed();
                debug!("Authentication completed in {:?} for {}", auth_duration, client_ip);
                
                // If auth was successful and auth_once is enabled, add IP to whitelist
                if opt.auth_once {
                    let ttl = if opt.whitelist_ttl > 0 { 
                        Some(opt.whitelist_ttl) 
                    } else { 
                        None 
                    };
                    auth_state.add_authenticated_ip(client_ip, ttl).await;
                    let count = auth_state.authenticated_count().await;
                    info!("IP {} authenticated and whitelisted. Total active whitelist entries: {}", client_ip, count);
                }
                
                result.0
            }
        }
    }
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
