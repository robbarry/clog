use std::fs;
use std::path::PathBuf;
use dirs::home_dir;
use sha2::{Sha256, Digest};

const APP_SALT: &str = "clog-device-2024";

pub fn get_device_id_path() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".clog")
        .join("device_id")
}

pub fn get_or_create_device_id() -> Result<String, Box<dyn std::error::Error>> {
    let path = get_device_id_path();
    
    // Check if device_id already exists
    if path.exists() {
        let id = fs::read_to_string(&path)?;
        return Ok(id.trim().to_string());
    }
    
    // Generate new device ID
    let raw_id = get_platform_id()?;
    let device_id = hash_device_id(&raw_id);
    
    // Ensure directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    
    // Write with restricted permissions
    fs::write(&path, &device_id)?;
    
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::metadata(&path)?;
        let mut perms = metadata.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(&path, perms)?;
    }
    
    Ok(device_id)
}

fn get_platform_id() -> Result<String, Box<dyn std::error::Error>> {
    #[cfg(target_os = "macos")]
    {
        // Try to get IOPlatformUUID
        use std::process::Command;
        let output = Command::new("ioreg")
            .args(&["-d2", "-c", "IOPlatformExpertDevice"])
            .output()?;
        
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if line.contains("IOPlatformUUID") {
                if let Some(uuid_part) = line.split('"').nth(3) {
                    return Ok(uuid_part.to_string());
                }
            }
        }
    }
    
    #[cfg(target_os = "linux")]
    {
        // Try /etc/machine-id first
        if let Ok(id) = fs::read_to_string("/etc/machine-id") {
            return Ok(id.trim().to_string());
        }
        
        // Fallback to /var/lib/dbus/machine-id
        if let Ok(id) = fs::read_to_string("/var/lib/dbus/machine-id") {
            return Ok(id.trim().to_string());
        }
    }
    
    // Fallback: generate a random UUID
    use ulid::Ulid;
    Ok(format!("fallback-{}", Ulid::new().to_string()))
}

fn hash_device_id(raw_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(APP_SALT.as_bytes());
    hasher.update(raw_id.as_bytes());
    let result = hasher.finalize();
    
    // Convert to base32 for readability
    base32::encode(base32::Alphabet::Rfc4648 { padding: false }, &result[..16])
}