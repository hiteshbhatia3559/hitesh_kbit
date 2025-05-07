# Hyperliquid Market Maker - Design Document & Roadmap

## Overview

This document outlines the design and implementation roadmap for a scalable, dockerized, UI-based market making application for Hyperliquid. The application will make markets across the top 10 perpetual swaps, with configurable risk parameters and position management features.

## Architecture

The application will follow a microservices architecture with the following components:

1. **Core Market Maker Service** - Implements the market making logic, leveraging the Hyperliquid Rust SDK ✅
2. **Symbol Scanner Service** - Periodically scans for new listings and tracks volume/OI metrics ✅
3. **Configuration Service** - Handles dynamic configuration via Redis ✅
4. **Position Manager** - Tracks and manages positions, implements risk controls ✅
5. **API Gateway** - Provides a unified interface for the UI
6. **UI Dashboard** - Web interface for monitoring and configuration (basic structure only) ✅

![Architecture Diagram (placeholder)]()

## Data Flow

1. Symbol Scanner collects market data and stores in Redis ✅
2. Manual configs are received via Redis pubsub channel "mm_config" ✅
3. Core Market Maker reads configs and market data to determine quoting parameters ✅
4. Market Maker submits orders via Hyperliquid SDK ✅
5. Position Manager tracks all positions and risk metrics ✅
6. Position updates are published to Redis stream and pubsub channel "mm_position_updates" ✅
7. UI Dashboard subscribes to position updates and displays real-time data

## Technology Stack

- **Backend**: Rust (leveraging Hyperliquid SDK) ✅
- **Data Storage**: Redis ✅
- **Containerization**: Docker + Docker Compose ✅
- **UI**: React with WebSockets for real-time updates (structure only) ✅
- **API Layer**: RESTful API with JSON
- **Deployment**: Kubernetes (optional for scaling)

## Components Detail

### 1. Core Market Maker Service

This service will build upon the existing `MarketMaker` implementation from the Hyperliquid SDK but extend it with:

- Support for multiple assets simultaneously ✅
- Advanced spread calculation based on volatility and target return ✅
- Risk management features (stop loss, take profit) ✅
- Position limits enforcement ✅
- Dynamic parameter adjustment ✅

```rust
// Key structures (conceptual)
struct EnhancedMarketMakerConfig {
    daily_return_bps: u16,
    notional_per_side: f64,
    daily_pnl_stop_loss: f64,
    trailing_take_profit: f64,
    trailing_stop_loss: f64,
    hedge_only_mode: bool,
    force_quote_refresh_interval: u64,
    max_long_usd: f64,
    max_short_usd: f64,
}

struct MarketMakingEngine {
    configs: HashMap<String, EnhancedMarketMakerConfig>,
    market_makers: HashMap<String, MarketMaker>,
    position_manager: PositionManager,
    // ...
}
```

### 2. Symbol Scanner Service ✅

```rust
struct SymbolMetrics {
    symbol: String,
    volume_24h: f64,
    open_interest: f64,
    is_active: bool,
    last_updated: u64,
}

struct SymbolScanner {
    info_client: InfoClient,
    redis_client: RedisClient,
    scan_interval: Duration,
    // ...
}
```

### 3. Position Manager ✅

```rust
struct Position {
    symbol: String,
    size: f64,
    entry_price: f64,
    current_price: f64,
    unrealized_pnl: f64,
    notional_usd: f64,
    // ...
}

struct PositionManager {
    positions: HashMap<String, Position>,
    redis_client: RedisClient,
    // ...
}
```

### 4. Configuration Service ✅

```rust
struct ConfigService {
    redis_client: RedisClient,
    config_channel: String,
    // ...
}
```

## Redis Schema

### Config Channel ✅
```json
{
  "symbol": "BTC",
  "daily_return_bps": 10,
  "notional_per_side": 1000.0,
  "daily_pnl_stop_loss": 100.0,
  "trailing_take_profit": 0.05,
  "trailing_stop_loss": 0.02,
  "hedge_only_mode": false,
  "force_quote_refresh_interval": 500,
  "max_long_usd": 10000.0,
  "max_short_usd": 10000.0
}
```

### Symbol Metrics ✅
```json
{
  "symbols": [
    {
      "symbol": "BTC",
      "volume_24h": 1000000.0,
      "open_interest": 500000.0,
      "is_active": true,
      "last_updated": 1623456789
    },
    // ...
  ]
}
```

### Position Updates Channel ✅
```json
{
  "timestamp": 1623456789,
  "positions": [
    {
      "symbol": "BTC",
      "size": 0.1,
      "entry_price": 50000.0,
      "current_price": 51000.0,
      "unrealized_pnl": 100.0,
      "notional_usd": 5100.0
    },
    // ...
  ],
  "total_pnl": 150.0,
  "total_long_exposure": 10000.0,
  "total_short_exposure": 5000.0
}
```

## Implementation Roadmap

### Phase 1: Foundation (Weeks 1-2) ✅
- Set up project structure and Docker environment ✅
- Implement Redis integration ✅
- Create Symbol Scanner service ✅
- Extend the Market Maker from SDK to support configuration params ✅

### Phase 2: Core Functionality (Weeks 3-4)
- Implement Position Manager with risk controls ✅
- Develop Configuration Service for dynamic parameters ✅
- Build integration between components ✅
- Add logging and monitoring ✅

### Phase 3: UI and Analytics (Weeks 5-6)
- Develop API Gateway
- Implement basic UI Dashboard (skeleton structure ✅)
- Add real-time position and PnL visualizations
- Create configuration interface

### Phase 4: Refinement and Testing (Weeks 7-8)
- End-to-end testing
- Performance optimization
- Stress testing
- Documentation

### Phase 5: Production Deployment (Weeks 9)
- Kubernetes deployment (optional)
- Integration with external monitoring
- Security hardening
- Final documentation and handover

## SDK Integration Points

The implementation will leverage the following components from the Hyperliquid SDK:

1. `InfoClient` for market data ✅
2. `ExchangeClient` for order execution ✅
3. Base `MarketMaker` implementation as a starting point ✅
4. WebSocket subscriptions for real-time data ✅

## User Interface Mockup

The UI will include:
- Real-time position dashboard
- PnL charts and metrics
- Configuration panel
- Order history and execution metrics
- Risk metrics visualization

## Future Enhancements

1. Machine learning for optimal spread determination
2. Integration with external data sources
3. Advanced risk models
4. Multiple account support
5. Historical performance analytics
