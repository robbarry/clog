-- Postgres schema for clog sync
-- This schema stores log entries from all devices

-- Create devices table to track registered devices
CREATE TABLE IF NOT EXISTS devices (
    device_id TEXT PRIMARY KEY,
    device_name TEXT,
    first_seen TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_seen TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Create main log_entries table
CREATE TABLE IF NOT EXISTS log_entries (
    event_id TEXT PRIMARY KEY,  -- ULID from client
    device_id TEXT NOT NULL REFERENCES devices(device_id),
    ppid INTEGER NOT NULL,
    name TEXT,
    timestamp TIMESTAMPTZ NOT NULL,
    directory TEXT NOT NULL,
    message TEXT NOT NULL,
    session_id TEXT NOT NULL,
    repo_root TEXT,
    repo_branch TEXT,
    repo_commit TEXT,
    received_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT unique_event_per_device UNIQUE (event_id, device_id)
);

-- Indexes for efficient querying
CREATE INDEX IF NOT EXISTS idx_log_entries_device_timestamp 
    ON log_entries(device_id, timestamp DESC);

CREATE INDEX IF NOT EXISTS idx_log_entries_timestamp 
    ON log_entries(timestamp DESC);

CREATE INDEX IF NOT EXISTS idx_log_entries_session 
    ON log_entries(session_id);

CREATE INDEX IF NOT EXISTS idx_log_entries_repo 
    ON log_entries(repo_root, timestamp DESC);

CREATE INDEX IF NOT EXISTS idx_log_entries_name 
    ON log_entries(name, timestamp DESC);

-- Create sync_state table to track what each device has synced
CREATE TABLE IF NOT EXISTS sync_state (
    device_id TEXT NOT NULL REFERENCES devices(device_id),
    last_event_id TEXT,
    last_sync_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (device_id)
);

-- Function to auto-register new devices
CREATE OR REPLACE FUNCTION auto_register_device()
RETURNS TRIGGER AS $$
BEGIN
    INSERT INTO devices (device_id)
    VALUES (NEW.device_id)
    ON CONFLICT (device_id) 
    DO UPDATE SET last_seen = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Trigger to auto-register devices on first log entry
CREATE TRIGGER register_device_on_insert
    BEFORE INSERT ON log_entries
    FOR EACH ROW
    EXECUTE FUNCTION auto_register_device();