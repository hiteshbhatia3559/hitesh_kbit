# Hyperliquid Market Maker

A scalable, dockerized, UI-based market making application for Hyperliquid perpetual swaps.

## Overview

This application implements an automated market making strategy for the top 10 perpetual swaps on Hyperliquid. It leverages the Hyperliquid Rust SDK and provides configurable risk parameters via Redis pubsub.

## Features

- Automatically makes markets across multiple tokens
- Configurable via Redis pubsub channel
- Real-time position monitoring and management
- Web-based UI for monitoring and configuration
- Containerized for easy deployment

## Architecture

The application is built with the following components:

- **Core Market Maker**: Implements market making logic using Hyperliquid SDK
- **Symbol Scanner**: Scans for new token listings and metrics
- **Configuration Service**: Manages dynamic parameters
- **Position Manager**: Tracks positions and implements risk controls
- **Redis**: Used for configuration and position updates
- **UI**: Web interface for monitoring and control

## Getting Started

### Prerequisites

- Docker and Docker Compose
- Redis (included in docker-compose setup)
- Hyperliquid API keys (for production use)

### Setup and Running

1. Clone this repository and the Hyperliquid SDK:

```bash
git clone https://github.com/your-username/hyperliquid-market-maker.git
cd hyperliquid-market-maker
```

2. Configure the environment variables:

The application uses environment variables for configuration. You can:
- Create a `.env` file based on the `env.sample` template
- Set environment variables directly in your shell
- Pass them to docker-compose

The important environment variables:
- `HYPERLIQUID_WALLET_KEY`: Your private key for interacting with Hyperliquid (required)
- `REDIS_URL`: URL for Redis connection (default: redis://redis:6379)
- `RUST_LOG`: Logging level (default: info)
- `HYPERLIQUID_TESTNET`: Whether to use testnet (default: true)

3. Build and run with Docker Compose:

```bash
./build.sh
docker-compose up
```

3. Send configurations via Redis:

Use the provided scripts in `test_scripts/` to send configurations:

```bash
./test_scripts/send_config.py --symbol BTC
```

4. Monitor positions and market maker status:

```bash
./test_scripts/listen_positions.py
```

## Configuration

The market maker can be configured via Redis pubsub channel "mm_config". Configuration parameters:

- **symbol** - Trading symbol (e.g., "BTC", "ETH")
- **daily_return_bps** - Target daily return in basis points
- **notional_per_side** - Size of orders to quote on each side
- **daily_pnl_stop_loss** - Daily stop loss limit
- **trailing_take_profit** - Trailing take profit percentage (0-1)
- **trailing_stop_loss** - Trailing stop loss percentage (0-1)
- **hedge_only_mode** - Only add liquidity, don't take directional risk
- **force_quote_refresh_interval** - Quote refresh rate in ms
- **max_long_usd** - Maximum long exposure in USD
- **max_short_usd** - Maximum short exposure in USD

## Development

For local development:

```bash
cargo build
cargo run
```

For more details, see the `Design.md` document which outlines the complete architecture and roadmap.

## License

MIT 