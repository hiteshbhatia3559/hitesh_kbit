# Changelog

All notable changes to the Hyperliquid Market Maker project will be documented in this file.

## [0.2.0] - 2025-05-06 19:25

### Added
- Added support for trading through vault addresses instead of main account
- Updated `send_config.py` to accept `--vault-address` parameter for specifying vault address
- Implemented proper vault address handling in the exchange client initialization
- Enhanced configuration with vault address parameter for safer trading
- Added multi-level quoting capability with configurable spread and size multipliers at each level
- Implemented bulk order placement across multiple price levels for more realistic market making

### Changed
- Modified order placement to properly use vault address when specified
- Enhanced slow path configuration handling to recreate exchange client when vault address changes
- Switched from modify-in-place to cancel-and-replace for order updates for better reliability
- Improved order tracking with enhanced order objects that include side information

### Fixed
- Fixed issue where vault address wasn't being properly passed to the exchange client
- Addressed edge cases in vault address string conversion to Ethereum address format
- Added proper error handling for invalid vault address format
- Fixed configuration comparison logic for vault address and quote levels

### Technical
- Added diagnostic logging for vault address usage during order placement
- Implemented thread-safe access to vault address configuration
- Enhanced order objects to better track essential order properties
- Added proper versioning for all configuration objects

## [0.1.9] - 2025-05-06 18:00

### Added
- Added `enable_trading` configuration flag to control order placement
- Created `toggle_trading.py` script for quickly enabling/disabling trading without stopping service
- Enhanced configuration update mechanism to properly handle all parameters
- Improved logging for configuration changes across all parameters
- Changed order type from GTC (Good Till Cancelled) to ALO (Add Limit Only) for better market making

### Changed
- Modified the main loop to detect changes in all configuration parameters
- Updated the slow path to store and log complete configuration objects
- Enhanced log messages to show comprehensive before/after values for all parameters
- Improved JSON handling in scripts to properly decode binary data from Redis
- Updated order placement to use Add Limit Only (ALO) mode instead of Good Till Cancelled (GTC)
- Reduced Redis configuration polling interval from 10 seconds to 1 second for faster updates

### Technical
- Added serde default attribute for backward compatibility with older configs
- Implemented proper error handling for missing or undefined configuration fields
- Enhanced configuration change detection for better visibility and debugging
- Switched to ALO order type to ensure liquidity is only added, never taken
- Increased configuration responsiveness with 10x faster polling interval

## [0.1.8] - 2025-05-07

### Changed
- Modified `send_config.py` to write configuration directly to Redis key (`config:{symbol}`) instead of publishing to a channel
- Reduced default `notional_per_side` value from 1500.0 to 150.0 for lower trading volume

### Technical
- Aligned test script with the Redis key format used by the market maker's slow path

## [0.1.7] - 2025-05-06 06:30

### Added
- Implemented Redis-based configuration polling ("slow path") mechanism
- Added periodic (10 second) configuration checks from Redis storage
- Created direct Redis integration for market makers

### Changed
- Simplified configuration update mechanism to use direct Redis polling
- Removed message-based configuration update mechanism
- Replaced atomic flag signaling with direct configuration updates

### Technical
- Added background task for regular config polling
- Implemented thread-safe access to shared market maker state
- Enhanced logging for Redis-based configuration updates
- Added proper error handling for Redis operations

## [0.1.6] - 2025-05-06 05:45

### Fixed
- Fixed critical timing issue in configuration update process
- Corrected the location of config change detection in market maker loop
- Added immediate order cancellation when configuration changes
- Enhanced logging of configuration parameters throughout the update process

### Technical
- Fixed race condition between config updates and config usage
- Moved config change detection closer to where config values are used
- Added detailed parameter logging for easier debugging
- Added verification that config values are properly propagated

## [0.1.5] - 2025-05-06 05:00

### Added
- Implemented atomic flag-based configuration change detection
- Added immediate order cancellation when configuration changes
- Enhanced logging of configuration change detection and application

### Fixed
- Fixed critical bug where configuration changes weren't being applied to running market makers
- Fixed configuration parameter mismatch between update detection and application
- Added force quote refresh on configuration change

### Technical
- Used atomic flags for thread-safe configuration change notification
- Improved configuration change visibility with detailed "before/after" logging
- Added configuration synchronization between main thread and market maker tasks

## [0.1.4] - 2025-05-06 04:15

### Added
- Enhanced configuration update mechanism with proper change detection
- Added configuration listener tool (`listen_config.py`) for debugging Redis messaging
- Improved logging for configuration updates and changes

### Fixed
- Fixed configuration updates not being properly detected and applied
- Added persistent storage of configurations in Redis for better recovery
- Improved Redis connection handling with retries and better error reporting

### Technical
- Added detailed debugging information for configuration update process
- Improved Redis URL consistency across components

## [0.1.3] - 2025-05-06 03:10

### Added

- Dynamic asset precision fetching from Hyperliquid API
- Added asset metadata retrieval during market maker initialization
- Enhanced logging for size and price precision information
- Added price precision handling with symbol-specific decimal places

### Fixed

- Fixed order size precision issues causing "Order has invalid size" errors
- Fixed order price precision issues causing "Order has invalid price" errors
- Fixed configuration updates not being applied to the underlying market maker parameters
- Corrected type mismatch between usize and u32 for decimal precision
- Properly truncate order sizes and prices to respect exchange requirements
- Applied precision handling to both market and limit orders

### Technical

- Improved error resilience with fallback precision values when API data is unavailable
- Added detailed logging for precision-related operations
- Implemented separate methods for size and price precision handling
- Enhanced configuration update process with proper parameter propagation

## [0.1.2] - 2025-05-05 14:20

### Added

- Restructured main.rs to properly handle different service modes
- Added dedicated ConfigService container to docker-compose.yml

### Fixed

- Fixed mode handling in main.rs to respect the single responsibility principle
- Corrected MODE environment variable values in docker-compose.yml
- Fixed scanner mutability issue in SymbolScanner service
- Improved service dependency management in docker-compose.yml

### Architecture

- Implemented proper microservice architecture with separate containers for each component
- Enhanced inter-service communication via Redis for better isolation
- Made market maker service dependent on config and position services

## [0.1.1] - 2025-04-28 12:30

### Added

- Environment variable support for sensitive configuration
  - Added `HYPERLIQUID_WALLET_KEY` for wallet private key
  - Sample environment file template (env.sample)
  - Automated .env file creation in build.sh

### Security

- Removed hardcoded wallet key from source code
- Added .env to .dockerignore to prevent leaking credentials
- Better error handling for missing or invalid wallet keys

## [0.1.0] - 2025-04-28 11:54

### Added

- Project structure and Docker environment setup
- Redis integration with proper error handling and connection management
- Symbol Scanner service for discovering available trading tokens
- Enhanced Market Maker module extending the base Hyperliquid SDK
  - Support for multiple assets simultaneously
  - PnL tracking with stop loss functionality
  - Trailing take profit/stop loss implementation
  - Position limits in USD terms
  - Dynamic parameter adjustment
- Configuration Service for Redis-based configuration
  - Dynamic updates via "mm_config" channel
  - Configuration validation and persistence
- Position Manager for tracking trading positions
  - Publishing to "mm_position_updates" channel
  - Calculating total exposure and PnL metrics
- Dockerized setup with docker-compose
- UI skeleton structure with React
- Test scripts for sending configurations and monitoring positions

### Fixed

- Thread safety for async functions by making error types Send + Sync compatible
- Redis method calls using proper type annotations
- Hyperliquid SDK integration to match API structure

### Technical

- Extended error handling throughout the application
- Implemented thread-safe data sharing with Arc<RwLock<>>
- Used environment variables for configuration
- Improved Docker build process with multi-stage builds
