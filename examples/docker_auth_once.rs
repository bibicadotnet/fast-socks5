use std::env;
use std::process::{Command, Stdio};

fn main() {
    // Basic configuration
    let proxy_port = env::var("PROXY_PORT").unwrap_or_else(|_| "2324".to_string());
    let auth_mode = env::var("AUTH_MODE").unwrap_or_else(|_| "password".to_string());
    let request_timeout = env::var("REQUEST_TIMEOUT").unwrap_or_else(|_| "10".to_string());
    
    // UDP configuration
    let allow_udp = env::var("ALLOW_UDP").unwrap_or_else(|_| "false".to_string());
    let public_addr = env::var("PUBLIC_ADDR").unwrap_or_default();
    
    // Authentication configuration
    let skip_auth = env::var("SKIP_AUTH").unwrap_or_else(|_| "false".to_string());
    let auth_once = env::var("AUTH_ONCE").unwrap_or_else(|_| "false".to_string());
    let whitelist_ttl = env::var("WHITELIST_TTL").unwrap_or_else(|_| "0".to_string());
    
    // Logging configuration
    let log_level = env::var("LOG_LEVEL").unwrap_or_else(|_| "off".to_string());
    let log_output = env::var("LOG_OUTPUT").unwrap_or_else(|_| "null".to_string());
    
    // Build command arguments
    let mut args = vec![
        "--listen-addr".to_string(),
        format!("0.0.0.0:{}", proxy_port),
        "--request-timeout".to_string(),
        request_timeout,
    ];

    // Add UDP support if enabled
    if allow_udp.to_lowercase() == "true" {
        if public_addr.is_empty() {
            eprintln!("ERROR: ALLOW_UDP=true requires PUBLIC_ADDR to be set");
            std::process::exit(1);
        }
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

    // Add whitelist TTL if specified and auth-once is enabled
    if auth_once.to_lowercase() == "true" && whitelist_ttl != "0" {
        match whitelist_ttl.parse::<u64>() {
            Ok(ttl) if ttl > 0 => {
                args.push("--whitelist-ttl".to_string());
                args.push(ttl.to_string());
            }
            _ => {
                eprintln!("ERROR: WHITELIST_TTL must be a positive number");
                std::process::exit(1);
            }
        }
    }

    // Configure authentication method
    match auth_mode.as_str() {
        "no-auth" => {
            // Validate incompatible combinations
            if auth_once.to_lowercase() == "true" {
                eprintln!("ERROR: AUTH_ONCE cannot be used with AUTH_MODE=no-auth");
                std::process::exit(1);
            }
            args.push("no-auth".to_string());
        }
        "password" => {
            match (env::var("PROXY_USER"), env::var("PROXY_PASSWORD")) {
                (Ok(user), Ok(password)) => {
                    if user.is_empty() || password.is_empty() {
                        eprintln!("ERROR: PROXY_USER and PROXY_PASSWORD cannot be empty");
                        std::process::exit(1);
                    }
                    args.push("password".to_string());
                    args.push("--username".to_string());
                    args.push(user);
                    args.push("--password".to_string());
                    args.push(password);
                }
                _ => {
                    eprintln!("ERROR: AUTH_MODE=password requires PROXY_USER and PROXY_PASSWORD");
                    std::process::exit(1);
                }
            }
        }
        _ => {
            eprintln!("ERROR: AUTH_MODE must be 'no-auth' or 'password'");
            std::process::exit(1);
        }
    }

    // Validate skip-auth combinations
    if skip_auth.to_lowercase() == "true" {
        if auth_mode != "no-auth" {
            eprintln!("ERROR: SKIP_AUTH=true can only be used with AUTH_MODE=no-auth");
            std::process::exit(1);
        }
        if auth_once.to_lowercase() == "true" {
            eprintln!("ERROR: SKIP_AUTH=true cannot be used with AUTH_ONCE=true");
            std::process::exit(1);
        }
    }

    // Print configuration summary if logging is enabled
    if log_level != "off" {
        println!("SOCKS5 Server Configuration:");
        println!("  Listen Address: 0.0.0.0:{}", proxy_port);
        println!("  Auth Mode: {}", auth_mode);
        println!("  Request Timeout: {}s", request_timeout);
        println!("  UDP Support: {}", allow_udp);
        if allow_udp.to_lowercase() == "true" {
            println!("  Public Address: {}", env::var("PUBLIC_ADDR").unwrap_or_default());
        }
        println!("  Skip Auth: {}", skip_auth);
        println!("  Auth Once: {}", auth_once);
        if auth_once.to_lowercase() == "true" && whitelist_ttl != "0" {
            println!("  Whitelist TTL: {}s", whitelist_ttl);
        }
        println!("  Command: /usr/local/bin/fast-socks5-server {}", args.join(" "));
        println!("---");
    }

    // Configure command execution
    let mut command = Command::new("/usr/local/bin/fast-socks5-server");
    command.args(&args);
    
    // Set environment variables
    command.env("RUST_LOG", &log_level);
    command.env("RUST_BACKTRACE", "0");
    command.env_remove("RUST_LOG_STYLE");

    // Configure output redirection
    match log_output.as_str() {
        "null" => {
            command.stdout(Stdio::null()).stderr(Stdio::null());
        }
        "inherit" => {
            command.stdout(Stdio::inherit()).stderr(Stdio::inherit());
        }
        "stdout" => {
            command.stdout(Stdio::inherit()).stderr(Stdio::null());
        }
        "stderr" => {
            command.stdout(Stdio::null()).stderr(Stdio::inherit());
        }
        _ => {
            command.stdout(Stdio::null()).stderr(Stdio::null());
        }
    }

    // Execute the server
    let status = command
        .status()
        .expect("Failed to execute fast-socks5-server");

    std::process::exit(status.code().unwrap_or(1));
}
