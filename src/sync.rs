use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::time::Duration;
use std::thread;
use crate::credentials;
use crate::db::Database;

const BATCH_SIZE: usize = 100;
const MAX_RETRIES: u32 = 5;
const RETRY_BASE_DELAY_MS: u64 = 1000;

#[derive(Debug, Serialize)]
struct EventBatch {
    events: Vec<SyncEvent>,
    device_id: String,
}

#[derive(Debug, Serialize)]
struct SyncEvent {
    event_id: String,
    ppid: u32,
    name: Option<String>,
    timestamp: String,  // Use string for simpler serialization
    directory: String,
    message: String,
    session_id: String,
    repo_root: Option<String>,
    repo_branch: Option<String>,
    repo_commit: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SyncResponse {
    success: bool,
    message: Option<String>,
    synced_count: Option<usize>,
}

pub struct SyncClient {
    client: Client,
    credentials: credentials::Credentials,
    device_id: String,
}

impl SyncClient {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let creds = credentials::get_credentials()?
            .ok_or("No sync credentials configured. Run 'clog --login' first.")?;
        
        let device_id = crate::device::get_or_create_device_id()?;
        
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;
        
        Ok(SyncClient {
            client,
            credentials: creds,
            device_id,
        })
    }
    
    pub fn sync_push(&self, db: &Database, push_only: bool) -> Result<(), Box<dyn std::error::Error>> {
        if !push_only {
            return Err("Pull sync not yet implemented. Use --push-only flag.".into());
        }
        
        loop {
            // Get batch of unsynced entries
            let entries = db.get_unsynced_entries(BATCH_SIZE)?;
            
            if entries.is_empty() {
                println!("✓ All entries synced");
                break;
            }
            
            println!("Syncing {} entries...", entries.len());
            
            // Convert to sync events
            let events: Vec<SyncEvent> = entries.iter().map(|e| SyncEvent {
                event_id: e.event_id.clone().unwrap_or_default(),
                ppid: e.ppid,
                name: e.name.clone(),
                timestamp: e.timestamp.to_rfc3339(),
                directory: e.directory.clone(),
                message: e.message.clone(),
                session_id: e.session_id.clone(),
                repo_root: e.repo_root.clone(),
                repo_branch: e.repo_branch.clone(),
                repo_commit: e.repo_commit.clone(),
            }).collect();
            
            let event_ids: Vec<String> = entries.iter()
                .filter_map(|e| e.event_id.clone())
                .collect();
            
            let batch = EventBatch {
                events,
                device_id: self.device_id.clone(),
            };
            
            // Send with retries
            let result = self.send_batch_with_retry(&batch)?;
            
            if result.success {
                // Mark entries as synced
                db.mark_entries_synced(&event_ids)?;
                
                // Update sync state watermark
                db.update_sync_watermark(&self.credentials.server_url)?;
                
                println!("✓ Synced {} entries", result.synced_count.unwrap_or(event_ids.len()));
            } else {
                return Err(format!("Sync failed: {}", 
                    result.message.unwrap_or_else(|| "Unknown error".to_string())).into());
            }
        }
        
        Ok(())
    }
    
    fn send_batch_with_retry(&self, batch: &EventBatch) -> Result<SyncResponse, Box<dyn std::error::Error>> {
        let url = format!("{}/v1/events/batch", self.credentials.server_url);
        
        let mut retry_count = 0;
        let mut delay_ms = RETRY_BASE_DELAY_MS;
        
        loop {
            match self.client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.credentials.token))
                .json(&batch)
                .send()
            {
                Ok(response) => {
                    if response.status().is_success() {
                        return response.json::<SyncResponse>()
                            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>);
                    }
                    
                    if response.status().is_client_error() {
                        // Client errors are not retryable
                        let status = response.status();
                        let text = response.text().unwrap_or_else(|_| "No response body".to_string());
                        return Err(format!("Client error {}: {}", status, text).into());
                    }
                    
                    // Server error - retry
                    if retry_count >= MAX_RETRIES {
                        return Err(format!("Server error after {} retries: {}", 
                            MAX_RETRIES, response.status()).into());
                    }
                }
                Err(e) => {
                    // Network error - retry
                    if retry_count >= MAX_RETRIES {
                        return Err(format!("Network error after {} retries: {}", 
                            MAX_RETRIES, e).into());
                    }
                }
            }
            
            // Exponential backoff
            thread::sleep(Duration::from_millis(delay_ms));
            delay_ms = (delay_ms * 2).min(30000); // Cap at 30 seconds
            retry_count += 1;
            
            println!("Retry {} of {}...", retry_count, MAX_RETRIES);
        }
    }
}