-- Create enum for position status
CREATE TYPE position_status AS ENUM ('active', 'unstaking', 'withdrawn');

-- Create positions table (stores all positions)
CREATE TABLE IF NOT EXISTS positions (
    user_address VARCHAR(42) NOT NULL,
    nonce BIGINT NOT NULL,
    amount NUMERIC(78, 0) NOT NULL, -- Store as wei (can hold up to 78 digits)
    deposit_timestamp BIGINT NOT NULL,
    status position_status NOT NULL,
    withdrawal_initiated_timestamp BIGINT,
    block_number BIGINT NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (user_address, nonce)
);

-- Create indexes for efficient querying
CREATE INDEX idx_positions_status ON positions(status);
CREATE INDEX idx_positions_user ON positions(user_address);
CREATE INDEX idx_positions_block ON positions(block_number);

-- Create metadata table for tracking sync state
CREATE TABLE IF NOT EXISTS sync_metadata (
    key VARCHAR(50) PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Insert default last block
INSERT INTO sync_metadata (key, value) VALUES ('last_processed_block', '0') 
ON CONFLICT (key) DO NOTHING;

-- Create events table for audit trail
CREATE TABLE IF NOT EXISTS events (
    id SERIAL PRIMARY KEY,
    event_type VARCHAR(50) NOT NULL,
    user_address VARCHAR(42) NOT NULL,
    nonce BIGINT,
    amount NUMERIC(78, 0),
    block_number BIGINT NOT NULL,
    transaction_hash VARCHAR(66) NOT NULL,
    timestamp BIGINT NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_events_user ON events(user_address);
CREATE INDEX idx_events_block ON events(block_number);
CREATE INDEX idx_events_type ON events(event_type);
