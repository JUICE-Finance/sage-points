use alloy::{
    primitives::{Address, U256},
    providers::{Provider, ProviderBuilder},
    rpc::types::{Filter, Log},
    sol,
    sol_types::SolEvent,
};
use eyre::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::sleep;

mod db;
mod api;
use db::{Database, EventData};

// Define the contract events using the sol! macro
sol!(
    #[sol(rpc)]
    contract SageStaking {
        event Deposit(address indexed user, uint256 amount, uint256 nonce, uint256 timestamp);
        event InitiateWithdraw(address indexed user, uint256 nonce, uint256 unlocksAt, uint256 timestamp);
        event Withdraw(address indexed user, uint256 amount, uint256 nonce, uint256 timestamp);
        event RestakeFromWithdrawalInitiated(address indexed user, uint256 nonce, uint256 amount, uint256 timestamp);
    }
);

// Maximum blocks to fetch in one request (to avoid RPC limits)
const MAX_BLOCK_RANGE: u64 = 500; // Reduced to avoid rate limits

// Position status for tracking
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PositionStatus {
    Active,
    Unstaking,  // Withdrawal initiated, waiting for cooldown
    Withdrawn,
}

// Structure to track a staking position
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub user: Address,
    pub nonce: u64,
    pub amount: U256, // Amount in wei
    pub deposit_timestamp: u64,
    pub status: PositionStatus,
    pub withdrawal_initiated_timestamp: Option<u64>,
    pub block_number: u64, // Track the block when position was created
}

// Points breakdown
#[derive(Debug, Clone, Default)]
struct PointsBreakdown {
    sage_points: f64,
    formation_points: f64,
}

// Global state to track all positions
struct PointsTracker {
    // Separate tracking for different position states for efficiency
    active_positions: HashMap<(Address, u64), Position>,     // Currently earning points
    unstaking_positions: HashMap<(Address, u64), Position>,  // Withdrawal initiated, not earning
    withdrawn_positions: HashMap<(Address, u64), Position>,  // Fully withdrawn
    total_events_processed: usize,
    current_block: u64,
    db: Option<Database>,  // Database connection for persistence
}

impl PointsTracker {
    async fn with_database_instance(db: Database) -> Result<Self> {
        // Load existing positions from database
        let (active, unstaking, withdrawn) = db.load_positions().await?;
        
        let tracker = Self {
            active_positions: active.into_iter().collect(),
            unstaking_positions: unstaking.into_iter().collect(),
            withdrawn_positions: withdrawn.into_iter().collect(),
            total_events_processed: 0,
            current_block: 0,
            db: Some(db),
        };
        
        Ok(tracker)
    }

    // Get a position from any of the maps
    fn get_position(&self, key: &(Address, u64)) -> Option<&Position> {
        self.active_positions.get(key)
            .or_else(|| self.unstaking_positions.get(key))
            .or_else(|| self.withdrawn_positions.get(key))
    }

    // Move position between states
    async fn move_to_unstaking(&mut self, key: (Address, u64), timestamp: u64) {
        if let Some(mut position) = self.active_positions.remove(&key) {
            position.status = PositionStatus::Unstaking;
            position.withdrawal_initiated_timestamp = Some(timestamp);
            
            // Save to database
            if let Some(db) = &self.db {
                if let Err(e) = db.save_position(&position).await {
                    eprintln!("⚠️  Failed to save position to database: {}", e);
                }
            }
            
            self.unstaking_positions.insert(key, position);
        }
    }

    async fn move_to_withdrawn(&mut self, key: (Address, u64)) {
        if let Some(mut position) = self.unstaking_positions.remove(&key) {
            position.status = PositionStatus::Withdrawn;
            
            // Save to database
            if let Some(db) = &self.db {
                if let Err(e) = db.save_position(&position).await {
                    eprintln!("⚠️  Failed to save position to database: {}", e);
                }
            }
            
            self.withdrawn_positions.insert(key, position);
        }
    }

    async fn move_to_active(&mut self, key: (Address, u64), new_deposit_timestamp: u64) {
        if let Some(mut position) = self.unstaking_positions.remove(&key) {
            position.status = PositionStatus::Active;
            position.withdrawal_initiated_timestamp = None;
            position.deposit_timestamp = new_deposit_timestamp;
            
            // Save to database
            if let Some(db) = &self.db {
                if let Err(e) = db.save_position(&position).await {
                    eprintln!("⚠️  Failed to save position to database: {}", e);
                }
            }
            
            self.active_positions.insert(key, position);
        }
    }
    
    async fn add_active_position(&mut self, key: (Address, u64), position: Position) {
        // Save to database
        if let Some(db) = &self.db {
            if let Err(e) = db.save_position(&position).await {
                eprintln!("⚠️  Failed to save position to database: {}", e);
            }
        }
        
        self.active_positions.insert(key, position);
    }

    // Calculate points for a position with both SAGE and Formation points
    fn calculate_position_points(&self, position: &Position) -> PointsBreakdown {
        let end_timestamp = if let Some(withdrawal_ts) = position.withdrawal_initiated_timestamp {
            // For unstaking/withdrawn positions, points stopped at withdrawal initiation
            withdrawal_ts
        } else if matches!(position.status, PositionStatus::Active) {
            // Still active, calculate until now
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
        } else {
            // Shouldn't happen, but use deposit timestamp as fallback
            position.deposit_timestamp
        };

        let seconds_staked = end_timestamp.saturating_sub(position.deposit_timestamp);
        let days_staked = seconds_staked as f64 / 86400.0; // 86400 seconds in a day
        
        // Convert amount from wei to tokens (18 decimals)
        let tokens = format_token_amount_as_float(position.amount);
        
        // 0.01 SAGE points per token per day
        // 0.0025 Formation points per token per day
        PointsBreakdown {
            sage_points: tokens * days_staked * 0.01,
            formation_points: tokens * days_staked * 0.005,
        }
    }

    // Calculate total points for a user
    fn calculate_user_points(&self, user: &Address) -> PointsBreakdown {
        let mut total = PointsBreakdown::default();
        
        // Points from active positions (still earning)
        for position in self.active_positions.values().filter(|p| p.user == *user) {
            let points = self.calculate_position_points(position);
            total.sage_points += points.sage_points;
            total.formation_points += points.formation_points;
        }
        
        // Points from unstaking positions (earned until withdrawal initiated)
        for position in self.unstaking_positions.values().filter(|p| p.user == *user) {
            let points = self.calculate_position_points(position);
            total.sage_points += points.sage_points;
            total.formation_points += points.formation_points;
        }
        
        // Points from withdrawn positions (earned until withdrawal initiated)
        for position in self.withdrawn_positions.values().filter(|p| p.user == *user) {
            let points = self.calculate_position_points(position);
            total.sage_points += points.sage_points;
            total.formation_points += points.formation_points;
        }
        
        total
    }

    // Get user deposit summary
    fn get_user_deposits_summary(&self, user: &Address) -> (f64, f64, f64) {
        let mut active_amount = 0.0;
        let mut unstaking_amount = 0.0;
        let mut withdrawn_amount = 0.0;
        
        // Sum active positions
        for position in self.active_positions.values().filter(|p| p.user == *user) {
            active_amount += format_token_amount_as_float(position.amount);
        }
        
        // Sum unstaking positions
        for position in self.unstaking_positions.values().filter(|p| p.user == *user) {
            unstaking_amount += format_token_amount_as_float(position.amount);
        }
        
        // Sum withdrawn positions
        for position in self.withdrawn_positions.values().filter(|p| p.user == *user) {
            withdrawn_amount += format_token_amount_as_float(position.amount);
        }
        
        (active_amount, unstaking_amount, withdrawn_amount)
    }

    // Get points leaderboard
    fn get_leaderboard(&self) -> Vec<(Address, PointsBreakdown)> {
        let mut user_points: HashMap<Address, PointsBreakdown> = HashMap::new();
        
        // Calculate points for all positions
        for position in self.active_positions.values() {
            let points = self.calculate_position_points(position);
            let entry = user_points.entry(position.user).or_default();
            entry.sage_points += points.sage_points;
            entry.formation_points += points.formation_points;
        }
        
        for position in self.unstaking_positions.values() {
            let points = self.calculate_position_points(position);
            let entry = user_points.entry(position.user).or_default();
            entry.sage_points += points.sage_points;
            entry.formation_points += points.formation_points;
        }
        
        for position in self.withdrawn_positions.values() {
            let points = self.calculate_position_points(position);
            let entry = user_points.entry(position.user).or_default();
            entry.sage_points += points.sage_points;
            entry.formation_points += points.formation_points;
        }
        
        let mut leaderboard: Vec<(Address, PointsBreakdown)> = user_points.into_iter().collect();
        leaderboard.sort_by(|a, b| {
            // Sort by total points (sage + formation)
            let total_a = a.1.sage_points + a.1.formation_points;
            let total_b = b.1.sage_points + b.1.formation_points;
            total_b.partial_cmp(&total_a).unwrap()
        });
        leaderboard
    }

    // Display current points status
    fn display_points_summary(&self) {
        println!("\n📊 POINTS SUMMARY | Block: {}", self.current_block);
        println!("{}", "=".repeat(100));
        
        let leaderboard = self.get_leaderboard();
        
        if leaderboard.is_empty() {
            println!("No positions tracked yet.");
        } else {
            println!("Top Users by Points:\n");
            println!("  {:4} {:16} {:>12} {:>12} {:>12} | {:>10} {:>10} {:>10}", 
                "Rank", "Address", "SAGE Points", "FORM Points", "Total", "Active", "Unstaking", "Withdrawn");
            println!("  {}", "-".repeat(95));
            
            for (i, (user, points)) in leaderboard.iter().take(10).enumerate() {
                let (active, unstaking, withdrawn) = self.get_user_deposits_summary(user);
                let total_points = points.sage_points + points.formation_points;
                
                println!("  #{:3} {} {:>12.4} {:>12.4} {:>12.4} | {:>10.2} {:>10.2} {:>10.2}", 
                    i + 1, 
                    format_address(*user),
                    points.sage_points,
                    points.formation_points,
                    total_points,
                    active,
                    unstaking,
                    withdrawn
                );
            }
            
            let total_sage: f64 = leaderboard.iter().map(|(_, p)| p.sage_points).sum();
            let total_formation: f64 = leaderboard.iter().map(|(_, p)| p.formation_points).sum();
            let total_positions = self.active_positions.len() + self.unstaking_positions.len() + self.withdrawn_positions.len();
            
            println!("\n📈 Global Statistics:");
            println!("  Total SAGE Points: {:.4}", total_sage);
            println!("  Total Formation Points: {:.4}", total_formation);
            println!("  Total Positions: {} (Active: {}, Unstaking: {}, Withdrawn: {})", 
                total_positions, 
                self.active_positions.len(),
                self.unstaking_positions.len(),
                self.withdrawn_positions.len());
            println!("  Total Events Processed: {}", self.total_events_processed);
        }
        
        println!("{}\n", "=".repeat(100));
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logger
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));
    
    // Load environment variables
    dotenv::dotenv().ok();
    
    println!("🚀 Starting Points Calculator Service...");
    
    // Get configuration from environment
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set");
    let base_rpc_url = std::env::var("BASE_RPC_URL")
        .expect("BASE_RPC_URL must be set");
    let contract_address_str = std::env::var("CONTRACT_ADDRESS")
        .expect("CONTRACT_ADDRESS must be set");
    let deployment_block = std::env::var("DEPLOYMENT_BLOCK")
        .expect("DEPLOYMENT_BLOCK must be set")
        .parse::<u64>()
        .expect("DEPLOYMENT_BLOCK must be a valid u64");
    let api_port = std::env::var("PORT")
        .unwrap_or_else(|_| "3000".to_string())
        .parse::<u16>()
        .unwrap_or(3000);

    // Initialize database connection
    let db = Database::new(&database_url).await?;
    
    // Clone database for monitoring task
    let monitor_db = db.clone();
    
    // Spawn monitoring task in the background
    tokio::spawn(async move {
        if let Err(e) = run_monitoring(monitor_db, base_rpc_url, contract_address_str, deployment_block).await {
            eprintln!("❌ Monitoring task error: {}", e);
        }
    });
    
    // Run API server on main task
    api::run_api_server(db, api_port).await?;
    
    Ok(())
}

// Extract monitoring logic into a separate function
async fn run_monitoring(
    db: Database,
    base_rpc_url: String,
    contract_address_str: String, 
    deployment_block: u64
) -> Result<()> {
    // Initialize points tracker with database
    let mut tracker = PointsTracker::with_database_instance(db).await?;

    // Parse the contract address
    let contract_address = Address::from_str(&contract_address_str)?;

    // Create HTTP provider
    let provider = ProviderBuilder::new().on_http(base_rpc_url.parse()?);

    // Get the current block number
    let current_block = provider.get_block_number().await?;

    // Load the last processed block from database or use deployment block
    let mut last_block = if let Some(db) = &tracker.db {
        let db_block = db.get_last_processed_block().await?;
        
        // Use the database block if it's valid, otherwise start from deployment
        db_block.filter(|&b| b >= deployment_block).unwrap_or(deployment_block)
    } else {
        deployment_block
    };
    
    // Fetch historical events first
    if last_block < current_block {
        let blocks_to_sync = current_block - last_block;
        println!("⏳ Syncing {} blocks ({} → {})...", blocks_to_sync, last_block, current_block);
        
        let mut from_block = last_block;
        let mut events_count = 0;
        let mut blocks_processed = 0;
        
        while from_block < current_block {
            // Calculate the range for this batch
            let to_block = (from_block + MAX_BLOCK_RANGE).min(current_block);
            
            // Show progress every 10 batches (5000 blocks)
            if blocks_processed % 5000 == 0 {
                println!("📊 Progress: Processed {} blocks, found {} events so far...", blocks_processed, events_count);
            }
            
            // Create a filter for events in this range
            let filter = Filter::new()
                .address(contract_address)
                .from_block(from_block)
                .to_block(to_block);

            // Get logs with retry on rate limit
            let mut retry_count = 0;
            loop {
                match provider.get_logs(&filter).await {
                    Ok(logs) => {
                        if !logs.is_empty() {
                            println!("   ✨ Found {} events in this range", logs.len());
                        }
                        events_count += logs.len();
                        blocks_processed += to_block - from_block + 1;
                        
                        // Update tracker's current block
                        tracker.current_block = to_block;
                        
                        for log in logs {
                            handle_log(log, &mut tracker).await?;
                        }
                        
                        // Update and save progress to database
                        last_block = to_block;
                        
                        if let Some(db) = &tracker.db {
                            if let Err(e) = db.update_last_processed_block(last_block).await {
                                eprintln!("⚠️  Failed to update last block in database: {}", e);
                            }
                        }
                        
                        break; // Success, exit retry loop
                    }
                    Err(e) => {
                        if e.to_string().contains("rate limit") && retry_count < 3 {
                            retry_count += 1;
                            println!("⏳ Rate limited, waiting 2s and retrying... (attempt {}/3)", retry_count);
                            sleep(Duration::from_secs(2)).await;
                            continue; // Retry the same block range
                        } else {
                            eprintln!("❌ Error fetching logs for blocks {}-{}: {}", from_block, to_block, e);
                            break; // Give up and move to next range
                        }
                    }
                }
            }
            
            from_block = to_block + 1;
            
            // Small delay to avoid rate limiting
            if from_block < current_block {
                sleep(Duration::from_millis(100)).await;
            }
        }
        
        println!("✅ Sync complete: {} blocks processed, {} events found", blocks_processed, events_count);
        
        // Display points summary after historical sync
        tracker.display_points_summary();
    }

    let mut last_points_update = SystemTime::now();
    
    // Continuous monitoring loop
    loop {
        // Recalculate points every 60 seconds (since points accumulate over time)
        if SystemTime::now().duration_since(last_points_update).unwrap().as_secs() >= 60 {
            println!("\n⏰ Periodic points update");
            tracker.display_points_summary();
            last_points_update = SystemTime::now();
        }
        
        // Get the current block
        match provider.get_block_number().await {
            Ok(current_block) => {
                // Update tracker's current block
                tracker.current_block = current_block;
                
                // If there are new blocks, fetch logs
                if current_block > last_block {
                    // Silent check - only log if events are found
                    
                    // Create a filter for events in the new blocks
                    let filter = Filter::new()
                        .address(contract_address)
                        .from_block(last_block + 1)
                        .to_block(current_block);

                    // Get logs
                    match provider.get_logs(&filter).await {
                        Ok(logs) => {
                            if !logs.is_empty() {
                                println!("🔔 Found {} new events!", logs.len());
                                for log in logs {
                                    handle_log(log, &mut tracker).await?;
                                }
                                
                                // Display summary after processing events
                                tracker.display_points_summary();
                            }
                            // Silent when no events found
                            
                            // Always update the last processed block
                            last_block = current_block;
                            
                            // Save to database
                            if let Some(db) = &tracker.db {
                                if let Err(e) = db.update_last_processed_block(last_block).await {
                                    eprintln!("⚠️  Failed to update last block in database: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("❌ Error fetching logs: {}", e);
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("❌ Error getting current block: {}", e);
            }
        }

        // Wait before next poll
        sleep(Duration::from_secs(2)).await;
    }
}


async fn handle_log(log: Log, tracker: &mut PointsTracker) -> Result<()> {
    tracker.total_events_processed += 1;
    let block_num = log.block_number.unwrap_or_default();
    tracker.current_block = block_num;
    
    // Get the first topic (event signature)
    if let Some(_topic0) = log.topics().first() {
        // Try to decode each event type
        if let Ok(event) = SageStaking::Deposit::decode_log(&log.inner, true) {
            println!("\n📥 DEPOSIT EVENT [Block: {}]", block_num);
            println!("   User: {}", format_address(event.user));
            println!("   Amount: {} tokens", format_token_amount(event.amount));
            println!("   Nonce: {}", event.nonce);
            println!("   Timestamp: {}", format_timestamp(event.timestamp));
            println!("   Tx Hash: {}", log.transaction_hash.unwrap_or_default());
            
            // Track the position as active
            let position = Position {
                user: event.user,
                nonce: event.nonce.to::<u64>(),
                amount: event.amount,
                deposit_timestamp: event.timestamp.to::<u64>(),
                status: PositionStatus::Active,
                withdrawal_initiated_timestamp: None,
                block_number: block_num,
            };
            
            // Add to active positions
            tracker.add_active_position((event.user, event.nonce.to::<u64>()), position).await;
            
            // Save event to database
            if let Some(db) = &tracker.db {
                if let Err(e) = db.save_event(EventData {
                    event_type: "Deposit".to_string(),
                    user: event.user,
                    nonce: Some(event.nonce.to::<u64>()),
                    amount: Some(event.amount),
                    block_number: block_num,
                    tx_hash: log.transaction_hash.unwrap_or_default().to_string(),
                    timestamp: event.timestamp.to::<u64>(),
                }).await {
                    eprintln!("⚠️  Failed to save deposit event: {}", e);
                }
            }
            
            let user_points = tracker.calculate_user_points(&event.user);
            let (active, unstaking, withdrawn) = tracker.get_user_deposits_summary(&event.user);
            println!("   📊 User Points: SAGE={:.4}, FORM={:.4}", 
                user_points.sage_points, user_points.formation_points);
            println!("   💰 User Deposits: Active={:.2}, Unstaking={:.2}, Withdrawn={:.2}", 
                active, unstaking, withdrawn);
            
        } else if let Ok(event) = SageStaking::InitiateWithdraw::decode_log(&log.inner, true) {
            println!("\n⏳ INITIATE WITHDRAW EVENT [Block: {}]", block_num);
            println!("   User: {}", format_address(event.user));
            println!("   Nonce: {}", event.nonce);
            println!("   Unlocks At: {}", format_timestamp(event.unlocksAt));
            println!("   Timestamp: {}", format_timestamp(event.timestamp));
            println!("   Tx Hash: {}", log.transaction_hash.unwrap_or_default());
            
            // Move position from active to unstaking
            let key = (event.user, event.nonce.to::<u64>());
            if let Some(position) = tracker.get_position(&key) {
                let position_points = tracker.calculate_position_points(position);
                println!("   📊 Position Points Earned: SAGE={:.4}, FORM={:.4}", 
                    position_points.sage_points, position_points.formation_points);
                println!("   ⚠️  Points accumulation STOPPED for this position");
            }
            
            // Move to unstaking state
            tracker.move_to_unstaking(key, event.timestamp.to::<u64>()).await;
            
            // Save event to database
            if let Some(db) = &tracker.db {
                if let Err(e) = db.save_event(EventData {
                    event_type: "InitiateWithdraw".to_string(),
                    user: event.user,
                    nonce: Some(event.nonce.to::<u64>()),
                    amount: None,  // No amount in this event
                    block_number: block_num,
                    tx_hash: log.transaction_hash.unwrap_or_default().to_string(),
                    timestamp: event.timestamp.to::<u64>(),
                }).await {
                    eprintln!("⚠️  Failed to save initiate withdraw event: {}", e);
                }
            }
            
            let user_points = tracker.calculate_user_points(&event.user);
            let (active, unstaking, withdrawn) = tracker.get_user_deposits_summary(&event.user);
            println!("   📊 User Total Points: SAGE={:.4}, FORM={:.4}", 
                user_points.sage_points, user_points.formation_points);
            println!("   💰 User Deposits: Active={:.2}, Unstaking={:.2}, Withdrawn={:.2}", 
                active, unstaking, withdrawn);
            
        } else if let Ok(event) = SageStaking::Withdraw::decode_log(&log.inner, true) {
            println!("\n💸 WITHDRAW EVENT [Block: {}]", block_num);
            println!("   User: {}", format_address(event.user));
            println!("   Amount: {} tokens", format_token_amount(event.amount));
            println!("   Nonce: {}", event.nonce);
            println!("   Timestamp: {}", format_timestamp(event.timestamp));
            println!("   Tx Hash: {}", log.transaction_hash.unwrap_or_default());
            
            // Move position from unstaking to withdrawn
            let key = (event.user, event.nonce.to::<u64>());
            if let Some(position) = tracker.get_position(&key) {
                let position_points = tracker.calculate_position_points(position);
                println!("   📊 Final Position Points: SAGE={:.4}, FORM={:.4}", 
                    position_points.sage_points, position_points.formation_points);
            }
            
            // Move to withdrawn state
            tracker.move_to_withdrawn(key).await;
            
            // Save event to database
            if let Some(db) = &tracker.db {
                if let Err(e) = db.save_event(EventData {
                    event_type: "Withdraw".to_string(),
                    user: event.user,
                    nonce: Some(event.nonce.to::<u64>()),
                    amount: Some(event.amount),
                    block_number: block_num,
                    tx_hash: log.transaction_hash.unwrap_or_default().to_string(),
                    timestamp: event.timestamp.to::<u64>(),
                }).await {
                    eprintln!("⚠️  Failed to save withdraw event: {}", e);
                }
            }
            
            let user_points = tracker.calculate_user_points(&event.user);
            let (active, unstaking, withdrawn) = tracker.get_user_deposits_summary(&event.user);
            println!("   📊 User Total Points: SAGE={:.4}, FORM={:.4}", 
                user_points.sage_points, user_points.formation_points);
            println!("   💰 User Deposits: Active={:.2}, Unstaking={:.2}, Withdrawn={:.2}", 
                active, unstaking, withdrawn);
            
        } else if let Ok(event) = SageStaking::RestakeFromWithdrawalInitiated::decode_log(&log.inner, true) {
            println!("\n🔄 RESTAKE EVENT [Block: {}]", block_num);
            println!("   User: {}", format_address(event.user));
            println!("   Nonce: {}", event.nonce);
            println!("   Amount: {} tokens", format_token_amount(event.amount));
            println!("   Timestamp: {}", format_timestamp(event.timestamp));
            println!("   Tx Hash: {}", log.transaction_hash.unwrap_or_default());
            
            // Move position from unstaking back to active
            let key = (event.user, event.nonce.to::<u64>());
            tracker.move_to_active(key, event.timestamp.to::<u64>()).await;
            println!("   ✅ Points accumulation RESUMED for this position");
            
            // Save event to database
            if let Some(db) = &tracker.db {
                if let Err(e) = db.save_event(EventData {
                    event_type: "RestakeFromWithdrawalInitiated".to_string(),
                    user: event.user,
                    nonce: Some(event.nonce.to::<u64>()),
                    amount: Some(event.amount),
                    block_number: block_num,
                    tx_hash: log.transaction_hash.unwrap_or_default().to_string(),
                    timestamp: event.timestamp.to::<u64>(),
                }).await {
                    eprintln!("⚠️  Failed to save restake event: {}", e);
                }
            }
            
            let user_points = tracker.calculate_user_points(&event.user);
            let (active, unstaking, withdrawn) = tracker.get_user_deposits_summary(&event.user);
            println!("   📊 User Total Points: SAGE={:.4}, FORM={:.4}", 
                user_points.sage_points, user_points.formation_points);
            println!("   💰 User Deposits: Active={:.2}, Unstaking={:.2}, Withdrawn={:.2}", 
                active, unstaking, withdrawn);
        }
        
        println!("{}", "=".repeat(100));
    }

    Ok(())
}

// Helper function to format token amounts (assuming 18 decimals)
fn format_token_amount(amount: U256) -> String {
    // Convert to string and handle decimals
    let amount_str = amount.to_string();
    if amount_str.len() > 18 {
        let (whole, decimal) = amount_str.split_at(amount_str.len() - 18);
        let decimal_trimmed = decimal.trim_end_matches('0');
        if decimal_trimmed.is_empty() {
            whole.to_string()
        } else {
            format!("{}.{}", whole, &decimal_trimmed[..decimal_trimmed.len().min(6)])
        }
    } else {
        let padded = format!("{:0>18}", amount_str);
        let decimal_trimmed = padded.trim_end_matches('0');
        if decimal_trimmed.is_empty() {
            "0".to_string()
        } else {
            format!("0.{}", &decimal_trimmed[..decimal_trimmed.len().min(6)])
        }
    }
}

// Helper function to format Unix timestamps
fn format_timestamp(timestamp: U256) -> String {
    let timestamp_u64 = timestamp.to::<u64>();
    let duration = Duration::from_secs(timestamp_u64);
    let datetime = std::time::UNIX_EPOCH + duration;
    
    // Format as human-readable date-time
    if let Ok(datetime) = datetime.duration_since(std::time::UNIX_EPOCH) {
        let secs = datetime.as_secs();
        let date = chrono::DateTime::from_timestamp(secs as i64, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
            .unwrap_or_else(|| format!("Unix timestamp: {}", secs));
        date
    } else {
        format!("Unix timestamp: {}", timestamp_u64)
    }
}

// Helper function to format addresses (show first 6 and last 4 chars)
fn format_address(address: Address) -> String {
    let addr_str = format!("{:?}", address);
    if addr_str.len() > 10 {
        format!("{}...{}", &addr_str[..6], &addr_str[addr_str.len()-4..])
    } else {
        addr_str
    }
}

// Helper function to convert token amount to float (18 decimals)
fn format_token_amount_as_float(amount: U256) -> f64 {
    // Convert to string
    let amount_str = amount.to_string();
    
    // Parse as f64 and divide by 10^18
    if let Ok(amount_num) = amount_str.parse::<f64>() {
        amount_num / 1e18
    } else {
        0.0
    }
}