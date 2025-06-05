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
};

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

    /// Enable one-time authentication (IP whitelist after first successful auth)
    #[structopt(long)]
    pub auth_once: bool,

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
    info!("Listen for socks connections @ {}", &opt.listen_addr);

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

// Custom auth selection logic theo pseudo-code yêu cầu
fn select_auth_method(client_methods: &[u8], opt: &Opt, client_ip: IpAddr, ip_whitelisted: bool) -> Option<u8> {
    let auth_user = !matches!(opt.auth, AuthMode::NoAuth);
    
    // Duyệt theo thứ tự client gửi
    for &method in client_methods {
        match method {
            0x00 => { // AM_NO_AUTH
                if !auth_user {
                    return Some(0x00); // AM_NO_AUTH
                } else if opt.auth_once && ip_whitelisted {
                    return Some(0x00); // AM_NO_AUTH
                }
                // IP chưa whitelist => không chọn NO_AUTH, tiếp tục duyệt
            }
            0x02 => { // AM_USERNAME
                if auth_user {
                    return Some(0x02); // AM_USERNAME
                }
                // Nếu không bật auth thì bỏ qua
            }
            _ => {
                // Bỏ qua các method không hỗ trợ
                continue;
            }
        }
    }
    
    // Không method nào hợp lệ → từ chối kết nối
    None // AM_INVALID
}

async fn serve_socks5(
    opt: &Opt,
    mut socket: tokio::net::TcpStream,
    client_ip: IpAddr,
    state: Arc<ServerState>,
) -> Result<(), SocksError> {
    // Đọc SOCKS5 greeting từ client
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    
    let mut buf = [0u8; 2];
    socket.read_exact(&mut buf).await.map_err(|_| SocksError::InvalidData)?;
    
    if buf[0] != 0x05 {
        return Err(SocksError::InvalidData);
    }
    
    let nmethods = buf[1] as usize;
    if nmethods == 0 {
        return Err(SocksError::InvalidData);
    }
    
    let mut methods = vec![0u8; nmethods];
    socket.read_exact(&mut methods).await.map_err(|_| SocksError::InvalidData)?;
    
    // Kiểm tra IP có trong whitelist không
    let ip_whitelisted = if opt.auth_once {
        state.auth_once_ips.read().await.contains(&client_ip)
    } else {
        false
    };
    
    // Áp dụng logic selection theo pseudo-code
    let selected_method = select_auth_method(&methods, opt, client_ip, ip_whitelisted);
    
    let auth_method = match selected_method {
        Some(method) => method,
        None => {
            // Từ chối kết nối
            socket.write_all(&[0x05, 0xFF]).await.map_err(|_| SocksError::InvalidData)?;
            return Err(SocksError::InvalidData);
        }
    };
    
    // Gửi phản hồi method selection
    socket.write_all(&[0x05, auth_method]).await.map_err(|_| SocksError::InvalidData)?;
    
    let (proto, cmd, target_addr) = match auth_method {
        0x00 => { // NO_AUTH
            if opt.skip_auth {
                Socks5ServerProtocol::skip_auth_this_is_not_rfc_compliant(socket)
            } else {
                // Tạo protocol object từ socket đã authenticated
                // (Cần modify fast_socks5 để support trường hợp này)
                // Tạm thời dùng accept_no_auth
                Socks5ServerProtocol::accept_no_auth(socket).await?
            }
        }
        0x02 => { // USERNAME/PASSWORD
            if let AuthMode::Password { username, password } = &opt.auth {
                let auth_result = Socks5ServerProtocol::accept_password_auth(
                    socket,
                    |user, pass| {
                        let auth_ok = user == *username && pass == *password;
                        if auth_ok && opt.auth_once {
                            // Add to whitelist if auth succeeds
                            task::spawn({
                                let state = state.clone();
                                let client_ip = client_ip;
                                async move {
                                    state.auth_once_ips.write().await.insert(client_ip);
                                }
                            });
                        }
                        auth_ok
                    },
                ).await?;
                auth_result.0
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
        match fut.await {
            Ok(()) => {}
            Err(err) => error!("{:#}", &err),
        }
    })
}
