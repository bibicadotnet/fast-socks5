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
use tokio::io::{AsyncReadExt, AsyncWriteExt};
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

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    spawn_socks_server().await
}

async fn spawn_socks_server() -> Result<()> {
    let opt: &'static Opt = Box::leak(Box::new(Opt::from_args()));
    
    // Validation
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

    let listener = TcpListener::bind(&opt.listen_addr).await?;
    let whitelist: Arc<RwLock<HashSet<IpAddr>>> = Arc::new(RwLock::new(HashSet::new()));

    info!("Listen for socks connections @ {}", &opt.listen_addr);
    if opt.auth_once {
        info!("One-time authentication enabled");
    }

    // Standard TCP loop
    loop {
        match listener.accept().await {
            Ok((socket, client_addr)) => {
                let whitelist = whitelist.clone();
                spawn_and_log_error(serve_socks5(opt, socket, client_addr.ip(), whitelist));
            }
            Err(err) => {
                error!("accept error = {:?}", err);
            }
        }
    }
}

async fn serve_socks5(
    opt: &Opt, 
    mut socket: tokio::net::TcpStream, 
    client_ip: IpAddr,
    whitelist: Arc<RwLock<HashSet<IpAddr>>>
) -> Result<(), SocksError> {
    
    let (proto, cmd, target_addr) = match &opt.auth {
        AuthMode::NoAuth if opt.skip_auth => {
            Socks5ServerProtocol::skip_auth_this_is_not_rfc_compliant(socket)
        }
        AuthMode::NoAuth => Socks5ServerProtocol::accept_no_auth(socket).await?,
        AuthMode::Password { username, password } => {
            // Custom method negotiation for auth_once support
            if opt.auth_once {
                let selected_method = negotiate_auth_method(&mut socket, client_ip, &whitelist).await?;
                
                match selected_method {
                    0 => {
                        // NO_AUTH selected (whitelisted IP)
                        debug!("IP {} whitelisted, using NO_AUTH", client_ip);
                        Socks5ServerProtocol::accept_no_auth(socket).await?
                    },
                    2 => {
                        // PASSWORD auth selected
                        debug!("IP {} requires PASSWORD auth", client_ip);
                        let (proto, ..) = Socks5ServerProtocol::accept_password_auth(socket, |user, pass| {
                            user == *username && pass == *password
                        }).await?;
                        
                        // Add to whitelist after successful auth
                        whitelist.write().await.insert(client_ip);
                        info!("IP {} authenticated and whitelisted", client_ip);
                        
                        proto
                    },
                    _ => return Err(SocksError::ArgumentInputError("No acceptable authentication methods"))
                }
            } else {
                // Normal password auth without auth_once
                debug!("IP {} requires PASSWORD auth", client_ip);
                let (proto, ..) = Socks5ServerProtocol::accept_password_auth(socket, |user, pass| {
                    user == *username && pass == *password
                }).await?;
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

// Custom method negotiation similar to MicroSocks
async fn negotiate_auth_method(
    socket: &mut tokio::net::TcpStream,
    client_ip: IpAddr,
    whitelist: &Arc<RwLock<HashSet<IpAddr>>>,
) -> Result<u8, SocksError> {
    // Read method selection request
    let mut buf = [0u8; 257];
    let n = socket.read(&mut buf).await
        .map_err(|e| SocksError::Io(e))?;
    
    if n < 2 || buf[0] != 5 {
        return Err(SocksError::ArgumentInputError("Invalid SOCKS version"));
    }
    
    let n_methods = buf[1] as usize;
    if n < 2 + n_methods {
        return Err(SocksError::ArgumentInputError("Invalid data format"));
    }
    
    let methods = &buf[2..2+n_methods];
    
    // Check if IP is whitelisted
    let is_whitelisted = whitelist.read().await.contains(&client_ip);
    
    let selected_method = if is_whitelisted {
        // For whitelisted IPs, prefer NO_AUTH if available
        if methods.contains(&0) {
            debug!("IP {} whitelisted, selecting NO_AUTH", client_ip);
            0
        } else {
            0xFF // No acceptable methods
        }
    } else {
        // For non-whitelisted IPs, require PASSWORD auth
        if methods.contains(&2) {
            debug!("IP {} not whitelisted, selecting PASSWORD", client_ip);
            2
        } else {
            0xFF // No acceptable methods
        }
    };
    
    // Send method selection response
    let response = [5u8, selected_method];
    socket.write_all(&response).await
        .map_err(|e| SocksError::Io(e))?;
    
    if selected_method == 0xFF {
        return Err(SocksError::ArgumentInputError("No acceptable authentication methods"));
    }
    
    Ok(selected_method)
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
