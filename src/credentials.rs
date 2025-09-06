use std::env;
use std::fs;
use std::path::PathBuf;
use dirs::home_dir;
use keyring::Entry;
use serde::{Deserialize, Serialize};

const SERVICE_NAME: &str = "clog-sync";
const CONFIG_FILE: &str = ".clog/config.json";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Credentials {
    pub server_url: String,
    pub token: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    server_url: Option<String>,
}

pub fn get_credentials() -> Result<Option<Credentials>, Box<dyn std::error::Error>> {
    // 1. Check environment variables first
    if let (Ok(token), Ok(url)) = (env::var("CLOG_TOKEN"), env::var("CLOG_SERVER_URL")) {
        return Ok(Some(Credentials {
            server_url: url,
            token,
        }));
    }
    
    // 2. Try OS keychain
    if let Ok(Some(creds)) = get_keychain_credentials() {
        return Ok(Some(creds));
    }
    
    // 3. Try config file (with warning)
    if let Ok(Some(creds)) = get_config_credentials() {
        eprintln!("Warning: Using credentials from config file (less secure than keychain)");
        return Ok(Some(creds));
    }
    
    Ok(None)
}

fn get_keychain_credentials() -> Result<Option<Credentials>, Box<dyn std::error::Error>> {
    // Get server URL from config file or keychain metadata
    let config = read_config()?;
    let server_url = match config.server_url {
        Some(url) => url,
        None => return Ok(None),
    };
    
    // Get token from keychain
    let entry = Entry::new(SERVICE_NAME, &server_url)?;
    match entry.get_password() {
        Ok(token) => Ok(Some(Credentials { server_url, token })),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(Box::new(e)),
    }
}

fn get_config_credentials() -> Result<Option<Credentials>, Box<dyn std::error::Error>> {
    let config_path = get_config_path();
    if !config_path.exists() {
        return Ok(None);
    }
    
    let content = fs::read_to_string(&config_path)?;
    let config: serde_json::Value = serde_json::from_str(&content)?;
    
    if let (Some(url), Some(token)) = (
        config.get("server_url").and_then(|v| v.as_str()),
        config.get("token").and_then(|v| v.as_str()),
    ) {
        Ok(Some(Credentials {
            server_url: url.to_string(),
            token: token.to_string(),
        }))
    } else {
        Ok(None)
    }
}

pub fn save_credentials(creds: &Credentials) -> Result<(), Box<dyn std::error::Error>> {
    // Save server URL to config
    save_config(&Config {
        server_url: Some(creds.server_url.clone()),
    })?;
    
    // Try to save token to keychain
    let entry = Entry::new(SERVICE_NAME, &creds.server_url)?;
    match entry.set_password(&creds.token) {
        Ok(_) => {
            println!("✓ Credentials saved to keychain");
            Ok(())
        }
        Err(e) => {
            // Fallback: save to config file with warning
            eprintln!("Warning: Could not save to keychain ({}), using config file", e);
            save_config_with_token(creds)?;
            println!("✓ Credentials saved to config file (less secure)");
            
            // Set restrictive permissions on config file
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let metadata = fs::metadata(&get_config_path())?;
                let mut perms = metadata.permissions();
                perms.set_mode(0o600);
                fs::set_permissions(&get_config_path(), perms)?;
            }
            
            Ok(())
        }
    }
}

pub fn delete_credentials() -> Result<(), Box<dyn std::error::Error>> {
    // Get server URL from config
    let config = read_config()?;
    
    // Remove from keychain if exists
    if let Some(server_url) = config.server_url {
        let entry = Entry::new(SERVICE_NAME, &server_url)?;
        match entry.delete_credential() {
            Ok(_) | Err(keyring::Error::NoEntry) => {}
            Err(e) => eprintln!("Warning: Could not remove from keychain: {}", e),
        }
    }
    
    // Remove config file
    let config_path = get_config_path();
    if config_path.exists() {
        fs::remove_file(&config_path)?;
    }
    
    println!("✓ Credentials removed");
    Ok(())
}

fn get_config_path() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(CONFIG_FILE)
}

fn read_config() -> Result<Config, Box<dyn std::error::Error>> {
    let config_path = get_config_path();
    if !config_path.exists() {
        return Ok(Config { server_url: None });
    }
    
    let content = fs::read_to_string(&config_path)?;
    let config: Config = serde_json::from_str(&content)?;
    Ok(config)
}

fn save_config(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    let config_path = get_config_path();
    
    // Ensure directory exists
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }
    
    let content = serde_json::to_string_pretty(config)?;
    fs::write(&config_path, content)?;
    Ok(())
}

fn save_config_with_token(creds: &Credentials) -> Result<(), Box<dyn std::error::Error>> {
    let config_path = get_config_path();
    
    // Ensure directory exists
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }
    
    let config = serde_json::json!({
        "server_url": creds.server_url,
        "token": creds.token,
    });
    
    let content = serde_json::to_string_pretty(&config)?;
    fs::write(&config_path, content)?;
    Ok(())
}