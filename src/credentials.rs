use std::env;
use std::fs;
use std::path::PathBuf;
use dirs::home_dir;

const CONFIG_FILE: &str = ".clog/config.json";

#[derive(Debug, Clone)]
pub struct Credentials {
    pub database_url: String,
}

pub fn get_credentials() -> Result<Option<Credentials>, Box<dyn std::error::Error>> {
    // 1. Check environment variable first
    if let Ok(database_url) = env::var("DATABASE_URL") {
        return Ok(Some(Credentials { database_url }));
    }
    
    // 2. Try .env file in current directory
    dotenv::dotenv().ok();
    if let Ok(database_url) = env::var("DATABASE_URL") {
        return Ok(Some(Credentials { database_url }));
    }
    
    // 3. Try config file in home directory
    let config_path = get_config_path();
    if config_path.exists() {
        let content = fs::read_to_string(&config_path)?;
        let config: serde_json::Value = serde_json::from_str(&content)?;
        
        if let Some(database_url) = config.get("database_url").and_then(|v| v.as_str()) {
            return Ok(Some(Credentials {
                database_url: database_url.to_string(),
            }));
        }
    }
    
    Ok(None)
}

pub fn save_credentials(creds: &Credentials) -> Result<(), Box<dyn std::error::Error>> {
    let config_path = get_config_path();
    
    // Ensure directory exists
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }
    
    let config = serde_json::json!({
        "database_url": creds.database_url,
    });
    
    let content = serde_json::to_string_pretty(&config)?;
    fs::write(&config_path, content)?;
    
    // Set restrictive permissions on config file
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::metadata(&config_path)?;
        let mut perms = metadata.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(&config_path, perms)?;
    }
    
    println!("✓ Database credentials saved to config file");
    Ok(())
}

pub fn delete_credentials() -> Result<(), Box<dyn std::error::Error>> {
    // Remove config file
    let config_path = get_config_path();
    if config_path.exists() {
        fs::remove_file(&config_path)?;
        println!("✓ Database credentials removed");
    } else {
        println!("No credentials found to remove");
    }
    
    Ok(())
}

fn get_config_path() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(CONFIG_FILE)
}