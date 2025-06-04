use std::env;
use std::process::{Command, Stdio};
use std::path::Path;

fn main() {
    // Basic configuration
    let proxy_port = env::var("PROXY_PORT").unwrap_or_else(|_| "2324".to_string());
    let auth_mode = env::var("AUTH_MODE").unwrap_or_else(|_| "password".to_string());
    let allow_udp = env::var("ALLOW_UDP").unwrap_or_else(|_| "false".to_string());
    let public_addr = env::var("PUBLIC_ADDR").unwrap_or_default();
    let request_timeout = env::var("REQUEST_TIMEOUT").unwrap_or_else(|_| "10".to_string());
    let skip_auth = env::var("SKIP_AUTH").unwrap_or_else(|_| "false".to_string());
    
    // New authentication and whitelist features
    let auth_once = env::var("AUTH_ONCE").unwrap_or_else(|_| "false".to_string());
    let whitelist_ttl = env::var("WHITELIST_TTL").unwrap_or_else(|_| "0".to_string());
    let whitelist_file = env::var("WHITELIST_FILE").unwrap_or_else(|_| "/data/whitelist.json".to_string());
    let save_interval = env::var("SAVE_INTERVAL").unwrap_or_else(|_| "60".to_string());
    let backup_count = env::var("BACKUP_COUNT").unwrap_or_else(|_| "3".to_string());
    
    // Logging configuration
    let log_level = env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string());
    let enable_logs = env::var("ENABLE_LOGS").unwrap_or_else(|_| "false".to_string());

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

    // Add auth-once flag and related options if enabled
    if auth_once.to_lowercase() == "true" {
        args.push("--auth-once".to_string());
        
        // Add whitelist TTL if specified
        if whitelist_ttl != "0" {
            args.push("--whitelist-ttl".to_string());
            args.push(whitelist_ttl);
        }
        
        // Add whitelist file path for persistence
        args.push("--whitelist-file".to_string());
        args.push(whitelist_file.clone());
        
        // Add save interval
        args.push("--save-interval".to_string());
        args.push(save_interval);
        
        // Add backup count
        args.push("--backup-count".to_string());
        args.push(backup_count);
        
        // Ensure data directory exists
        if let Some(parent) = Path::new(&whitelist_file).parent() {
            if !parent.exists() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    eprintln!("Warning: Failed to create data directory {}: {}", parent.display(), e);
                }
            }
        }
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

    // Print configuration info if logs are enabled
    if enable_logs.to_lowercase() == "true" {
        println!("Starting SOCKS5 server with configuration:");
        println!("  Listen address: 0.0.0.0:{}", proxy_port);
        println!("  Authentication mode: {}", auth_mode);
        println!("  Request timeout: {}s", request_timeout);
        println!("  UDP support: {}", allow_udp);
        if !public_addr.is_empty() {
            println!("  Public address: {}", public_addr);
        }
        println!("  Skip auth: {}", skip_auth);
        println!("  Auth once: {}", auth_once);
        if auth_once.to_lowercase() == "true" {
            println!("  Whitelist TTL: {}s", whitelist_ttl);
            println!("  Whitelist file: {}", whitelist_file);
            println!("  Save interval: {}s", save_interval);
            println!("  Backup count: {}", backup_count);
        }
        println!("  Log level: {}", log_level);
        println!();
    }

    // Prepare environment variables for the server
    let mut cmd = Command::new("/usr/local/bin/fast-socks5-server");
    cmd.args(&args);

    // Configure logging
    if enable_logs.to_lowercase() == "true" {
        cmd.env("RUST_LOG", log_level);
        cmd.env("RUST_BACKTRACE", "1");
    } else {
        cmd.env("RUST_LOG", "off");
        cmd.env("RUST_BACKTRACE", "0");
        cmd.env_remove("RUST_LOG_STYLE");
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());
    }

    // Execute the fast-socks5-server with constructed arguments
    let status = cmd
        .status()
        .expect("Failed to execute fast-socks5-server");

    std::process::exit(status.code().unwrap_or(1));
}
