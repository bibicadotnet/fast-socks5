use std::env;
use std::process::{Command, Stdio};

fn main() {
    let proxy_port = env::var("PROXY_PORT").unwrap_or_else(|_| "2324".to_string());
    let auth_mode = env::var("AUTH_MODE").unwrap_or_else(|_| "password".to_string());
    let allow_udp = env::var("ALLOW_UDP").unwrap_or_else(|_| "false".to_string());
    let public_addr = env::var("PUBLIC_ADDR").unwrap_or_default();
    let request_timeout = env::var("REQUEST_TIMEOUT").unwrap_or_else(|_| "10".to_string());
    let skip_auth = env::var("SKIP_AUTH").unwrap_or_else(|_| "false".to_string());
    let auth_once = env::var("AUTH_ONCE").unwrap_or_else(|_| "false".to_string());

    let mut args = vec![
        "--listen-addr".to_string(),
        format!("0.0.0.0:{}", proxy_port),
        "--request-timeout".to_string(),
        request_timeout,
    ];

    // Add UDP support if enabled
    if allow_udp.to_lowercase() == "true" && !public_addr.is_empty() {
        args.push("--allow-udp".to_string());
        args.push("--public-addr".to_string());
        args.push(public_addr);
    }

    // Add skip-auth flag if enabled
    if skip_auth.to_lowercase() == "true" {
        args.push("--skip-auth".to_string());
    }

    // Add auth-once flag if enabled
    if auth_once.to_lowercase() == "true" {
        args.push("--auth-once".to_string());
    }

    // Configure authentication mode
    if auth_mode == "no-auth" {
        args.push("no-auth".to_string());
    } else {
        match (env::var("PROXY_USER"), env::var("PROXY_PASSWORD")) {
            (Ok(user), Ok(password)) => {
                args.push("password".to_string());
                args.push("--username".to_string());
                args.push(user);
                args.push("--password".to_string());
                args.push(password);
            }
            _ => {
                eprintln!("Error: PROXY_USER and PROXY_PASSWORD environment variables are required for password authentication");
                std::process::exit(1);
            }
        }
    }

    // Execute the fast-socks5-server with constructed arguments
    let status = Command::new("/usr/local/bin/fast-socks5-server")
        .args(&args)
        .env("RUST_LOG", "off")
        .env("RUST_BACKTRACE", "0")
        .env_remove("RUST_LOG_STYLE")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("Failed to execute fast-socks5-server");

    std::process::exit(status.code().unwrap_or(1));
}
