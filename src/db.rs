use alloy::primitives::{Address, U256};
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use eyre::Result;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, postgres::PgPoolOptions, Row};
use std::str::FromStr;

use crate::{Position, PositionStatus};

// Struct for saving events to avoid too many arguments
pub struct EventData {
    pub event_type: String,
    pub user: alloy::primitives::Address,
    pub nonce: Option<u64>,
    pub amount: Option<alloy::primitives::U256>,
    pub block_number: u64,
    pub tx_hash: String,
    pub timestamp: u64,
}

/// Response structure for user points data
#[derive(Debug, Serialize, Deserialize)]
pub struct UserPoints {
    pub address: String,
    pub sage_points: f64,
    pub formation_points: f64,
    pub total_points: f64,
    pub active_amount: f64,
    pub unstaking_amount: f64,
    pub withdrawn_amount: f64,
}

/// Historical event data for a user
#[derive(Debug, Serialize, Deserialize)]
pub struct UserEvent {
    pub event_type: String,
    pub amount: String,
    pub nonce: i64,
    pub timestamp: DateTime<Utc>,
    pub block_number: i64,
    pub status: String,
}

/// Entry in the points leaderboard
#[derive(Debug, Serialize, Deserialize)]
pub struct LeaderboardEntry {
    pub rank: i32,
    pub address: String,
    pub sage_points: f64,
    pub formation_points: f64,
    pub total_points: f64,
}

/// Database connection and operations handler
#[derive(Clone)]
pub struct Database {
    pool: PgPool,
}

impl Database {
    /// Create a new database connection with migrations
    pub async fn new(database_url: &str) -> Result<Self> {
        // Create connection pool
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await?;

        // Run migrations using sqlx migrate
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await?;
        
        Ok(Self { pool })
    }

    // Load all positions from database on startup
    pub async fn load_positions(&self) -> Result<(
        Vec<((Address, u64), Position)>,  // active
        Vec<((Address, u64), Position)>,  // unstaking
        Vec<((Address, u64), Position)>,  // withdrawn
    )> {
        let rows = sqlx::query(
            "SELECT user_address, nonce, amount, deposit_timestamp, status::text as status, 
             withdrawal_initiated_timestamp, block_number 
             FROM positions"
        )
        .fetch_all(&self.pool)
        .await?;

        let mut active = Vec::new();
        let mut unstaking = Vec::new();
        let mut withdrawn = Vec::new();

        for row in rows {
            let user_address: String = row.get("user_address");
            let nonce: i64 = row.get("nonce");
            let amount_str: BigDecimal = row.get("amount");
            let deposit_timestamp: i64 = row.get("deposit_timestamp");
            let status: String = row.get("status");
            let withdrawal_timestamp: Option<i64> = row.get("withdrawal_initiated_timestamp");
            let block_number: i64 = row.get("block_number");

            // Convert BigDecimal to U256
            let amount = U256::from_str(&amount_str.to_string()).unwrap_or_default();
            let address = Address::from_str(&user_address)?;
            
            let position = Position {
                user: address,
                nonce: nonce as u64,
                amount,
                deposit_timestamp: deposit_timestamp as u64,
                status: match status.as_str() {
                    "active" => PositionStatus::Active,
                    "unstaking" => PositionStatus::Unstaking,
                    "withdrawn" => PositionStatus::Withdrawn,
                    _ => PositionStatus::Active,
                },
                withdrawal_initiated_timestamp: withdrawal_timestamp.map(|t| t as u64),
                block_number: block_number as u64,
            };

            let key = (address, nonce as u64);
            
            match status.as_str() {
                "active" => active.push((key, position)),
                "unstaking" => unstaking.push((key, position)),
                "withdrawn" => withdrawn.push((key, position)),
                _ => {}
            }
        }

        println!("📚 Loaded {} active, {} unstaking, {} withdrawn positions from database", 
                 active.len(), unstaking.len(), withdrawn.len());

        Ok((active, unstaking, withdrawn))
    }

    // Save or update a position
    pub async fn save_position(&self, position: &Position) -> Result<()> {
        let status_str = match position.status {
            PositionStatus::Active => "active",
            PositionStatus::Unstaking => "unstaking",
            PositionStatus::Withdrawn => "withdrawn",
        };

        let amount_str = position.amount.to_string();

        sqlx::query(
            "INSERT INTO positions 
             (user_address, nonce, amount, deposit_timestamp, status, 
              withdrawal_initiated_timestamp, block_number, updated_at)
             VALUES ($1, $2, $3, $4, $5::position_status, $6, $7, CURRENT_TIMESTAMP)
             ON CONFLICT (user_address, nonce) 
             DO UPDATE SET 
                amount = EXCLUDED.amount,
                deposit_timestamp = EXCLUDED.deposit_timestamp,
                status = EXCLUDED.status,
                withdrawal_initiated_timestamp = EXCLUDED.withdrawal_initiated_timestamp,
                block_number = EXCLUDED.block_number,
                updated_at = CURRENT_TIMESTAMP"
        )
        .bind(position.user.to_string())
        .bind(position.nonce as i64)
        .bind(BigDecimal::from_str(&amount_str).unwrap_or_else(|_| BigDecimal::from(0)))
        .bind(position.deposit_timestamp as i64)
        .bind(status_str)
        .bind(position.withdrawal_initiated_timestamp.map(|t| t as i64))
        .bind(position.block_number as i64)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    // Save an event for audit trail
    pub async fn save_event(&self, event: EventData) -> Result<()> {
        let amount_str = event.amount.and_then(|a| BigDecimal::from_str(&a.to_string()).ok());

        sqlx::query(
            "INSERT INTO events 
             (event_type, user_address, nonce, amount, block_number, transaction_hash, timestamp)
             VALUES ($1, $2, $3, $4, $5, $6, $7)"
        )
        .bind(event.event_type)
        .bind(event.user.to_string())
        .bind(event.nonce.map(|n| n as i64))
        .bind(amount_str)
        .bind(event.block_number as i64)
        .bind(event.tx_hash)
        .bind(event.timestamp as i64)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    // Get last processed block
    pub async fn get_last_processed_block(&self) -> Result<Option<u64>> {
        let row = sqlx::query(
            "SELECT value FROM sync_metadata WHERE key = 'last_processed_block'"
        )
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let value: String = row.get("value");
            Ok(value.parse::<u64>().ok())
        } else {
            Ok(None)
        }
    }

    // Update last processed block
    pub async fn update_last_processed_block(&self, block: u64) -> Result<()> {
        sqlx::query(
            "INSERT INTO sync_metadata (key, value, updated_at) 
             VALUES ('last_processed_block', $1, CURRENT_TIMESTAMP)
             ON CONFLICT (key) 
             DO UPDATE SET value = EXCLUDED.value, updated_at = CURRENT_TIMESTAMP"
        )
        .bind(block.to_string())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    // API Methods
    
    /// Get user points and deposit summary for a specific address
    pub async fn get_user_points(&self, user_address: &str) -> Result<UserPoints> {
        // Get all positions for the user
        let rows = sqlx::query(
            "SELECT nonce, amount, deposit_timestamp, status::text as status, 
                    withdrawal_initiated_timestamp, block_number
             FROM positions 
             WHERE user_address = $1"
        )
        .bind(user_address)
        .fetch_all(&self.pool)
        .await?;

        let mut sage_points = 0.0;
        let mut formation_points = 0.0;
        let mut active_amount = 0.0;
        let mut unstaking_amount = 0.0;
        let mut withdrawn_amount = 0.0;

        let current_time = chrono::Utc::now().timestamp();

        for row in rows {
            let amount: BigDecimal = row.get("amount");
            let amount_float = amount.to_string().parse::<f64>().unwrap_or(0.0) / 1e18;
            let deposit_timestamp: i64 = row.get("deposit_timestamp");
            let status: String = row.get("status");
            let withdrawal_initiated_timestamp: Option<i64> = row.get("withdrawal_initiated_timestamp");

            // Calculate points based on status
            let end_timestamp = if let Some(withdrawal_ts) = withdrawal_initiated_timestamp {
                withdrawal_ts
            } else if status == "active" {
                current_time
            } else {
                deposit_timestamp
            };

            let seconds_staked = (end_timestamp - deposit_timestamp) as f64;
            let days_staked = seconds_staked / 86400.0;
            
            // Calculate points (0.01 SAGE per token per day, 0.005 Formation per token per day)
            sage_points += amount_float * days_staked * 0.01;
            formation_points += amount_float * days_staked * 0.005;

            // Sum amounts by status
            match status.as_str() {
                "active" => active_amount += amount_float,
                "unstaking" => unstaking_amount += amount_float,
                "withdrawn" => withdrawn_amount += amount_float,
                _ => {}
            }
        }

        Ok(UserPoints {
            address: user_address.to_string(),
            sage_points,
            formation_points,
            total_points: sage_points + formation_points,
            active_amount,
            unstaking_amount,
            withdrawn_amount,
        })
    }

    /// Get historical event data for a specific user
    pub async fn get_user_events(&self, user_address: &str) -> Result<Vec<UserEvent>> {
        let rows = sqlx::query(
            "SELECT e.event_type, e.amount, e.nonce, e.timestamp, e.block_number,
                    COALESCE(p.status::text, '') as status
             FROM events e
             LEFT JOIN positions p ON p.user_address = e.user_address AND p.nonce = e.nonce
             WHERE e.user_address = $1
             ORDER BY e.block_number DESC, e.timestamp DESC"
        )
        .bind(user_address)
        .fetch_all(&self.pool)
        .await?;

        let mut events = Vec::new();
        for row in rows {
            let amount: Option<BigDecimal> = row.get("amount");
            let amount_str = if let Some(amt) = amount {
                format!("{:.6}", amt.to_string().parse::<f64>().unwrap_or(0.0) / 1e18)
            } else {
                "0.000000".to_string()
            };
            
            events.push(UserEvent {
                event_type: row.get("event_type"),
                amount: amount_str,
                nonce: row.get("nonce"),
                timestamp: DateTime::from_timestamp(row.get("timestamp"), 0).unwrap_or_default(),
                block_number: row.get("block_number"),
                status: row.get("status"),
            });
        }

        Ok(events)
    }

    /// Get the top users by total points
    pub async fn get_leaderboard(&self, limit: i64) -> Result<Vec<LeaderboardEntry>> {
        // Complex query to calculate points for all users
        let rows = sqlx::query(
            "WITH user_points AS (
                SELECT 
                    user_address,
                    SUM(
                        CAST(amount AS FLOAT) / 1e18 * 
                        (EXTRACT(EPOCH FROM (
                            CASE 
                                WHEN withdrawal_initiated_timestamp IS NOT NULL THEN 
                                    to_timestamp(withdrawal_initiated_timestamp)
                                WHEN status = 'active' THEN 
                                    NOW()
                                ELSE 
                                    to_timestamp(deposit_timestamp)
                            END
                        )) - deposit_timestamp) / 86400.0 * 0.01
                    ) AS sage_points,
                    SUM(
                        CAST(amount AS FLOAT) / 1e18 * 
                        (EXTRACT(EPOCH FROM (
                            CASE 
                                WHEN withdrawal_initiated_timestamp IS NOT NULL THEN 
                                    to_timestamp(withdrawal_initiated_timestamp)
                                WHEN status = 'active' THEN 
                                    NOW()
                                ELSE 
                                    to_timestamp(deposit_timestamp)
                            END
                        )) - deposit_timestamp) / 86400.0 * 0.005
                    ) AS formation_points
                FROM positions
                GROUP BY user_address
            )
            SELECT 
                user_address,
                sage_points,
                formation_points,
                (sage_points + formation_points) AS total_points,
                ROW_NUMBER() OVER (ORDER BY (sage_points + formation_points) DESC) AS rank
            FROM user_points
            ORDER BY total_points DESC
            LIMIT $1"
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let mut leaderboard = Vec::new();
        for row in rows {
            leaderboard.push(LeaderboardEntry {
                rank: row.get::<i64, _>("rank") as i32,
                address: row.get("user_address"),
                sage_points: row.get::<f64, _>("sage_points"),
                formation_points: row.get::<f64, _>("formation_points"),
                total_points: row.get::<f64, _>("total_points"),
            });
        }

        Ok(leaderboard)
    }
}
