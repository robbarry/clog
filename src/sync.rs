use tokio_postgres::{NoTls, Client};
use postgres_native_tls::MakeTlsConnector;
use native_tls::TlsConnector;
use crate::db::Database;
use crate::credentials;

const BATCH_SIZE: usize = 100;

pub struct SyncClient {
    device_id: String,
}

impl SyncClient {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        // Load .env file
        dotenv::dotenv().ok();
        
        let device_id = crate::device::get_or_create_device_id()?;
        
        Ok(SyncClient {
            device_id,
        })
    }
    
    pub fn sync_push(&self, db: &Database, push_only: bool) -> Result<(), Box<dyn std::error::Error>> {
        if !push_only {
            return Err("Pull sync not yet implemented. Use --push-only flag.".into());
        }
        
        // Run async sync in blocking context
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(self.async_sync_push(db))
    }
    
    async fn async_sync_push(&self, db: &Database) -> Result<(), Box<dyn std::error::Error>> {
        // Resolve database URL from env/.env/config
        let database_url = credentials::get_credentials()?
            .map(|c| c.database_url)
            .ok_or("No database credentials configured. Run 'clog --login' or set DATABASE_URL")?;
        
        // Connect to Postgres with SSL if required
        let client = if database_url.contains("sslmode=require") {
            let connector = TlsConnector::builder()
                .danger_accept_invalid_certs(true) // For Digital Ocean self-signed certs
                .build()?;
            let connector = MakeTlsConnector::new(connector);
            
            let (client, connection) = tokio_postgres::connect(&database_url, connector).await?;
            
            // Spawn connection handler
            tokio::spawn(async move {
                if let Err(e) = connection.await {
                    eprintln!("Postgres connection error: {}", e);
                }
            });
            
            client
        } else {
            let (client, connection) = tokio_postgres::connect(&database_url, NoTls).await?;
            
            tokio::spawn(async move {
                if let Err(e) = connection.await {
                    eprintln!("Postgres connection error: {}", e);
                }
            });
            
            client
        };
        
        // Create schema if needed
        self.ensure_schema(&client).await?;
        
        // Sync in batches
        loop {
            let entries = db.get_unsynced_entries(BATCH_SIZE)?;
            
            if entries.is_empty() {
                println!("✓ All entries synced");
                break;
            }
            
            println!("Syncing {} entries...", entries.len());
            
            let mut synced_ids = Vec::new();
            
            for entry in &entries {
                let event_id = entry.event_id.as_ref()
                    .ok_or("Entry missing event_id")?;
                
                // Insert into Postgres (upsert to handle duplicates)
                let result = client.execute(
                    "INSERT INTO log_entries (
                        event_id, device_id, ppid, name, timestamp, 
                        directory, message, session_id, 
                        repo_root, repo_branch, repo_commit
                    ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
                    ON CONFLICT (event_id) DO NOTHING",
                    &[
                        &event_id,
                        &self.device_id,
                        &(entry.ppid as i32),
                        &entry.name,
                        &entry.timestamp,
                        &entry.directory,
                        &entry.message,
                        &entry.session_id,
                        &entry.repo_root,
                        &entry.repo_branch,
                        &entry.repo_commit,
                    ]
                ).await;
                
                match result {
                    Ok(_) => {
                        synced_ids.push(event_id.clone());
                    }
                    Err(e) => {
                        eprintln!("Failed to sync entry {}: {}", event_id, e);
                        // Continue with other entries
                    }
                }
            }
            
            // Mark successfully synced entries in local DB
            if !synced_ids.is_empty() {
                db.mark_entries_synced(&synced_ids)?;
                println!("✓ Synced {} entries", synced_ids.len());
            }
            
            // Update sync state in Postgres
            client.execute(
                "INSERT INTO sync_state (device_id, last_sync_at) 
                 VALUES ($1, CURRENT_TIMESTAMP)
                 ON CONFLICT (device_id) 
                 DO UPDATE SET last_sync_at = CURRENT_TIMESTAMP",
                &[&self.device_id]
            ).await?;
        }
        
        Ok(())
    }
    
    async fn ensure_schema(&self, client: &Client) -> Result<(), Box<dyn std::error::Error>> {
        // Check if tables exist, create if needed
        let table_exists = client.query_one(
            "SELECT EXISTS (
                SELECT FROM information_schema.tables 
                WHERE table_schema = 'public' 
                AND table_name = 'log_entries'
            )",
            &[]
        ).await?;
        
        let exists: bool = table_exists.get(0);
        
        if !exists {
            println!("Creating database schema...");
            
            // Read schema.sql file
            let schema = std::fs::read_to_string("schema.sql")
                .unwrap_or_else(|_| include_str!("../schema.sql").to_string());
            
            // Execute schema
            client.batch_execute(&schema).await?;
            
            println!("✓ Database schema created");
        }
        
        Ok(())
    }
}
