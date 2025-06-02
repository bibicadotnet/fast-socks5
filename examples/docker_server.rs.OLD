use std::env;
use std::process::Command;

fn main() {
    // Disable logging output
    env::set_var("RUST_LOG", "off");
    env::set_var("RUST_BACKTRACE", "0");
    env::remove_var("RUST_LOG_STYLE");

    // Default values
    let proxy_port = env::var("PROXY_PORT").unwrap_or_else(|_| "2324".to_string());
    let auth_mode = env::var("AUTH_MODE").unwrap_or_else(|_| "password".to_string());
    let allow_udp = env::var("ALLOW_UDP").unwrap_or_else(|_| "false".to_string());
    let public_addr = env::var("PUBLIC_ADDR").unwrap_or_default();
    let request_timeout = env::var("REQUEST_TIMEOUT").unwrap_or_else(|_| "10".to_string());
    let skip_auth = env::var("SKIP_AUTH").unwrap_or_else(|_| "false".to_string());

    // Build base command arguments
    let mut args = vec![
        "--listen-addr".to_string(),
        format!("0.0.0.0:{}", proxy_port),
        "--request-timeout".to_string(),
        request_timeout,
    ];

    // Add UDP support if enabled and public_addr is provided
    if allow_udp.to_lowercase() == "true" && !public_addr.is_empty() {
        args.push("--allow-udp".to_string());
        args.push("--public-addr".to_string());
        args.push(public_addr);
    }

    // Add skip-auth if enabled
    if skip_auth.to_lowercase() == "true" {
        args.push("--skip-auth".to_string());
    }

    // Configure authentication mode
    if auth_mode == "no-auth" {
        args.push("no-auth".to_string());
    } else {
        // For password mode, check if credentials are provided
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

    // Execute the server - sử dụng đường dẫn chính xác
    let status = Command::new("/usr/local/bin/fast-socks5-server")
        .args(&args)
        .status()
        .expect("Failed to execute fast-socks5-server");

    // Exit with the same code as the server
    std::process::exit(status.code().unwrap_or(1));
}
