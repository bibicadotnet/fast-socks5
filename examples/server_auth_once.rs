#[forbid(unsafe_code)]
#[macro_use]
extern crate log;

use anyhow::Context;
use fast_socks5::{
    server::{run_tcp_proxy, run_udp_proxy, DnsResolveHelper as _, Socks5ServerProtocol},
    ReplyError, Result, Socks5Command, SocksError,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;
use structopt::StructOpt;
use tokio::fs;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio::task;

/// # How to use it:
///
/// Listen on a local address, authentication-free:
///     `$ RUST_LOG=debug cargo run --example server -- --listen-addr 127.0.0.1:1337 no-auth`
///
/// Listen on a local address, with basic username/password requirement:
///     `$ RUST_LOG=debug cargo run --example server -- --listen-addr 127.0.0.1:1337 password --username admin --password password`
///
/// Same as above but with UDP support and persistent whitelist:
///     `$ RUST_LOG=debug cargo run --example server -- --listen-addr 127.0.0.1:1337 --allow-udp --public-addr 127.0.0.1 --whitelist-file ./whitelist.json password --username admin --password password`
///
/// Listen with one-time authentication and disk persistence:
///     `$ RUST_LOG=debug cargo run --example server -- --listen-addr 127.0.0.1:1337 --auth-once --whitelist-file ./socks5_whitelist.json password --username admin --password password`
#[derive(Debug, StructOpt)]
#[structopt(
    name = "socks5-server",
    about = "A simple implementation of a socks5-server with persistent whitelist."
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

    /// Time in seconds to cache whitelist status (0 = no expiry)
    #[structopt(long, default_value = "0")]
    pub whitelist_ttl: u64,

    /// File path to store persistent whitelist (JSON format)
    #[structopt(long)]
    pub whitelist_file: Option<PathBuf>,

    /// Auto-save interval in seconds (0 = save immediately after each change)
    #[structopt(long, default_value = "60")]
    pub save_interval: u64,

    /// Backup old whitelist files (keep N backups)
    #[structopt(long, default_value = "3")]
    pub backup_count: u32,
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

/// Whitelist entry with optional expiry (serializable)
#[derive(Debug, Clone, Serialize, Deserialize)]
struct WhitelistEntry {
    #[serde(with = "timestamp_serde")]
    added_at: std::time::SystemTime,
    ttl_seconds: Option<u64>,
    /// Optional metadata for debugging/monitoring
    first_auth_user: Option<String>,
    connection_count: u64,
}

impl WhitelistEntry {
    fn new(ttl_seconds: Option<u64>, auth_user: Option<String>) -> Self {
        Self {
            added_at: std::time::SystemTime::now(),
            ttl_seconds,
            first_auth_user: auth_user,
            connection_count: 0,
        }
    }

    fn is_expired(&self) -> bool {
        if let Some(ttl) = self.ttl_seconds {
            if let Ok(elapsed) = self.added_at.elapsed() {
                elapsed.as_secs() > ttl
            } else {
                true // Clock went backwards, consider expired
            }
        } else {
            false
        }
    }

    fn increment_usage(&mut self) {
        self.connection_count += 1;
    }
}

/// Serializable whitelist data
#[derive(Debug, Clone, Serialize, Deserialize)]
struct WhitelistData {
    version: u32,
    created_at: std::time::SystemTime,
    last_modified: std::time::SystemTime,
    entries: HashMap<IpAddr, WhitelistEntry>,
}

impl Default for WhitelistData {
    fn default() -> Self {
        let now = std::time::SystemTime::now();
        Self {
            version: 1,
            created_at: now,
            last_modified: now,
            entries: HashMap::new(),
        }
    }
}

/// Shared state for authenticated IPs with disk persistence
#[derive(Debug, Clone)]
struct AuthState {
    /// In-memory cache
    cache: Arc<RwLock<HashMap<IpAddr, WhitelistEntry>>>,
    /// File path for persistence
    file_path: Option<PathBuf>,
    /// Pending changes (for batched saves)
    dirty: Arc<RwLock<bool>>,
    /// Save configuration
    save_interval: u64,
    backup_count: u32,
}

impl AuthState {
    async fn new(file_path: Option<PathBuf>, save_interval: u64, backup_count: u32) -> Result<Self> {
        let state = Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            file_path: file_path.clone(),
            dirty: Arc::new(RwLock::new(false)),
            save_interval,
            backup_count,
        };

        // Load existing whitelist from disk
        if let Some(path) = &file_path {
            if let Err(e) = state.load_from_disk().await {
                warn!("Failed to load whitelist from {}: {}", path.display(), e);
                info!("Starting with empty whitelist");
            }
        }

        Ok(state)
    }

    /// Load whitelist from disk
    async fn load_from_disk(&self) -> Result<()> {
        let path = self.file_path.as_ref().context("No file path configured")?;
        
        if !path.exists() {
            info!("Whitelist file {} doesn't exist, starting fresh", path.display());
            return Ok(());
        }

        let content = fs::read_to_string(path).await
            .with_context(|| format!("Failed to read whitelist file: {}", path.display()))?;

        let data: WhitelistData = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse whitelist JSON: {}", path.display()))?;

        let mut cache = self.cache.write().await;
        let mut loaded_count = 0;
        let mut expired_count = 0;

        for (ip, entry) in data.entries {
            if entry.is_expired() {
                expired_count += 1;
                debug!("Skipping expired entry for IP {}", ip);
            } else {
                cache.insert(ip, entry);
                loaded_count += 1;
            }
        }

        info!("Loaded {} whitelist entries from disk ({} expired entries skipped)", 
              loaded_count, expired_count);

        // Mark as dirty if we skipped expired entries (will clean up on next save)
        if expired_count > 0 {
            *self.dirty.write().await = true;
        }

        Ok(())
    }

    /// Save whitelist to disk
    async fn save_to_disk(&self) -> Result<()> {
        let path = self.file_path.as_ref().context("No file path configured")?;
        
        let cache = self.cache.read().await;
        let data = WhitelistData {
            version: 1,
            created_at: std::time::SystemTime::now(), // Could track this better
            last_modified: std::time::SystemTime::now(),
            entries: cache.clone(),
        };
        drop(cache);

        // Create backup if file exists
        if path.exists() && self.backup_count > 0 {
            self.create_backup(path).await?;
        }

        // Write to temporary file first
        let temp_path = path.with_extension("tmp");
        let json_content = serde_json::to_string_pretty(&data)
            .context("Failed to serialize whitelist data")?;

        fs::write(&temp_path, json_content).await
            .with_context(|| format!("Failed to write temp file: {}", temp_path.display()))?;

        // Atomic move
        fs::rename(&temp_path, path).await
            .with_context(|| format!("Failed to move temp file to: {}", path.display()))?;

        *self.dirty.write().await = false;
        
        info!("Saved {} whitelist entries to disk", data.entries.len());
        Ok(())
    }

    /// Create backup of existing file
    async fn create_backup(&self, path: &PathBuf) -> Result<()> {
        let backup_path = path.with_extension(format!("bak.{}", 
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        ));

        if let Err(e) = fs::copy(path, &backup_path).await {
            warn!("Failed to create backup {}: {}", backup_path.display(), e);
        } else {
            debug!("Created backup: {}", backup_path.display());
        }

        // Clean old backups
        self.cleanup_old_backups(path).await;
        Ok(())
    }

    /// Remove old backup files
    async fn cleanup_old_backups(&self, path: &PathBuf) {
        if let Some(parent) = path.parent() {
            if let Ok(mut entries) = fs::read_dir(parent).await {
                let mut backups = Vec::new();
                let base_name = path.file_stem().unwrap_or_default();

                while let Ok(Some(entry)) = entries.next_entry().await {
                    if let Some(name) = entry.file_name().to_str() {
                        if name.starts_with(&format!("{}.bak.", base_name.to_string_lossy())) {
                            if let Ok(metadata) = entry.metadata().await {
                                if let Ok(modified) = metadata.modified() {
                                    backups.push((entry.path(), modified));
                                }
                            }
                        }
                    }
                }

                // Sort by modification time (newest first)
                backups.sort_by(|a, b| b.1.cmp(&a.1));

                // Remove old backups
                for (path, _) in backups.iter().skip(self.backup_count as usize) {
                    if let Err(e) = fs::remove_file(path).await {
                        debug!("Failed to remove old backup {}: {}", path.display(), e);
                    } else {
                        debug!("Removed old backup: {}", path.display());
                    }
                }
            }
        }
    }

    /// Check if an IP is currently authenticated (and not expired)
    async fn is_authenticated(&self, ip: &IpAddr) -> bool {
        let mut cache = self.cache.write().await;
        
        if let Some(entry) = cache.get_mut(ip) {
            if entry.is_expired() {
                info!("IP {} whitelist entry expired, removing from cache", ip);
                cache.remove(ip);
                *self.dirty.write().await = true;
                false
            } else {
                // Increment usage counter
                entry.increment_usage();
                true
            }
        } else {
            false
        }
    }

    /// Add an IP to the authenticated list
    async fn add_authenticated_ip(&self, ip: IpAddr, ttl_seconds: Option<u64>, auth_user: String) {
        let mut cache = self.cache.write().await;
        let entry = WhitelistEntry::new(ttl_seconds, auth_user);
        cache.insert(ip, entry);
        drop(cache);

        *self.dirty.write().await = true;
        
        if let Some(ttl) = ttl_seconds {
            info!("IP {} added to whitelist (expires in {} seconds)", ip, ttl);
        } else {
            info!("IP {} added to permanent whitelist", ip);
        }

        // Save immediately if save_interval is 0
        if self.save_interval == 0 && self.file_path.is_some() {
            if let Err(e) = self.save_to_disk().await {
                error!("Failed to save whitelist immediately: {}", e);
            }
        }
    }

    /// Get count of authenticated IPs (cleaning expired ones)
    async fn authenticated_count(&self) -> usize {
        let cache = self.cache.read().await;
        cache.len()
    }

    /// Clean expired entries
    async fn cleanup_expired(&self) {
        let mut cache = self.cache.write().await;
        let before_count = cache.len();
        
        cache.retain(|ip, entry| {
            if entry.is_expired() {
                debug!("Removing expired whitelist entry for IP {}", ip);
                false
            } else {
                true
            }
        });
        
        let after_count = cache.len();
        if before_count != after_count {
            info!("Cleaned {} expired whitelist entries", before_count - after_count);
            *self.dirty.write().await = true;
        }
    }

    /// Periodic save task
    async fn save_periodically(&self) {
        if self.save_interval == 0 || self.file_path.is_none() {
            return;
        }

        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(self.save_interval));
        
        loop {
            interval.tick().await;
            
            let is_dirty = *self.dirty.read().await;
            if is_dirty {
                if let Err(e) = self.save_to_disk().await {
                    error!("Periodic save failed: {}", e);
                } else {
                    debug!("Periodic save completed");
                }
            }
        }
    }

    /// Get statistics for monitoring
    async fn get_stats(&self) -> HashMap<String, serde_json::Value> {
        let cache = self.cache.read().await;
        let mut stats = HashMap::new();
        
        stats.insert("total_entries".to_string(), cache.len().into());
        
        let mut permanent_count = 0;
        let mut temporary_count = 0;
        let mut total_connections = 0u64;
        
        for entry in cache.values() {
            if entry.ttl_seconds.is_some() {
                temporary_count += 1;
            } else {
                permanent_count += 1;
            }
            total_connections += entry.connection_count;
        }
        
        stats.insert("permanent_entries".to_string(), permanent_count.into());
        stats.insert("temporary_entries".to_string(), temporary_count.into());
        stats.insert("total_connections".to_string(), total_connections.into());
        stats.insert("is_persistent".to_string(), self.file_path.is_some().into());
        
        stats
    }
}

// Custom serde module for SystemTime
mod timestamp_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::{SystemTime, UNIX_EPOCH};

    pub fn serialize<S>(time: &SystemTime, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let duration = time.duration_since(UNIX_EPOCH).unwrap_or_default();
        duration.as_secs().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<SystemTime, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = u64::deserialize(deserializer)?;
        Ok(UNIX_EPOCH + std::time::Duration::from_secs(secs))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    spawn_socks_server().await
}

async fn spawn_socks_server() -> Result<()> {
    let opt: &'static Opt = Box::leak(Box::new(Opt::from_args()));
    
    // Validation checks
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
            "Can't use auth-once with no-auth mode. Use password authentication with auth-once.",
        ));
    }
    if opt.auth_once && opt.skip_auth {
        return Err(SocksError::ArgumentInputError(
            "Can't use auth-once with skip-auth flag.",
        ));
    }

    let listener = TcpListener::bind(&opt.listen_addr).await?;
    let auth_state = AuthState::new(
        opt.whitelist_file.clone(),
        opt.save_interval,
        opt.backup_count,
    ).await.context("Failed to initialize AuthState")?;

    info!("Listen for socks connections @ {}", &opt.listen_addr);
    
    if opt.auth_once {
        if let Some(path) = &opt.whitelist_file {
            if opt.whitelist_ttl > 0 {
                info!("One-time authentication enabled - IPs whitelisted for {} seconds (persistent: {})", 
                      opt.whitelist_ttl, path.display());
            } else {
                info!("One-time authentication enabled - permanent IP whitelist (persistent: {})", 
                      path.display());
            }
        } else {
            warn!("One-time authentication without persistence - whitelist will be lost on restart!");
        }
    }

    // Spawn background tasks
    if opt.auth_once {
        // Cleanup task for expired entries
        if opt.whitelist_ttl > 0 {
            let auth_state_cleanup = auth_state.clone();
            let cleanup_interval = std::cmp::max(opt.whitelist_ttl / 4, 30);
            
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(cleanup_interval));
                loop {
                    interval.tick().await;
                    auth_state_cleanup.cleanup_expired().await;
                }
            });
            
            info!("Started whitelist cleanup task (interval: {} seconds)", cleanup_interval);
        }

        // Periodic save task
        if opt.whitelist_file.is_some() && opt.save_interval > 0 {
            let auth_state_save = auth_state.clone();
            tokio::spawn(async move {
                auth_state_save.save_periodically().await;
            });
            
            info!("Started periodic save task (interval: {} seconds)", opt.save_interval);
        }
    }

    // Graceful shutdown handler
    let auth_state_shutdown = auth_state.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
        info!("Received Ctrl+C, saving whitelist before shutdown...");
        
        if let Err(e) = auth_state_shutdown.save_to_disk().await {
            error!("Failed to save whitelist on shutdown: {}", e);
        } else {
            info!("Whitelist saved successfully");
        }
        
        std::process::exit(0);
    });

    // Log initial stats
    let stats = auth_state.get_stats().await;
    info!("Initial whitelist stats: {:?}", stats);

    // Standard TCP loop
    loop {
        match listener.accept().await {
            Ok((socket, client_addr)) => {
                let auth_state_clone = auth_state.clone();
                spawn_and_log_error(serve_socks5(opt, socket, client_addr.ip(), auth_state_clone));
            }
            Err(err) => {
                error!("accept error = {:?}", err);
            }
        }
    }
}

async fn serve_socks5(
    opt: &Opt, 
    socket: tokio::net::TcpStream, 
    client_ip: IpAddr,
    auth_state: AuthState
) -> Result<(), SocksError> {
    
    // Pre-check whitelist status for auth_once mode
    let is_whitelisted = if opt.auth_once {
        auth_state.is_authenticated(&client_ip).await
    } else {
        false
    };

    // Choose authentication method based on whitelist status
    let (proto, cmd, target_addr) = if is_whitelisted {
        // IP is whitelisted - use NO AUTH for maximum performance
        debug!("IP {} is whitelisted, using no-auth method", client_ip);
        Socks5ServerProtocol::accept_no_auth(socket).await?
    } else {
        // IP not whitelisted - use configured auth method
        match &opt.auth {
            AuthMode::NoAuth if opt.skip_auth => {
                debug!("Using skip-auth method for {}", client_ip);
                Socks5ServerProtocol::skip_auth_this_is_not_rfc_compliant(socket)
            }
            AuthMode::NoAuth => {
                debug!("Using no-auth method for {}", client_ip);
                Socks5ServerProtocol::accept_no_auth(socket).await?
            }
            AuthMode::Password { username, password } => {
                debug!("Requiring password authentication for {}", client_ip);
                
                let start_time = std::time::Instant::now();
                let (proto, auth_user) = Socks5ServerProtocol::accept_password_auth(socket, |user, pass| {
                    let auth_success = user == *username && pass == *password;
                    if auth_success {
                        debug!("Authentication successful for user: {} from IP: {}", user, client_ip);
                    } else {
                        warn!("Authentication failed for user: {} from IP: {}", user, client_ip);
                    }
                    auth_success
                }).await?;
                
                let auth_duration = start_time.elapsed();
                debug!("Authentication completed in {:?} for {}", auth_duration, client_ip);
                
                // If auth was successful and auth_once is enabled, add IP to whitelist
                if opt.auth_once {
                    let ttl = if opt.whitelist_ttl > 0 { 
                        Some(opt.whitelist_ttl) 
                    } else { 
                        None 
                    };
                    auth_state.add_authenticated_ip(client_ip, ttl, auth_user).await;
                    let count = auth_state.authenticated_count().await;
                    info!("IP {} authenticated as '{}' and whitelisted. Total active entries: {}", 
                          client_ip, auth_user, count);
                }
                
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
            debug!("Handling TCP connect for {} to {}", client_ip, target_addr);
            run_tcp_proxy(proto, &target_addr, opt.request_timeout, false).await?;
        }
        Socks5Command::UDPAssociate if opt.allow_udp => {
            debug!("Handling UDP associate for {} to {}", client_ip, target_addr);
            let reply_ip = opt.public_addr.context("invalid reply ip")?;
            run_udp_proxy(proto, &target_addr, None, reply_ip, None).await?;
        }
        _ => {
            warn!("Unsupported command from {}: {:?}", client_ip, cmd);
            proto.reply_error(&ReplyError::CommandNotSupported).await?;
            return Err(ReplyError::CommandNotSupported.into());
        }
    };
    
    debug!("Connection completed for {}", client_ip);
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
