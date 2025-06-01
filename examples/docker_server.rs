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

    // Build command arguments
    let mut args = vec![
        "--listen-addr".to_string(),
        format!("0.0.0.0:{}", proxy_port),
        "--request-timeout".to_string(),
        request_timeout,
    ];

    // Add UDP support if enabled
    if allow_udp.to_lowercase() == "true" && !public_addr.is_empty() {
        args.extend_from_slice(&["--allow-udp".to_string(), "--public-addr".to_string(), public_addr]);
    }

    // Add skip-auth if enabled
    if skip_auth.to_lowercase() == "true" {
        args.push("--skip-auth".to_string());
    }

    // Configure authentication
    if auth_mode == "no-auth" {
        args.push("no-auth".to_string());
    } else {
        let proxy_user = env::var("PROXY_USER").expect("PROXY_USER environment variable required");
        let proxy_password = env::var("PROXY_PASSWORD").expect("PROXY_PASSWORD environment variable required");
        
        args.extend_from_slice(&[
            "password".to_string(),
            "--username".to_string(),
            proxy_user,
            "--password".to_string(),
            proxy_password,
        ]);
    }

    // Execute the server
    let status = Command::new("/fast-socks5-server")
        .args(&args)
        .status()
        .expect("Failed to execute server");

    std::process::exit(status.code().unwrap_or(1));
}
