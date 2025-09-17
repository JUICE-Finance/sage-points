# Railway Deployment Guide

This guide will walk you through deploying the Points Calculator service to Railway.

## Prerequisites

- GitHub account with this repository
- Railway account (sign up at https://railway.app)
- Base RPC endpoint URL (you can use the public one or get your own from Alchemy/Infura)

## Step-by-Step Deployment

### 1. Push to GitHub

First, make sure your code is pushed to GitHub:

```bash
git init
git add .
git commit -m "Initial commit"
git remote add origin YOUR_GITHUB_REPO_URL
git push -u origin main
```

### 2. Create Railway Project

1. Go to [Railway Dashboard](https://railway.app/dashboard)
2. Click "New Project"
3. Select "Deploy from GitHub repo"
4. Authorize Railway to access your GitHub account
5. Select your `points-calculator` repository

### 3. Add PostgreSQL Database

1. In your Railway project, click "New Service"
2. Select "Database" → "Add PostgreSQL"
3. Railway will automatically create the database and set `DATABASE_URL`

### 4. Configure Environment Variables

Click on your service and go to the "Variables" tab. Add these required variables:

```env
BASE_RPC_URL=https://mainnet.base.org
CONTRACT_ADDRESS=0x413D15aFe510cD1003540E8EF57A29eF9a086Efc
DEPLOYMENT_BLOCK=35283433
```

Note: `DATABASE_URL` and `PORT` are automatically provided by Railway.

### 5. Deploy

Railway will automatically:
1. Detect the Dockerfile
2. Build your application
3. Run database migrations
4. Start the service

### 6. Monitor Deployment

- Check the "Deployments" tab for build logs
- Once deployed, Railway will provide a URL like `https://your-app.railway.app`
- Test the health endpoint: `https://your-app.railway.app/health`

## API Endpoints

Once deployed, your API will be available at:

- `GET https://your-app.railway.app/health` - Health check
- `GET https://your-app.railway.app/api/points/{address}` - Get user points
- `GET https://your-app.railway.app/api/events/{address}` - Get user events
- `GET https://your-app.railway.app/api/leaderboard?limit=10` - Get leaderboard

## Monitoring & Logs

### View Logs
In Railway dashboard, click on your service and go to the "Logs" tab to see real-time logs.

### Database Access
To connect to your PostgreSQL database:
1. Click on the PostgreSQL service
2. Go to "Connect" tab
3. Copy the connection string for your preferred tool

### Metrics
Railway provides basic metrics (CPU, Memory, Network) in the dashboard.

## Updating the Service

To deploy updates:

1. Push changes to GitHub:
```bash
git add .
git commit -m "Update description"
git push
```

2. Railway will automatically detect the push and redeploy

## Troubleshooting

### Service Won't Start
- Check environment variables are set correctly
- Review deployment logs for errors
- Ensure database migrations ran successfully

### High Memory Usage
- The service caches positions in memory
- This is normal and helps with API performance
- Railway will auto-scale if needed

### RPC Rate Limiting
- Consider using a paid RPC provider (Alchemy, Infura, QuickNode)
- Update `BASE_RPC_URL` environment variable with your provider URL

### Database Connection Issues
- Railway manages the database connection
- If issues persist, check the PostgreSQL service status
- Migrations run automatically on startup

## Cost Estimation

Railway pricing (as of 2024):
- **Hobby Plan**: $5/month (includes $5 of usage)
- **Estimated usage**:
  - Service: ~$3-5/month (running 24/7)
  - PostgreSQL: ~$5/month
  - Total: ~$8-10/month

## Security Considerations

1. **Environment Variables**: Never commit `.env` files
2. **Database**: Railway provides automatic backups
3. **API**: CORS is configured to allow all origins (update if needed)
4. **RPC Endpoint**: Consider using your own for better reliability

## Support

For Railway-specific issues:
- [Railway Documentation](https://docs.railway.app)
- [Railway Discord](https://discord.gg/railway)

For application issues:
- Check the logs in Railway dashboard
- Review the README.md for configuration details
