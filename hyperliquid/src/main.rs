use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use log::{info, error, warn};
use redis::Client as RedisClient;
use ethers::signers::{LocalWallet, Signer};

// Import our custom modules
use hyperliquid_market_maker::{
    EnhancedMarketMaker,
    MarketMakerConfig,
    Position,
    SymbolScanner,
    ConfigService,
    PositionManager,
    Mode
};

// This will be our main application
#[tokio::main]
async fn main() {
    env_logger::init();

    // Initialize Redis connection first as it's needed by all modes
    let redis_client = match init_redis().await {
        Ok(client) => client,
        Err(e) => {
            error!("Failed to initialize Redis: {}", e);
            return;
        }
    };

    // Run the appropriate service based on the mode
    match env::var("MODE") {
        Ok(mode) => {
            match mode.as_str() {
                "MarketMaker" => {
                    info!("Starting Hyperliquid Market Maker");
                    
                    // Get wallet private key from environment variable (only needed for MarketMaker)
                    let wallet = match init_wallet() {
                        Ok(wallet) => wallet,
                        Err(e) => {
                            error!("Wallet initialization failed: {}", e);
                            return;
                        }
                    };
                    
                    // Run market maker
                    if let Err(e) = run_market_maker_service(redis_client, wallet).await {
                        error!("Market maker service error: {}", e);
                    }
                }
                "SymbolScanner" => {
                    info!("Starting Symbol Scanner");
                    if let Err(e) = run_symbol_scanner_service(redis_client).await {
                        error!("Symbol scanner service error: {}", e);
                    }
                }
                "ConfigService" => {
                    info!("Starting Config Service");
                    if let Err(e) = run_config_service(redis_client).await {
                        error!("Config service error: {}", e);
                    }
                }
                "PositionManager" => {
                    info!("Starting Position Manager");
                    if let Err(e) = run_position_manager_service(redis_client).await {
                        error!("Position manager service error: {}", e);
                    }
                }
                _ => {
                    error!("Invalid mode: {}", mode);
                }
            }
        }
        Err(e) => {
            error!("Failed to get MODE environment variable: {}", e);
        }
    }
}

// Initialize wallet from environment variable
fn init_wallet() -> Result<LocalWallet, Box<dyn std::error::Error + Send + Sync>> {
    let wallet_key = env::var("HYPERLIQUID_WALLET_KEY")
        .map_err(|_| {
            error!("HYPERLIQUID_WALLET_KEY environment variable not set");
            error!("Please set the wallet private key as HYPERLIQUID_WALLET_KEY environment variable");
            error!("For development only, you can use the sample test key:");
            error!("e908f86dbb4d55ac876378565aafeabc187f6690f046459397b17d9b9a19688e");
            "Wallet key not set"
        })?;
    
    let wallet: LocalWallet = wallet_key.parse()
        .map_err(|e| {
            error!("Invalid wallet private key: {}", e);
            Box::new(e) as Box<dyn std::error::Error + Send + Sync>
        })?;
    
    info!("Using wallet address: {}", wallet.address());
    Ok(wallet)
}

// Initialize Redis connection
async fn init_redis() -> Result<RedisClient, Box<dyn std::error::Error + Send + Sync>> {
    let redis_url = env::var("REDIS_URL").unwrap_or_else(|_| "redis://redis:6379".to_string());
    info!("Connecting to Redis at {}", redis_url);
    
    let client = redis::Client::open(redis_url)?;
    
    // Test connection
    let mut conn = client.get_async_connection().await?;
    redis::cmd("PING").query_async::<_, ()>(&mut conn).await?;
    info!("Successfully connected to Redis");
    
    Ok(client)
}

// Run the symbol scanner service
async fn run_symbol_scanner_service(redis_client: RedisClient) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Shared state for symbol metrics
    let symbol_metrics = Arc::new(RwLock::new(HashMap::new()));
    
    // Create and start the scanner
    let mut scanner = SymbolScanner::new(
        redis_client.clone(),
        symbol_metrics,
        Duration::from_secs(3600), // 1 hour
        10, // Top 10 symbols
        true, // Use testnet
    ).await?;
    
    scanner.start().await?;
    
    // Keep the task running
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

// Run the config service
async fn run_config_service(redis_client: RedisClient) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Shared state for configs
    let configs = Arc::new(RwLock::new(HashMap::new()));
    
    // Create the config service
    let config_service = ConfigService::new(
        redis_client,
        configs,
        "mm_config".to_string(),
    );
    
    // Load any stored configurations
    if let Err(e) = config_service.load_stored_configs().await {
        error!("Failed to load stored configurations: {}", e);
    }
    
    // Start listening for configuration updates
    config_service.start().await?;
    
    // Keep the task running
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

// Run the position manager service
async fn run_position_manager_service(redis_client: RedisClient) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Shared state for positions
    let positions = Arc::new(RwLock::new(HashMap::new()));
    
    // Create the position manager
    let position_manager = PositionManager::new(
        redis_client,
        positions,
        Duration::from_secs(1), // Update every second
        "mm_position_updates".to_string(),
    );
    
    // Start the position manager
    position_manager.start().await?;
    
    // Keep the task running (should never return from start() in normal operation)
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

// Run the market making service
async fn run_market_maker_service(redis_client: RedisClient, wallet: LocalWallet) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Shared state
    let configs = Arc::new(RwLock::new(HashMap::new()));
    let positions = Arc::new(RwLock::new(HashMap::new()));
    
    // Start the configuration service (market maker needs this component)
    let config_service_configs = configs.clone();
    let redis_config = redis_client.clone();
    tokio::spawn(async move {
        let config_service = ConfigService::new(
            redis_config,
            config_service_configs,
            "mm_config".to_string(),
        );
        
        // Load any stored configurations
        if let Err(e) = config_service.load_stored_configs().await {
            error!("Failed to load stored configurations: {}", e);
        }
        
        // Start listening for configuration updates
        if let Err(e) = config_service.start().await {
            error!("Configuration service error: {}", e);
        }
    });
    
    // Start position manager (market maker needs this component)
    let position_manager_positions = positions.clone();
    let redis_position = redis_client.clone();
    tokio::spawn(async move {
        let position_manager = PositionManager::new(
            redis_position,
            position_manager_positions,
            Duration::from_secs(1), // Update every second
            "mm_position_updates".to_string(),
        );
        
        if let Err(e) = position_manager.start().await {
            error!("Position manager error: {}", e);
        }
    });
    
    // Run the market making engine with the Redis client
    run_market_making_engine(configs, positions, wallet, redis_client.clone()).await?;
    
    // Keep the task running
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

// Run the market making engine
async fn run_market_making_engine(
    configs: Arc<RwLock<HashMap<String, MarketMakerConfig>>>, 
    positions: Arc<RwLock<HashMap<String, Position>>>,
    wallet: LocalWallet,
    redis_client: RedisClient,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    info!("Starting Market Making Engine");
    
    // Map of active market makers by symbol
    let mut market_makers: HashMap<String, Arc<RwLock<EnhancedMarketMaker>>> = HashMap::new();
    
    // Track the previous configuration to detect changes
    let mut previous_configs: HashMap<String, MarketMakerConfig> = HashMap::new();
    
    loop {
        // Get current configs
        let configs_read = configs.read().await;
        info!("Checking for config updates. Current config count: {}", configs_read.len());
        
        // Check for new configs or updates
        for (symbol, config) in configs_read.iter() {
            // Check if this is a new config or an update to an existing one
            let is_new = !previous_configs.contains_key(symbol);
            let is_update = if let Some(prev_config) = previous_configs.get(symbol) {
                // Compare configuration values to see if anything has changed
                prev_config.daily_return_bps != config.daily_return_bps ||
                prev_config.notional_per_side != config.notional_per_side ||
                prev_config.daily_pnl_stop_loss != config.daily_pnl_stop_loss ||
                prev_config.trailing_take_profit != config.trailing_take_profit ||
                prev_config.trailing_stop_loss != config.trailing_stop_loss ||
                prev_config.hedge_only_mode != config.hedge_only_mode ||
                prev_config.force_quote_refresh_interval != config.force_quote_refresh_interval ||
                prev_config.max_long_usd != config.max_long_usd ||
                prev_config.max_short_usd != config.max_short_usd ||
                prev_config.enable_trading != config.enable_trading
            } else {
                false
            };
            
            if is_new {
                info!("Found new configuration for symbol: {}", symbol);
                info!("Config details: daily_return_bps={}, notional_per_side={}, interval={}", 
                    config.daily_return_bps, config.notional_per_side, config.force_quote_refresh_interval);
                    
                // Create a new market maker for this symbol
                info!("Creating new market maker for {}", symbol);
                
                match EnhancedMarketMaker::new(config.clone(), wallet.clone(), redis_client.clone()).await {
                    Ok(market_maker) => {
                        // Store the market maker in a shareable container
                        let mm = Arc::new(RwLock::new(market_maker));
                        
                        // Start the market maker in a separate task
                        let symbol_clone = symbol.clone();
                        let positions_clone = positions.clone();
                        let mm_clone = mm.clone();
                        
                        tokio::spawn(async move {
                            // Get mutable reference through the lock
                            let mut mm_lock = mm_clone.write().await;
                            
                            if let Err(e) = mm_lock.start().await {
                                error!("Market maker error for {}: {}", symbol_clone, e);
                            }
                        });
                        
                        market_makers.insert(symbol.clone(), mm);
                        
                        // Store the config for future comparison
                        previous_configs.insert(symbol.clone(), config.clone());
                    },
                    Err(e) => {
                        error!("Failed to create market maker for {}: {}", symbol, e);
                    }
                }
            } else if is_update {
                info!("Found updated configuration for symbol: {}", symbol);
                info!("New config values: daily_return_bps={}, notional_per_side={}, interval={}", 
                    config.daily_return_bps, config.notional_per_side, config.force_quote_refresh_interval);
                
                // Debug dump of the full config to verify nothing is being lost
                info!("FULL CONFIG UPDATE - Symbol: {}, daily_return_bps: {}, notional_per_side: {}, hedge_mode: {}, max_long: {}, max_short: {}, enable_trading: {}",
                     config.symbol, config.daily_return_bps, config.notional_per_side, 
                     config.hedge_only_mode, config.max_long_usd, config.max_short_usd, config.enable_trading);
                
                // Update existing market maker config
                if let Some(market_maker) = market_makers.get(symbol) {
                    info!("Updating existing market maker for {}", symbol);
                    let mut mm = market_maker.write().await;
                    mm.update_config(config.clone());
                    
                    // Store the updated config for future comparison
                    previous_configs.insert(symbol.clone(), config.clone());
                } else {
                    warn!("Found config update for {} but no market maker instance exists", symbol);
                }
            }
        }
        
        // Wait before checking for updates again
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
