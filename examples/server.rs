#[forbid(unsafe_code)]
#[macro_use]
extern crate log;

use anyhow::Context;
use fast_socks5::{
    server::{run_tcp_proxy, run_udp_proxy, DnsResolveHelper as _, Socks5ServerProtocol},
    ReplyError, Result, Socks5Command, SocksError,
};
use std::future::Future;
use structopt::StructOpt;
use tokio::net::TcpListener;
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

    let listener = TcpListener::bind(&opt.listen_addr).await?;

    info!("Listen for socks connections @ {}", &opt.listen_addr);

    // Standard TCP loop
    loop {
        match listener.accept().await {
            Ok((socket, _client_addr)) => {
                spawn_and_log_error(serve_socks5(opt, socket));
            }
            Err(err) => {
                error!("accept error = {:?}", err);
            }
        }
    }
}

async fn negotiate_auth_method(
    socket: &mut tokio::net::TcpStream,
    client_ip: IpAddr,
    whitelist: &Arc<RwLock<HashSet<IpAddr>>>,
    auth_mode: &AuthMode
) -> Result<u8, SocksError> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    
    // Read method selection request
    let mut buf = [0u8; 257];
    let n = socket.read(&mut buf).await?;
    
    if n < 2 || buf[0] != 5 {
        return Err(SocksError::InvalidData);
    }
    
    let n_methods = buf[1] as usize;
    if n < 2 + n_methods {
        return Err(SocksError::InvalidData);
    }
    
    let methods = &buf[2..2+n_methods];
    
    // Check available methods like MicroSocks
    let selected_method = match auth_mode {
        AuthMode::NoAuth => {
            if methods.contains(&0) { 0 } else { 0xFF }
        },
        AuthMode::Password { .. } => {
            // Check if IP is whitelisted (auth_once logic)
            if whitelist.read().await.contains(&client_ip) {
                debug!("IP {} whitelisted, selecting NO_AUTH", client_ip);
                if methods.contains(&0) { 0 } else { 0xFF }
            } else {
                debug!("IP {} requires PASSWORD auth", client_ip);
                if methods.contains(&2) { 2 } else { 0xFF }
            }
        }
    };
    
    // Send method selection response
    let response = [5u8, selected_method];
    socket.write_all(&response).await?;
    
    Ok(selected_method)
}

// Modify serve_socks5 function
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
            // Use proper method negotiation
            let selected_method = negotiate_auth_method(&mut socket, client_ip, &whitelist, &opt.auth).await?;
            
            match selected_method {
                0 => {
                    // NO_AUTH selected (whitelisted IP)
                    Socks5ServerProtocol::accept_no_auth(socket).await?
                },
                2 => {
                    // PASSWORD auth selected
                    let (proto, ..) = Socks5ServerProtocol::accept_password_auth(socket, |user, pass| {
                        user == *username && pass == *password
                    }).await?;
                    
                    // Add to whitelist after successful auth
                    if opt.auth_once {
                        whitelist.write().await.insert(client_ip);
                        info!("IP {} authenticated and whitelisted", client_ip);
                    }
                    
                    proto
                },
                _ => return Err(SocksError::NoAcceptableAuthMethods)
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
