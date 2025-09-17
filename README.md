# SageStaking Points Calculator API

A Rust-based service that monitors the SageStaking smart contract on Base mainnet, calculates staking points, and provides a REST API for frontend integration.

## Contract Details
- **Address**: `0x413D15aFe510cD1003540E8EF57A29eF9a086Efc`
- **Network**: Base Mainnet
- **Events Monitored**:
  - `Deposit`: When users stake tokens
  - `InitiateWithdraw`: When users start the withdrawal process
  - `Withdraw`: When users complete withdrawals
  - `RestakeFromWithdrawalInitiated`: When users cancel withdrawal and restake

## Prerequisites

- Rust 1.70+ installed
- Cargo package manager
- PostgreSQL installed (for persistent storage)

## Installation

### Quick Setup
```bash
# Run the setup script
./setup.sh

# Edit .env file with your database credentials
cp env.example .env
# Edit .env with your PostgreSQL connection string
```

### Manual Setup
1. Install PostgreSQL
2. Create database: `createdb points_calculator`
3. Copy env.example to .env and configure
4. Build the project:
```bash
cargo build --release
```

## Usage

Run the service:
```bash
cargo run
```

The service will:
- Start an HTTP API server on port 3000 (or PORT env variable)
- Run blockchain monitoring in the background
- Connect to Base mainnet via HTTP RPC
- **On first run**: Fetch all historical events from the deployment block specified in env
- **On subsequent runs**: Resume from the last processed block stored in database
- Process historical events in batches to avoid RPC limits
- Poll every 2 seconds for new events
- Automatically save progress to PostgreSQL
- All block tracking is persisted in the database (no file storage)

## API Endpoints

The service provides a REST API on port 3000 (configurable via PORT environment variable). All endpoints support CORS for frontend integration.

### 1. Health Check
Check if the service is running and healthy.

**Endpoint:**
```
GET /health
```

**Example Request:**
```bash
curl http://localhost:3000/health
```

**Example Response:**
```json
{
  "status": "ok",
  "timestamp": "2025-09-17T12:00:00Z"
}
```

### 2. Get User Points
Returns points breakdown and balance information for a specific address.

**Endpoint:**
```
GET /api/points/{address}
```

**Parameters:**
- `address` (path parameter): Ethereum address (checksummed or lowercase)

**Example Request:**
```bash
curl http://localhost:3000/api/points/0xc7827Cbf84A0556f33d04d76c4aEc1FE73469fe
```

**Example Response:**
```json
{
  "status": "success",
  "data": {
    "address": "0xc7827cbf84a0556f33d04d76c4aec1fe7346969fe",
    "sage_points": 12.7974,
    "formation_points": 3.1993,
    "total_points": 15.9967,
    "active_balance": 169.0,
    "unstaking_balance": 220.0,
    "withdrawn_balance": 0.0
  }
}
```

**Error Response (User Not Found):**
```json
{
  "status": "error",
  "message": "User not found",
  "data": null
}
```

### 3. Get User Events
Returns historical blockchain events for a specific user address.

**Endpoint:**
```
GET /api/events/{address}
```

**Parameters:**
- `address` (path parameter): Ethereum address (checksummed or lowercase)

**Example Request:**
```bash
curl http://localhost:3000/api/events/0xc7827Cbf84A0556f33d04d76c4aEc1FE73469fe

# With pretty print using jq
curl -s http://localhost:3000/api/events/0xc7827Cbf84A0556f33d04d76c4aEc1FE73469fe | jq .
```

**Example Response:**
```json
{
  "status": "success",
  "data": [
    {
      "event_type": "Deposit",
      "user_address": "0xc7827cbf84a0556f33d04d76c4aec1fe7346969fe",
      "nonce": 42,
      "amount": "100.0",
      "block_number": 35283500,
      "transaction_hash": "0x123abc...",
      "timestamp": "2025-09-17T10:00:00Z",
      "position_status": "Active"
    },
    {
      "event_type": "InitiateWithdraw",
      "user_address": "0xc7827cbf84a0556f33d04d76c4aec1fe7346969fe",
      "nonce": 42,
      "amount": null,
      "block_number": 35284000,
      "transaction_hash": "0x456def...",
      "timestamp": "2025-09-17T11:00:00Z",
      "position_status": "Unstaking"
    }
  ]
}
```

### 4. Get Leaderboard
Returns top users ranked by total points.

**Endpoint:**
```
GET /api/leaderboard
```

**Query Parameters:**
- `limit` (optional): Number of users to return (default: 10, max: 100)

**Example Requests:**
```bash
# Get top 10 users (default)
curl http://localhost:3000/api/leaderboard

# Get top 5 users
curl "http://localhost:3000/api/leaderboard?limit=5"

# Get top 20 users with pretty print
curl -s "http://localhost:3000/api/leaderboard?limit=20" | jq .
```

**Example Response:**
```json
{
  "status": "success",
  "data": [
    {
      "rank": 1,
      "address": "0xc7827cbf84a0556f33d04d76c4aec1fe7346969fe",
      "sage_points": 12.7974,
      "formation_points": 3.1993,
      "total_points": 15.9967
    },
    {
      "rank": 2,
      "address": "0xf250b0886ec22d1fc4070baac90fcd1d87a2d74a",
      "sage_points": 1.1087,
      "formation_points": 0.2772,
      "total_points": 1.3859
    },
    {
      "rank": 3,
      "address": "0xd6f2af86ac87b6e9a1b74c946f0c2a0c1f7cbf7cb",
      "sage_points": 0.0006,
      "formation_points": 0.0001,
      "total_points": 0.0007
    }
  ]
}
```

## Testing the API

### Quick Test Commands
Test all endpoints with these commands:

```bash
# 1. Health Check
curl http://localhost:3000/health

# 2. Get specific user's points
curl http://localhost:3000/api/points/0xc7827Cbf84A0556f33d04d76c4aEc1FE73469fe

# 3. Get user's event history
curl http://localhost:3000/api/events/0xc7827Cbf84A0556f33d04d76c4aEc1FE73469fe

# 4. Get leaderboard (top 10)
curl http://localhost:3000/api/leaderboard

# 5. Get leaderboard with custom limit
curl "http://localhost:3000/api/leaderboard?limit=5"

# 6. Test invalid address handling
curl http://localhost:3000/api/points/invalid_address
```

### Pretty Print with jq
For better readability, install `jq` and pipe the output:

```bash
# Install jq on macOS
brew install jq

# Use with API calls
curl -s http://localhost:3000/api/leaderboard | jq .
curl -s http://localhost:3000/api/points/0xc7827Cbf84A0556f33d04d76c4aEc1FE73469fe | jq .
```

### Production Testing
After deploying to Railway, replace `localhost:3000` with your Railway URL:

```bash
# Example with Railway deployment
API_URL=https://your-app.railway.app

curl $API_URL/health
curl $API_URL/api/points/0xc7827Cbf84A0556f33d04d76c4aEc1FE73469fe
curl $API_URL/api/leaderboard?limit=10
```

## Configuration

All configuration is done through environment variables. Copy `env.example` to `.env` and fill in the required values:

```bash
cp env.example .env
```

### Required Environment Variables

- **DATABASE_URL**: PostgreSQL connection string (Railway provides this automatically)
- **BASE_RPC_URL**: Base mainnet RPC endpoint (e.g., `https://mainnet.base.org`)
- **CONTRACT_ADDRESS**: SageStaking contract address
- **DEPLOYMENT_BLOCK**: Starting block for event syncing

### Optional Environment Variables

- **PORT**: API server port (default: 3000, Railway provides this automatically)

### State Persistence

- The last processed block is stored in the database
- To re-sync from the beginning, you can reset the database or manually update the `sync_metadata` table

## Output Format

Events are displayed with the following information:
- Event type with emoji indicator
- User address
- Token amounts (formatted with 18 decimals)
- Nonce values
- Timestamps (human-readable format)
- Block number
- Transaction hash

Example output:
```
📥 DEPOSIT EVENT
   User: 0x1234...5678
   Amount: 1000.5 tokens
   Nonce: 42
   Timestamp: 2024-01-15 14:30:00 UTC
   Block: 1234567
   Tx Hash: 0xabcd...ef01
```

## Deployment

### Railway Deployment

1. Create a new Railway project
2. Add PostgreSQL service to your project
3. Deploy from GitHub:
   - Railway will automatically detect Rust project
   - DATABASE_URL will be automatically provided
   - All positions and state will persist in PostgreSQL

### Environment Variables

Required for production:
- `DATABASE_URL`: PostgreSQL connection string (auto-provided by Railway)
- `BASE_RPC_URL`: Base mainnet RPC endpoint
- `CONTRACT_ADDRESS`: SageStaking contract address
- `DEPLOYMENT_BLOCK`: Starting block for sync
- `PORT`: HTTP API port (default: 3000, auto-provided by Railway)

## Database

The system uses PostgreSQL to persist:
- All staking positions (active, unstaking, withdrawn)
- Points calculations
- Last processed block
- Event audit trail

Points are recalculated dynamically but position states are persisted.

## Troubleshooting

### Connection Issues
- Check PostgreSQL connection string in .env
- Ensure database exists: `createdb points_calculator`
- Consider using your own RPC endpoint for better reliability

### Performance
- Batch size can be adjusted via MAX_BLOCK_RANGE
- Database indexes are automatically created for efficient queries
- Consider rate limits when using public RPC endpoints

## Dependencies

- `alloy`: Ethereum client library
- `tokio`: Async runtime
- `actix-web`: High-performance web framework
- `actix-cors`: CORS middleware for Actix
- `sqlx`: Async PostgreSQL driver
- `eyre`: Error handling
- `chrono`: Timestamp formatting with serde support
- `serde`: Serialization/deserialization
- `serde_json`: JSON support
- `dotenv`: Environment variable management
- `env_logger`: Logging framework

## License

MIT

