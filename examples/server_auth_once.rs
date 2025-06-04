use anyhow::Context;
use fast_socks5::{
    server::{run_tcp_proxy, run_udp_proxy, Socks5ServerProtocol},
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
struct Opt {
    #[structopt(short, long)]
    pub listen_addr: String,
    #[structopt(long)]
    pub public_addr: Option<std::net::IpAddr>,
    #[structopt(short = "t", long, default_value = "10")]
    pub request_timeout: u64,
    #[structopt(subcommand)]
    pub auth: AuthMode,
    #[structopt(short = "k", long)]
    pub skip_auth: bool,
    #[structopt(short = "U", long)]
    pub allow_udp: bool,
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
        self.authenticated_ips.read().await.contains(ip)
    }

    async fn add_authenticated_ip(&self, ip: IpAddr) {
        self.authenticated_ips.write().await.insert(ip);
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let opt: &'static Opt = Box::leak(Box::new(Opt::from_args()));
    
    if opt.allow_udp && opt.public_addr.is_none() {
        return Err(SocksError::ArgumentInputError(""));
    }
    if opt.skip_auth && opt.auth != AuthMode::NoAuth {
        return Err(SocksError::ArgumentInputError(""));
    }
    if opt.auth_once && opt.auth == AuthMode::NoAuth {
        return Err(SocksError::ArgumentInputError(""));
    }
    if opt.auth_once && opt.skip_auth {
        return Err(SocksError::ArgumentInputError(""));
    }

    let listener = TcpListener::bind(&opt.listen_addr).await?;
    let auth_state = AuthState::new();

    loop {
        match listener.accept().await {
            Ok((socket, client_addr)) => {
                let auth_state_clone = auth_state.clone();
                task::spawn(async move {
                    if let Err(e) = handle_connection(opt, socket, client_addr.ip(), auth_state_clone).await {
                        let _ = e;
                    }
                });
            }
            Err(_) => continue,
        }
    }
}

async fn handle_connection(
    opt: &Opt,
    socket: tokio::net::TcpStream,
    client_ip: IpAddr,
    auth_state: AuthState,
) -> Result<()> {
    let proto = match &opt.auth {
        AuthMode::NoAuth => {
            if opt.skip_auth {
                Socks5ServerProtocol::skip_auth_this_is_not_rfc_compliant(socket)
            } else {
                Socks5ServerProtocol::accept_no_auth(socket).await?
            }
        }
        AuthMode::Password { username, password } => {
            if opt.auth_once && auth_state.is_authenticated(&client_ip).await {
                Socks5ServerProtocol::accept_no_auth(socket).await?
            } else {
                let (proto, auth_success) = Socks5ServerProtocol::accept_password_auth(socket, |u, p| {
                    u == username && p == password
                }).await?;

                if auth_success && opt.auth_once {
                    auth_state.add_authenticated_ip(client_ip).await;
                }
                
                proto
            }
        }
    };

    let (proto, cmd, target_addr) = proto.read_command().await?.resolve_dns().await?;

    match cmd {
        Socks5Command::TCPConnect => {
            run_tcp_proxy(proto, &target_addr, opt.request_timeout, false).await?;
        }
        Socks5Command::UDPAssociate if opt.allow_udp => {
            let reply_ip = opt.public_addr.context("")?;
            run_udp_proxy(proto, &target_addr, None, reply_ip, None).await?;
        }
        _ => {
            proto.reply_error(&ReplyError::CommandNotSupported).await?;
            return Err(ReplyError::CommandNotSupported.into());
        }
    };
    
    Ok(())
}
