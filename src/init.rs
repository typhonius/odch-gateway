//! `odch-gateway init` — one-command setup for the entire stack.
//!
//! Creates PostgreSQL database, generates secrets, writes configs,
//! optionally creates systemd services.

use std::fs;
use std::path::Path;
use std::process::Command;

/// Generate a random alphanumeric string using /dev/urandom.
fn random_secret(len: usize) -> String {
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    (0..len)
        .map(|_| {
            let mut buf = [0u8; 1];
            std::io::Read::read_exact(&mut std::fs::File::open("/dev/urandom").unwrap(), &mut buf)
                .unwrap();
            CHARSET[(buf[0] as usize) % CHARSET.len()] as char
        })
        .collect()
}

/// Generate a bcrypt hash for the admin UI password.
fn bcrypt_hash(password: &str) -> String {
    bcrypt::hash(password, 10).expect("bcrypt hash failed")
}

pub struct InitConfig<'a> {
    pub db_host: &'a str,
    pub db_port: u16,
    pub db_name: &'a str,
    pub db_user: &'a str,
    pub db_password: &'a str,
    pub hub_port: u16,
    pub api_port: u16,
    pub admin_ui_port: u16,
    pub config_dir: &'a str,
    pub install_services: bool,
}

pub fn run_init(cfg: InitConfig) -> Result<(), Box<dyn std::error::Error>> {
    let InitConfig {
        db_host,
        db_port,
        db_name,
        db_user,
        db_password,
        hub_port,
        api_port,
        admin_ui_port,
        config_dir,
        install_services,
    } = cfg;
    println!("odch-gateway init v{}", env!("CARGO_PKG_VERSION"));
    println!();

    // 1. Create config directory
    let config_path = Path::new(config_dir);
    fs::create_dir_all(config_path)?;
    println!("  Config directory: {}", config_dir);

    // 2. Try to create database
    let db_url = format!(
        "postgres://{}:{}@{}:{}/{}",
        db_user, db_password, db_host, db_port, db_name
    );
    println!("  Database: {}", db_name);

    // Try createdb (won't fail if DB already exists)
    let createdb_result = Command::new("createdb")
        .args([
            "-h",
            db_host,
            "-p",
            &db_port.to_string(),
            "-U",
            db_user,
            db_name,
        ])
        .env("PGPASSWORD", db_password)
        .output();

    match createdb_result {
        Ok(output) if output.status.success() => println!("  Created database '{}'", db_name),
        Ok(_) => println!("  Database '{}' already exists (OK)", db_name),
        Err(_) => println!("  Could not run createdb — create the database manually"),
    }

    // 3. Generate secrets
    let hub_secret = random_secret(48);
    let api_key = random_secret(48);
    let jwt_secret = random_secret(48);
    let admin_password = random_secret(16);
    let admin_password_hash = bcrypt_hash(&admin_password);
    let hub_socket_path = format!("{}/gateway.sock", config_dir);

    // 4. Write gateway config
    let gateway_config = format!(
        r#"[server]
bind_address = "0.0.0.0:{api_port}"
cors_origins = []

[hub]
socket_path = "{hub_socket_path}"
secret = "{hub_secret}"

[database]
url = "{db_url}"

[auth]
api_keys = ["{api_key}"]

[webhook]
max_retries = 3
retry_delay_secs = 5
timeout_secs = 10
max_webhooks = 50
storage_path = "{config_dir}/webhooks.json"

[rate_limit]
requests_per_minute = 10

[admin_ui]
bind_address = "127.0.0.1:{admin_ui_port}"
username = "admin"
password_hash = "{admin_password_hash}"
session_expiry_hours = 8
jwt_secret = "{jwt_secret}"
"#
    );

    let gateway_config_path = format!("{}/gateway.toml", config_dir);
    fs::write(&gateway_config_path, &gateway_config)?;
    println!("  Wrote {}", gateway_config_path);

    // 5. Write hub config
    let hub_config = format!(
        r#"hub_name = "OpenDCHub"
max_users = 500
listening_port = {hub_port}
json_socket_path = "{hub_socket_path}"
json_socket_secret = "{hub_secret}"
registered_only = 0
check_key = 1
hublist_upload = 0
min_share = 0
default_pass = ""
link_pass = ""
users_per_fork = 1000
"#
    );

    let hub_config_path = format!("{}/hub.conf", config_dir);
    fs::write(&hub_config_path, &hub_config)?;
    println!("  Wrote {}", hub_config_path);

    // 6. Optionally create systemd services
    if install_services {
        let hub_service = format!(
            r#"[Unit]
Description=OpenDCHub NMDC Server
After=network.target

[Service]
Type=simple
User=opendchub
ExecStart=/usr/local/bin/opendchub
WorkingDirectory={config_dir}
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
"#
        );

        let gateway_service = format!(
            r#"[Unit]
Description=ODCHub Gateway
After=network.target postgresql.service
Requires=postgresql.service

[Service]
Type=simple
User=opendchub
ExecStart=/usr/local/bin/odch-gateway {gateway_config_path}
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
"#
        );

        let hub_svc_path = "/etc/systemd/system/opendchub.service";
        let gw_svc_path = "/etc/systemd/system/odch-gateway.service";

        match fs::write(hub_svc_path, &hub_service) {
            Ok(_) => println!("  Wrote {}", hub_svc_path),
            Err(e) => println!("  Could not write {} (run as root?): {}", hub_svc_path, e),
        }
        match fs::write(gw_svc_path, &gateway_service) {
            Ok(_) => println!("  Wrote {}", gw_svc_path),
            Err(e) => println!("  Could not write {} (run as root?): {}", gw_svc_path, e),
        }
    }

    // 7. Print summary
    println!();
    println!("  ==========================================");
    println!("  Setup complete!");
    println!("  ==========================================");
    println!();
    println!("  DC port:     {}", hub_port);
    println!("  API port:    {}", api_port);
    println!("  Admin UI:    http://127.0.0.1:{}", admin_ui_port);
    println!("  Admin user:  admin");
    println!("  Admin pass:  {}", admin_password);
    println!("  API key:     {}", api_key);
    println!();
    println!("  Start with:");
    if install_services {
        println!("    sudo systemctl daemon-reload");
        println!("    sudo systemctl enable --now opendchub odch-gateway");
    } else {
        println!("    opendchub &");
        println!("    odch-gateway {}", gateway_config_path);
    }
    println!();
    println!("  Save the admin password above — it won't be shown again.");

    Ok(())
}
