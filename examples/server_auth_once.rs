#[forbid(unsafe_code)]
#[macro_use]
extern crate log;

use anyhow::Context;
use fast_socks5::{
    server::{run_tcp_proxy, run_udp_proxy, DnsResolveHelper as _, Socks5ServerProtocol},
    ReplyError, Result, Socks5Command, SocksError,
};
use std::{
    collections::HashSet,
    future::Future,
    net::{IpAddr, SocketAddr},
    sync::Arc,
};
use structopt::StructOpt;
use tokio::{
    net::TcpListener,
    sync::RwLock,
    task,
    io::{AsyncReadExt, AsyncWriteExt},
};

#[derive(Debug, StructOpt)]
#[structopt(
    name = "socks5-server",
    about = "A simple implementation of a socks5-server."
)]
struct Opt {
    #[structopt(short, long)]
    pub listen_addr: String,

    #[structopt(long)]
    pub public_addr: Option<IpAddr>,

    #[structopt(short = "t", long, default_value = "10")]
    pub request_timeout: u64,

    #[structopt(subcommand, name = "auth")]
    pub auth: AuthMode,

    #[structopt(long)]
    pub auth_once: bool,

    #[structopt(short = "k", long)]
    pub skip_auth: bool,

    #[structopt(short = "U", long)]
    pub allow_udp: bool,
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

struct ServerState {
    auth_once_ips: RwLock<HashSet<IpAddr>>,
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

    if opt.auth_once && matches!(opt.auth, AuthMode::NoAuth) {
        return Err(SocksError::ArgumentInputError(
            "Can't use auth-once with no-auth mode",
        ));
    }

    let state = Arc::new(ServerState {
        auth_once_ips: RwLock::new(HashSet::new()),
    });

    let listener = TcpListener::bind(&opt.listen_addr).await?;
    info!("Listening @ {}", &opt.listen_addr);

    loop {
        match listener.accept().await {
            Ok((socket, client_addr)) => {
                let state = state.clone();
                spawn_and_log_error(serve_socks5(opt, socket, client_addr.ip(), state));
            }
            Err(err) => {
                error!("accept error = {:?}", err);
            }
        }
    }
}

fn select_auth_method(client_methods: &[u8], opt: &Opt, client_ip: IpAddr, ip_whitelisted: bool) -> Option<u8> {
    let auth_user = !matches!(opt.auth, AuthMode::NoAuth);
    for &method in client_methods {
        match method {
            0x00 => {
                if !auth_user || (opt.auth_once && ip_whitelisted) {
                    return Some(0x00);
                }
            }
            0x02 => {
                if auth_user {
                    return Some(0x02);
                }
            }
            _ => continue,
        }
    }
    None
}

async fn serve_socks5(
    opt: &Opt,
    mut socket: tokio::net::TcpStream,
    client_ip: IpAddr,
    state: Arc<ServerState>,
) -> Result<(), SocksError> {
    let mut buf = [0u8; 2];
    socket.read_exact(&mut buf).await.map_err(|_| SocksError::InvalidData)?;
    if buf[0] != 0x05 {
        return Err(SocksError::InvalidData);
    }

    let nmethods = buf[1] as usize;
    let mut methods = vec![0u8; nmethods];
    socket.read_exact(&mut methods).await.map_err(|_| SocksError::InvalidData)?;

    let ip_whitelisted = opt.auth_once && state.auth_once_ips.read().await.contains(&client_ip);
    let selected = select_auth_method(&methods, opt, client_ip, ip_whitelisted);

    let auth_method = match selected {
        Some(m) => m,
        None => {
            socket.write_all(&[0x05, 0xFF]).await.ok();
            return Err(SocksError::InvalidData);
        }
    };

    socket.write_all(&[0x05, auth_method]).await.map_err(|_| SocksError::InvalidData)?;

    let (proto, cmd, target_addr) = match auth_method {
        0x00 => {
            if opt.skip_auth {
                Socks5ServerProtocol::skip_auth_this_is_not_rfc_compliant(socket)
            } else {
                Socks5ServerProtocol::accept_no_auth(socket).await?
            }
        }
        0x02 => {
            if let AuthMode::Password { username, password } = &opt.auth {
                let (proto, _) = Socks5ServerProtocol::accept_password_auth(socket, {
                    let username = username.clone();
                    let password = password.clone();
                    move |user, pass| user == username && pass == password
                }).await?;

                if opt.auth_once {
                    let state = state.clone();
                    task::spawn(async move {
                        state.auth_once_ips.write().await.insert(client_ip);
                    });
                }

                proto
            } else {
                return Err(SocksError::InvalidData);
            }
        }
        _ => return Err(SocksError::InvalidData),
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
        if let Err(e) = fut.await {
            error!("{:#}", e);
        }
    })
}
