use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};
use serde::{Serialize, Deserialize};
use log::{info, error, warn, debug};
use ethers::signers::{LocalWallet, Signer};
use std::time::{SystemTime, UNIX_EPOCH};
use futures::StreamExt;
use tokio::sync::mpsc::UnboundedSender;
use std::sync::atomic::{AtomicBool, Ordering};
use redis::{Client as RedisClient, AsyncCommands};

use hyperliquid_rust_sdk::{
    MarketMaker as SdkMarketMaker,
    MarketMakerInput,
    MarketMakerRestingOrder,
    BaseUrl,
    ExchangeClient,
    InfoClient,
    bps_diff,
    truncate_float,
    EPSILON,
    ClientCancelRequest,
    ClientOrderRequest,
    ClientOrder,
    ClientLimit,
    MarketOrderParams,
    ExchangeResponseStatus,
    ExchangeDataStatus,
    UserStateResponse,
    Message,
    Subscription,
    Meta,
};

/// Structure for a single quote level in the market maker
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuoteLevel {
    pub level: u16,                // Level number (1 is closest to mid price)
    pub spread_multiplier: f64,    // Multiplier on the base spread (1.0 = base spread)
    pub size_multiplier: f64,      // Multiplier on the base size (1.0 = base size)
}

/// Configuration for the enhanced market maker
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketMakerConfig {
    pub symbol: String,
    pub daily_return_bps: u16,
    pub notional_per_side: f64,
    pub daily_pnl_stop_loss: f64,
    pub trailing_take_profit: f64,
    pub trailing_stop_loss: f64,
    pub hedge_only_mode: bool,
    pub force_quote_refresh_interval: u64,
    pub max_long_usd: f64,
    pub max_short_usd: f64,
    #[serde(default = "default_enable_trading")]
    pub enable_trading: bool,
    #[serde(default = "default_quote_levels")]
    pub quote_levels: Vec<QuoteLevel>,
    pub vault_address: Option<String>,
}

/// Default value for enable_trading is true
fn default_enable_trading() -> bool {
    true
}

/// Default quote levels is a single level at 1.0x spread and 1.0x size
fn default_quote_levels() -> Vec<QuoteLevel> {
    vec![QuoteLevel {
        level: 1,
        spread_multiplier: 1.0,
        size_multiplier: 1.0,
    }]
}

/// Helper function to check if two quote level vectors are different
fn quote_levels_changed(a: &[QuoteLevel], b: &[QuoteLevel]) -> bool {
    // First check if lengths are different
    if a.len() != b.len() {
        return true;
    }

    // Then check each level
    for (i, level_a) in a.iter().enumerate() {
        let level_b = &b[i];
        if level_a.level != level_b.level 
            || (level_a.spread_multiplier - level_b.spread_multiplier).abs() > EPSILON
            || (level_a.size_multiplier - level_b.size_multiplier).abs() > EPSILON {
            return true;
        }
    }
    
    false
}

/// Helper function to compare two Option<String> values
fn option_string_changed(a: &Option<String>, b: &Option<String>) -> bool {
    match (a, b) {
        (Some(a_val), Some(b_val)) => a_val != b_val,
        (None, None) => false,
        _ => true, // One is Some and one is None
    }
}

/// Add this new struct for shared configuration data
#[derive(Clone)]
struct SharedMarketMakerParams {
    config: MarketMakerConfig,
    half_spread: u16,
    target_liquidity: f64,
    max_position_size: f64,
    price_decimals: u32,
    last_quote_time: u64,
    needs_refresh: bool,
}

/// Enhanced version of the SDK's MarketMakerRestingOrder that includes the order side
#[derive(Clone, Debug)]
pub struct EnhancedRestingOrder {
    pub oid: u64,
    pub position: f64,
    pub price: f64,
    pub is_bid: bool,  // True for buy orders, false for sell orders
}

/// Enhanced market maker that builds upon the SDK's basic market maker
pub struct EnhancedMarketMaker {
    pub config: MarketMakerConfig,
    pub market_maker: SdkMarketMaker,
    pub positions: HashMap<String, Position>,
    pub current_mid_price: f64,
    pub daily_pnl: f64,
    pub realized_daily_pnl: f64, // Track realized PnL separately
    pub highest_pnl: f64,
    pub lowest_pnl: f64,
    last_quote_time: u64,
    day_start_timestamp: u64, // Track when the trading day started
    asset_meta: Option<Meta>, // Store asset metadata retrieved from the API
    redis_client: RedisClient, // Redis client for config updates
}

/// Position information
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Position {
    pub symbol: String,
    pub size: f64,
    pub entry_price: f64,
    pub current_price: f64,
    pub unrealized_pnl: f64,
    pub notional_usd: f64,
}

impl EnhancedMarketMaker {
    /// Create a new enhanced market maker
    pub async fn new(config: MarketMakerConfig, wallet: LocalWallet, redis_client: RedisClient) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Fetch asset metadata first to get precision information
        let base_url = if cfg!(feature = "testnet") { Some(BaseUrl::Testnet) } else { Some(BaseUrl::Mainnet) };
        let info_client = InfoClient::new(None, base_url.clone()).await?;
        let asset_meta = match info_client.meta().await {
            Ok(meta) => {
                info!("Successfully retrieved asset metadata from Hyperliquid");
                Some(meta)
            },
            Err(e) => {
                warn!("Failed to retrieve asset metadata: {}", e);
                None
            }
        };
        
        // Get the appropriate price precision for this symbol from hardcoded values
        // We can't use self.get_price_decimals_for_symbol yet as we're still constructing self
        let price_decimals = match config.symbol.as_str() {
            "BTC" => 0,
            "ETH" => 2,
            "SOL" => 3,
            "AVAX" => 3,
            "MATIC" => 4,
            "DOGE" => 6,
            "SHIB" => 8,
            _ => 2,
        };
        
        // Create the basic market maker input from our enhanced config with correct precision
        let market_maker_input = MarketMakerInput {
            asset: config.symbol.clone(),
            target_liquidity: config.notional_per_side,
            half_spread: config.daily_return_bps / 365, // Convert daily to per-trade basis approx
            max_bps_diff: 2, // Default, will be recalculated dynamically
            max_absolute_position_size: calculate_max_position(&config),
            decimals: price_decimals, // Use the symbol's price precision
            wallet: wallet.clone(),
        };

        // Create the base market maker from SDK
        let market_maker = SdkMarketMaker::new(market_maker_input).await;
        
        info!("Created market maker for {} with price precision of {} decimals", 
            config.symbol, price_decimals);
        
        Ok(EnhancedMarketMaker {
            config,
            market_maker,
            positions: HashMap::new(),
            current_mid_price: 0.0,
            daily_pnl: 0.0,
            realized_daily_pnl: 0.0,
            highest_pnl: 0.0,
            lowest_pnl: 0.0,
            last_quote_time: 0,
            day_start_timestamp: current_timestamp_ms(),
            asset_meta,
            redis_client,
        })
    }

    /// Start the market maker
    pub async fn start(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("Starting enhanced market maker for {}", self.config.symbol);
        
        // Initialize position tracking
        self.positions.insert(
            self.config.symbol.clone(),
            Position {
                symbol: self.config.symbol.clone(),
                size: 0.0,
                entry_price: 0.0,
                current_price: 0.0,
                unrealized_pnl: 0.0,
                notional_usd: 0.0,
            },
        );
        
        // Instead of using the base market maker's start method,
        // we implement our own logic to handle our enhanced features
        self.run_market_making_loop().await
    }
    
    /// The main market making loop with enhanced features
    async fn run_market_making_loop(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Subscribe to necessary data feeds and handle events with our enhanced logic
        info!("Market making loop started for {}", self.config.symbol);
        
        // Create clients for market data and order execution
        let base_url = if cfg!(feature = "testnet") { Some(BaseUrl::Testnet) } else { Some(BaseUrl::Mainnet) };
        
        // Get wallet from the MarketMaker
        let wallet = self.market_maker.exchange_client.wallet.clone();
        
        // Convert vault address String to H160 if present
        let vault_address = if let Some(addr_str) = &self.config.vault_address {
            use ethers::types::H160;
            use std::str::FromStr;
            
            match H160::from_str(addr_str) {
                Ok(h160_addr) => {
                    info!("Using vault address: {}", addr_str);
                    Some(h160_addr)
                },
                Err(e) => {
                    error!("Invalid vault address format: {}. Error: {}", addr_str, e);
                    None
                }
            }
        } else {
            None
        };
        
        // Create new exchange client with the correct parameters
        let exchange_client = ExchangeClient::new(
            None, 
            wallet.clone(), 
            base_url.clone(), 
            None, 
            vault_address
        ).await?;
        
        let mut info_client = InfoClient::new(None, base_url).await?;
        
        // Setup channels for messaging
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        
        // Subscribe to AllMids to get price updates
        info_client.subscribe(Subscription::AllMids, sender.clone()).await?;
        
        // Get initial user state
        let user_address = wallet.address();
        let user_state = info_client.user_state(user_address).await?;
        let symbol_clone = self.config.symbol.clone();
        self.update_position_from_user_state(&user_state, &symbol_clone).await?;
        
        // Track order status using EnhancedRestingOrder instead of MarketMakerRestingOrder
        let active_orders_arc = Arc::new(Mutex::new(HashMap::<u64, EnhancedRestingOrder>::new()));
        
        // Initialize shared parameters - avoid storing references to self fields
        let shared_params = Arc::new(Mutex::new(SharedMarketMakerParams {
            config: self.config.clone(),
            half_spread: self.market_maker.half_spread,
            target_liquidity: self.market_maker.target_liquidity,
            max_position_size: self.market_maker.max_absolute_position_size,
            price_decimals: self.market_maker.decimals,
            last_quote_time: self.last_quote_time,
            needs_refresh: false,
        }));
        
        // Setup slow_path timer for config updates
        let symbol_for_redis = self.config.symbol.clone();
        let redis_client_clone = self.redis_client.clone();
        
        // The exchange client needs to be shared between the main loop and slow_path
        let exchange_client_arc = Arc::new(Mutex::new(exchange_client));
        let exchange_client_clone = exchange_client_arc.clone();
        
        // Share the active orders map between main loop and slow_path
        let active_orders_clone = active_orders_arc.clone();
        
        // Share parameters for the slow path
        let params_clone = shared_params.clone();
        
        // Spawn the slow_path task to check Redis for config updates
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            
            loop {
                interval.tick().await;
                
                // Check Redis for updated config
                if let Err(e) = slow_path_check_config(
                    &redis_client_clone, 
                    &symbol_for_redis, 
                    params_clone.clone(),
                    &exchange_client_clone,
                    &active_orders_clone
                ).await {
                    error!("Error in slow_path config check: {}", e);
                }
            }
        });
        
        // Use the mutex-protected data in the main loop - avoid holding locks across await points
        while let Some(message) = receiver.recv().await {
            // Update our local params from the shared object - Use a separate block to ensure lock is released
            let force_refresh = {
                let shared = shared_params.lock().await;
                
                let needs_update = self.config.daily_return_bps != shared.config.daily_return_bps ||
                                   self.config.notional_per_side != shared.config.notional_per_side ||
                                   self.config.force_quote_refresh_interval != shared.config.force_quote_refresh_interval ||
                                   self.config.enable_trading != shared.config.enable_trading ||
                                   self.config.daily_pnl_stop_loss != shared.config.daily_pnl_stop_loss ||
                                   self.config.trailing_take_profit != shared.config.trailing_take_profit ||
                                   self.config.trailing_stop_loss != shared.config.trailing_stop_loss ||
                                   self.config.hedge_only_mode != shared.config.hedge_only_mode ||
                                   self.config.max_long_usd != shared.config.max_long_usd ||
                                   self.config.max_short_usd != shared.config.max_short_usd ||
                                   quote_levels_changed(&self.config.quote_levels, &shared.config.quote_levels) ||
                                   option_string_changed(&self.config.vault_address, &shared.config.vault_address);
                
                if needs_update {
                    info!("Main loop detected config change in one or more parameters. Updating configuration.");
                    info!("Old config: daily_return_bps={}, notional={}, stop_loss={}, max_long={}, max_short={}, enable_trading={}", 
                          self.config.daily_return_bps, self.config.notional_per_side, 
                          self.config.daily_pnl_stop_loss, self.config.max_long_usd, 
                          self.config.max_short_usd, self.config.enable_trading);
                    info!("New config: daily_return_bps={}, notional={}, stop_loss={}, max_long={}, max_short={}, enable_trading={}", 
                          shared.config.daily_return_bps, shared.config.notional_per_side, 
                          shared.config.daily_pnl_stop_loss, shared.config.max_long_usd, 
                          shared.config.max_short_usd, shared.config.enable_trading);
                    
                    // Update internal configuration
                    self.config = shared.config.clone();
                    self.market_maker.half_spread = shared.half_spread;
                    self.market_maker.target_liquidity = shared.target_liquidity;
                    self.market_maker.max_absolute_position_size = shared.max_position_size;
                    self.market_maker.decimals = shared.price_decimals;
                    
                    shared.needs_refresh
                } else {
                    false
                }
            }; // Lock is released here
            
            // Apply force_refresh outside of the lock scope
            if force_refresh {
                self.last_quote_time = 0; // Force refresh
                info!("Forcing quote refresh due to configuration change");
            }
            
            match message {
                Message::AllMids(all_mids) => {
                    // Get current timestamp in ms
                    let now = current_timestamp_ms();
                    
                    // Extract the mid price for our symbol
                    let all_mids = all_mids.data.mids;
                    if let Some(mid_price_str) = all_mids.get(&self.config.symbol) {
                        if let Ok(mid_price) = mid_price_str.parse::<f64>() {
                            self.current_mid_price = mid_price;
                            
                            // Update position data in a separate block to minimize lock time
                            {
                                if let Ok(user_state) = info_client.user_state(user_address).await {
                                    let symbol_clone = self.config.symbol.clone();
                                    self.update_position_from_user_state(&user_state, &symbol_clone).await?;
                                }
                            }
                            
                            // Check if we need to refresh quotes based on the config interval
                            let should_refresh = now - self.last_quote_time > self.config.force_quote_refresh_interval;
                            
                            if should_refresh {
                                // Update timestamp before any awaits to avoid duplicate refreshes
                                self.last_quote_time = now;
                                
                                // Skip quoting if we've hit risk limits
                                if self.check_risk_limits().await? {
                                    // Log all the values in config
                                    info!("Config values - daily_return_bps: {}, notional_per_side: {}, max_long_usd: {}, max_short_usd: {}, force_quote_refresh_interval: {}, enable_trading: {}, quote_levels: {}",
                                        self.config.daily_return_bps, self.config.notional_per_side, self.config.max_long_usd, self.config.max_short_usd, 
                                        self.config.force_quote_refresh_interval, self.config.enable_trading, self.config.quote_levels.len());

                                    // Get the appropriate price precision for this symbol
                                    let price_decimals = self.get_price_decimals_for_symbol(&self.config.symbol);
                                    info!("Using {} decimal precision for {} prices", price_decimals, self.config.symbol);
                                    
                                    // Get current position and limits
                                    let position_size = self.get_position_size(&self.config.symbol);
                                    let max_long = self.config.max_long_usd / self.current_mid_price;
                                    let max_short = self.config.max_short_usd / self.current_mid_price;
                                    
                                    // Get size precision for this symbol
                                    let size_decimals = self.get_size_decimals_for_symbol(&self.config.symbol);
                                    info!("Using {} decimal precision for {} order sizes", size_decimals, self.config.symbol);
                                    
                                    // Calculate desired orders for each level
                                    let mut desired_orders: Vec<(bool, f64, f64)> = Vec::new(); // (is_bid, price, size)
                                    
                                    // Base half spread calculation (in price units, not BPS)
                                    let base_half_spread = self.current_mid_price * self.config.daily_return_bps as f64 / 10000.0;
                                    
                                    // Calculate orders for each level
                                    for level in &self.config.quote_levels {
                                        // Calculate spread for this level
                                        let level_spread = base_half_spread * level.spread_multiplier;
                                        
                                        // Calculate bid and ask prices for this level
                                        let bid_price = truncate_float(
                                            self.current_mid_price - level_spread,
                                            price_decimals,
                                            false
                                        );
                                        
                                        let ask_price = truncate_float(
                                            self.current_mid_price + level_spread,
                                            price_decimals,
                                            false
                                        );
                                        
                                        // Calculate sizes for this level
                                        let base_notional = self.config.notional_per_side;
                                        let level_notional = base_notional * level.size_multiplier;
                                        
                                        // Adjust sizes based on current position
                                        let raw_bid_size = if position_size >= max_long {
                                            0.0 // Don't buy more if we're at max long
                                        } else {
                                            level_notional / bid_price
                                        };
                                        
                                        let raw_ask_size = if position_size <= -max_short {
                                            0.0 // Don't sell more if we're at max short
                                        } else {
                                            level_notional / ask_price
                                        };
                                        
                                        // Truncate sizes to appropriate precision
                                        let bid_size = truncate_float(raw_bid_size, size_decimals, false);
                                        let ask_size = truncate_float(raw_ask_size, size_decimals, false);
                                        
                                        info!("Level {}: Bid: {} @ {}, Ask: {} @ {} (spread multiplier: {}, size multiplier: {})",
                                            level.level, bid_size, bid_price, ask_size, ask_price, 
                                            level.spread_multiplier, level.size_multiplier);
                                        
                                        // Add the desired orders to our list
                                        if bid_size > EPSILON {
                                            desired_orders.push((true, bid_price, bid_size));
                                        }
                                        
                                        if ask_size > EPSILON {
                                            desired_orders.push((false, ask_price, ask_size));
                                        }
                                    }
                                    
                                    // Only continue if trading is enabled
                                    if self.config.enable_trading {
                                        // Get current active orders
                                        let active_orders = active_orders_arc.lock().await.clone();
                                        
                                        // Separate orders for different operations
                                        let mut orders_to_cancel: Vec<u64> = Vec::new();
                                        let mut orders_to_place: Vec<ClientOrderRequest> = Vec::new();
                                        
                                        // Track order details for order placement
                                        let mut order_details: Vec<(bool, f64, f64)> = Vec::new();
                                        
                                        // For each desired order, decide if we need to place new, modify existing, or keep as is
                                        for (is_bid, price, size) in &desired_orders {
                                            // Look for an existing order of the same side
                                            let existing_order = active_orders.values()
                                                .find(|o| o.is_bid == *is_bid);
                                                
                                            if let Some(order) = existing_order {
                                                // Check if we should modify this order (price or size changed)
                                                let price_changed = bps_diff(order.price, *price) > 2; // Allow 2 bps difference
                                                let size_changed = (order.position - *size).abs() > EPSILON;
                                                
                                                if price_changed || size_changed {
                                                    // Create a new order with the updated parameters
                                                    // The order will cancel and replace the existing one
                                                    let order_req = ClientOrderRequest {
                                                        asset: self.config.symbol.clone(),
                                                        is_buy: *is_bid,
                                                        reduce_only: false,
                                                        limit_px: *price,
                                                        sz: *size,
                                                        cloid: None,
                                                        order_type: ClientOrder::Limit(ClientLimit {
                                                            tif: "Alo".to_string(),
                                                        }),
                                                    };
                                                    
                                                    // Cancel the old order
                                                    orders_to_cancel.push(order.oid);
                                                    
                                                    // Place the new order
                                                    orders_to_place.push(order_req);
                                                    order_details.push((*is_bid, *price, *size));
                                                    
                                                    info!("Will cancel and replace {} order: id={}, from {}@{} to {}@{}", 
                                                        if *is_bid { "bid" } else { "ask" }, 
                                                        order.oid, order.position, order.price, size, price);
                                                } else {
                                                    info!("Keeping existing {} order: id={}, {}@{}", 
                                                        if *is_bid { "bid" } else { "ask" }, 
                                                        order.oid, order.position, order.price);
                                                }
                                            } else {
                                                // Create a new order
                                                let order = ClientOrderRequest {
                                                    asset: self.config.symbol.clone(),
                                                    is_buy: *is_bid,
                                                    reduce_only: false,
                                                    limit_px: *price,
                                                    sz: *size,
                                                    cloid: None,
                                                    order_type: ClientOrder::Limit(ClientLimit {
                                                        tif: "Alo".to_string(),
                                                    }),
                                                };
                                                
                                                orders_to_place.push(order);
                                                order_details.push((*is_bid, *price, *size));
                                                info!("Will place new {} order: {}@{}", 
                                                    if *is_bid { "bid" } else { "ask" }, size, price);
                                            }
                                        }
                                        
                                        // For each active order, if it's not in our desired orders, cancel it
                                        for (oid, order) in &active_orders {
                                            let order_still_needed = desired_orders.iter()
                                                .any(|(is_bid, _, _)| *is_bid == order.is_bid);
                                                
                                            if !order_still_needed {
                                                orders_to_cancel.push(*oid);
                                                info!("Will cancel {} order: id={}, {}@{}", 
                                                    if order.is_bid { "bid" } else { "ask" }, 
                                                    order.oid, order.position, order.price);
                                            }
                                        }
                                        
                                        // Get the exchange client lock for operations
                                        let exchange_client_lock = exchange_client_arc.lock().await;
                                        
                                        // 1. Execute cancellations if needed
                                        if !orders_to_cancel.is_empty() {
                                            let cancel_requests = orders_to_cancel.iter()
                                                .map(|oid| ClientCancelRequest {
                                                    asset: self.config.symbol.clone(),
                                                    oid: *oid,
                                                })
                                                .collect::<Vec<_>>();
                                                
                                            match exchange_client_lock.bulk_cancel(cancel_requests, None).await {
                                                Ok(_) => {
                                                    info!("Successfully cancelled {} orders", orders_to_cancel.len());
                                                    
                                                    // Remove cancelled orders from the tracking map
                                                    let mut active_orders_lock = active_orders_arc.lock().await;
                                                    for oid in orders_to_cancel {
                                                        active_orders_lock.remove(&oid);
                                                    }
                                                },
                                                Err(e) => {
                                                    warn!("Failed to cancel orders: {}", e);
                                                }
                                            }
                                        }
                                        
                                        // 2. Place new orders (after cancellations)
                                        if !orders_to_place.is_empty() {
                                            // Log vault address status before placing orders
                                            {
                                                let client = exchange_client_lock.wallet.address();
                                                let has_vault = exchange_client_lock.vault_address.is_some();
                                                if has_vault {
                                                    info!("Placing orders with wallet {} and VAULT ADDRESS: {:?}", 
                                                         client, exchange_client_lock.vault_address);
                                                } else {
                                                    info!("Placing orders with wallet {} (NO VAULT ADDRESS)", client);
                                                }
                                            }
                                            
                                            match exchange_client_lock.bulk_order(orders_to_place, None).await {
                                                Ok(response) => {
                                                    if let ExchangeResponseStatus::Ok(ok_response) = response {
                                                        if let Some(data) = ok_response.data {
                                                            for (index, status) in data.statuses.into_iter().enumerate() {
                                                                match status {
                                                                    ExchangeDataStatus::Resting(order) => {
                                                                        // Get a separate lock for updating orders
                                                                        let mut active_orders_lock = active_orders_arc.lock().await;
                                                                        
                                                                        // Get the corresponding order details from the index
                                                                        if let Some(&(is_bid, price, size)) = order_details.get(index) {
                                                                            let order_type = if is_bid { "bid" } else { "ask" };
                                                                        
                                                                            active_orders_lock.insert(order.oid, EnhancedRestingOrder {
                                                                                oid: order.oid,
                                                                                position: size,
                                                                                price,
                                                                                is_bid,
                                                                            });
                                                                            
                                                                            info!("Placed {} order: id={}, size={}, price={}, tif=Alo", 
                                                                                order_type, order.oid, size, price);
                                                                        } else {
                                                                            warn!("Received order response with no matching details: {:?}", order);
                                                                        }
                                                                    },
                                                                    _ => {warn!("Unknown order status: {:?}", status)},
                                                                }
                                                            }
                                                        }
                                                    } else {
                                                        warn!("Bulk order placement failed: {:?}", response);
                                                    }
                                                },
                                                Err(e) => {
                                                    warn!("Failed to place bulk orders: {}", e);
                                                }
                                            }
                                        }
                                    } else {
                                        // If trading is disabled, just cancel all existing orders
                                        let active_orders = active_orders_arc.lock().await.clone();
                                        
                                        if !active_orders.is_empty() {
                                            let cancel_requests = active_orders.keys()
                                                .map(|oid| ClientCancelRequest {
                                                    asset: self.config.symbol.clone(),
                                                    oid: *oid,
                                                })
                                                .collect::<Vec<_>>();
                                                
                                            let exchange_client_lock = exchange_client_arc.lock().await;
                                            
                                            if let Err(e) = exchange_client_lock.bulk_cancel(cancel_requests, None).await {
                                                warn!("Failed to cancel orders: {}", e);
                                            } else {
                                                // Clear the order tracking map
                                                let mut active_orders_lock = active_orders_arc.lock().await;
                                                active_orders_lock.clear();
                                                info!("Trading is disabled - cancelled all {} existing orders", active_orders.len());
                                            }
                                        } else {
                                            info!("Trading is disabled for {}. No orders to cancel.", self.config.symbol);
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                // Handle other message types if needed
                _ => {},
            }
            
            // Sleep a bit to not overwhelm the CPU
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
        
        Ok(())
    }
    
    /// Check risk limits like stop loss and take profit
    /// Returns true if we should continue trading, false if we need to stop
    async fn check_risk_limits(&mut self) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        // Skip if we don't have a position or PnL data
        if self.positions.is_empty() {
            return Ok(true);
        }
        
        // Check if we need to reset daily PnL (new trading day)
        self.check_and_reset_daily_pnl();
        
        // Calculate total PnL (realized + unrealized)
        let unrealized_pnl = self.get_total_unrealized_pnl();
        let total_daily_pnl = self.realized_daily_pnl + unrealized_pnl;
        self.daily_pnl = total_daily_pnl;
        
        // Check daily stop loss
        if total_daily_pnl <= -self.config.daily_pnl_stop_loss {
            warn!("Daily stop loss hit for {}. Total PnL: {} (Realized: {}, Unrealized: {})", 
                self.config.symbol, total_daily_pnl, self.realized_daily_pnl, unrealized_pnl);
                
            // Close all positions
            self.close_all_positions().await?;
            return Ok(false);
        }
        
        // Check trailing take profit
        if total_daily_pnl > self.highest_pnl {
            self.highest_pnl = total_daily_pnl;
        }
        
        let take_profit_threshold = self.highest_pnl * (1.0 - self.config.trailing_take_profit);
        if total_daily_pnl < take_profit_threshold && self.highest_pnl > 0.0 {
            info!("Trailing take profit triggered for {}. Current PnL: {}, Highest: {}", 
                self.config.symbol, total_daily_pnl, self.highest_pnl);
                
            // Close all positions
            self.close_all_positions().await?;
            return Ok(false);
        }
        
        // Check trailing stop loss
        if total_daily_pnl < self.lowest_pnl {
            self.lowest_pnl = total_daily_pnl;
        }
        
        let stop_loss_threshold = self.lowest_pnl * (1.0 + self.config.trailing_stop_loss);
        if total_daily_pnl > stop_loss_threshold && self.lowest_pnl < 0.0 {
            info!("Trailing stop loss triggered for {}. Current PnL: {}, Lowest: {}", 
                self.config.symbol, total_daily_pnl, self.lowest_pnl);
                
            // Close all positions
            self.close_all_positions().await?;
            return Ok(false);
        }
        
        Ok(true)
    }
    
    /// Close all positions for the symbol
    async fn close_all_positions(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("Closing all positions for {}", self.config.symbol);
        
        // Get position size
        let position_opt = self.positions.get(&self.config.symbol);
        if position_opt.is_none() || position_opt.unwrap().size.abs() < EPSILON {
            info!("No position to close");
            return Ok(());
        }
        
        let position = position_opt.unwrap();
        let raw_size = position.size;
        let unrealized_pnl = position.unrealized_pnl;
        
        // Create exchange client with proper initialization
        let base_url = if cfg!(feature = "testnet") { Some(BaseUrl::Testnet) } else { Some(BaseUrl::Mainnet) };
        
        // Get wallet from market maker
        let wallet = self.market_maker.exchange_client.wallet.clone();
        
        let exchange_client = ExchangeClient::new(
            None, 
            wallet.clone(), 
            base_url.clone(), 
            None, 
            None
        ).await?;
        
        // Determine side for closing (opposite of current position)
        let is_buy = raw_size < 0.0; // If we have a short position, we need to buy to close
        
        // Truncate the size to appropriate precision
        let size_decimals = self.get_size_decimals_for_symbol(&self.config.symbol);
        let truncated_size = truncate_float(raw_size.abs(), size_decimals, false);
        
        // Place market order to close the position
        info!("Placing market order to close position: {} {} @ market (raw size: {}, truncated: {})", 
            if is_buy { "Buy" } else { "Sell" }, truncated_size, raw_size.abs(), truncated_size);
        
        // Use market_open with appropriate parameters
        let params = MarketOrderParams {
            asset: &self.config.symbol,
            is_buy,
            sz: truncated_size,  // Use truncated size
            px: None,           // Use current market price
            slippage: Some(0.03), // 3% slippage
            cloid: None,
            wallet: None,       // Use default wallet
        };
        
        match exchange_client.market_open(params).await {
            Ok(response) => {
                info!("Position closed successfully: {:?}", response);
                // Update realized PnL since we've closed the position
                self.realized_daily_pnl += unrealized_pnl;
                info!("Updated realized PnL: {}", self.realized_daily_pnl);
            },
            Err(e) => {
                error!("Failed to close position: {}", e);
            }
        }
        
        Ok(())
    }
    
    /// Update position from user state
    async fn update_position_from_user_state(&mut self, user_state: &UserStateResponse, symbol: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Check if we previously had a position that's now closed
        let had_position = self.positions.contains_key(symbol);
        let previous_size = if had_position {
            self.positions.get(symbol).map(|p| p.size).unwrap_or(0.0)
        } else {
            0.0
        };
        
        // Track if we found the symbol in current positions
        let mut found_position = false;
        
        // Loop through asset positions to find our symbol
        for asset_position in &user_state.asset_positions {
            if asset_position.position.coin == symbol {
                found_position = true;
                
                // Parse position data with proper Option handling
                let size = asset_position.position.szi.parse::<f64>().unwrap_or(0.0);
                
                // Use map_or to handle Option<String> -> f64 conversion in one line
                let entry_price = asset_position.position.entry_px.as_ref().map_or::<f64, _>(0.0, |s| s.parse::<f64>().unwrap_or(0.0));
                let unrealized_pnl = asset_position.position.unrealized_pnl.as_str().parse::<f64>().unwrap_or(0.0);
                
                // Update position
                let notional_usd = self.current_mid_price * size.abs();
                
                // Check if position was reduced (partial close)
                if had_position && previous_size.abs() > size.abs() {
                    // Calculate the portion of the position that was closed
                    let closed_portion = (previous_size.abs() - size.abs()) / previous_size.abs();
                    let previous_pnl = self.positions.get(symbol).map(|p| p.unrealized_pnl).unwrap_or(0.0);
                    
                    // Add the realized portion to daily realized PnL
                    let realized_portion = previous_pnl * closed_portion;
                    self.realized_daily_pnl += realized_portion;
                    
                    info!("Position partially closed. Realized PnL: {}, Total realized: {}", 
                        realized_portion, self.realized_daily_pnl);
                }
                
                let position = Position {
                    symbol: symbol.to_string(),
                    size,
                    entry_price,
                    current_price: self.current_mid_price,
                    unrealized_pnl,
                    notional_usd,
                };
                
                self.positions.insert(symbol.to_string(), position);
                
                // Update highest/lowest PnL if needed
                // Calculate total PnL (realized + unrealized)
                let total_pnl = self.realized_daily_pnl + unrealized_pnl;
                
                if self.highest_pnl == 0.0 || total_pnl > self.highest_pnl {
                    self.highest_pnl = total_pnl;
                }
                
                if self.lowest_pnl == 0.0 || total_pnl < self.lowest_pnl {
                    self.lowest_pnl = total_pnl;
                }
            }
        }
        
        // If we had a position but didn't find it now, it must have been fully closed
        if had_position && !found_position {
            // Get the previous position's PnL and add it to realized
            let previous_pnl = self.positions.get(symbol).map(|p| p.unrealized_pnl).unwrap_or(0.0);
            self.realized_daily_pnl += previous_pnl;
            
            // Remove the position from our tracking
            self.positions.remove(symbol);
            
            info!("Position fully closed. Realized PnL: {}, Total realized: {}", 
                previous_pnl, self.realized_daily_pnl);
        }
        
        Ok(())
    }
    
    /// Get position size for a symbol
    fn get_position_size(&self, symbol: &str) -> f64 {
        self.positions.get(symbol).map(|p| p.size).unwrap_or(0.0)
    }
    
    /// Get the appropriate size precision (number of decimal places) for a symbol
    fn get_size_decimals_for_symbol(&self, symbol: &str) -> u32 {
        // Try to use the precision from the SDK's market maker if available
        // This will have been populated from the exchange meta data
        if let Some(asset_meta) = self.asset_meta.as_ref() {
            for asset in &asset_meta.universe {
                if asset.name == symbol {
                    debug!("Using API-provided precision for {}: {} decimals", symbol, asset.sz_decimals);
                    return asset.sz_decimals;
                }
            }
        }
        
        // Fallback to default values in case we couldn't get the precision from API
        let fallback_precision = match symbol {
            "BTC" => 8,  // Bitcoin typically uses 8 decimal places
            "ETH" => 6,  // Ethereum typically uses 6
            "SOL" => 4,  // Solana with 4
            "AVAX" => 4,
            "MATIC" => 2,
            "DOGE" => 2,
            "SHIB" => 0, // Whole units only
            // Add more symbols as needed
            _ => 6,      // Default to 6 decimal places for other assets
        };
        
        debug!("Using fallback precision for {}: {} decimals (API data not available)", 
            symbol, fallback_precision);
        fallback_precision
    }
    
    /// Get total unrealized PnL across all positions
    fn get_total_unrealized_pnl(&self) -> f64 {
        self.positions.values().map(|p| p.unrealized_pnl).sum()
    }
    
    /// Check if we need to reset daily PnL (new trading day)
    fn check_and_reset_daily_pnl(&mut self) {
        let now = current_timestamp_ms();
        
        // Check if it's a new day (86,400,000 ms = 24 hours)
        if now - self.day_start_timestamp > 86_400_000 {
            // Reset for new trading day
            info!("New trading day started. Resetting daily PnL tracking.");
            info!("Previous day's final PnL: {} (Realized: {}, Unrealized: {})",
                self.daily_pnl, self.realized_daily_pnl, self.get_total_unrealized_pnl());
                
            self.realized_daily_pnl = 0.0;
            self.highest_pnl = 0.0;
            self.lowest_pnl = 0.0;
            self.day_start_timestamp = now;
        }
    }
    
    /// Update market maker configuration
    pub fn update_config(&mut self, config: MarketMakerConfig) {
        info!("Updating configuration for {}", config.symbol);
        info!("New params - daily_return_bps: {}, notional_per_side: {}, interval: {}", 
            config.daily_return_bps, config.notional_per_side, config.force_quote_refresh_interval);
        
        // Log old vs new values for key parameters
        info!("Config CHANGE - daily_return_bps: {} -> {}, notional_per_side: {} -> {}", 
            self.config.daily_return_bps, config.daily_return_bps,
            self.config.notional_per_side, config.notional_per_side);
        
        // Store the new configuration
        self.config = config.clone();
        
        // Update the underlying market maker parameters
        // Get the correct price precision for this symbol
        let price_decimals = self.get_price_decimals_for_symbol(&self.config.symbol);
        
        // Update the market maker's parameters
        self.market_maker.half_spread = self.config.daily_return_bps / 365;
        self.market_maker.target_liquidity = self.config.notional_per_side;
        self.market_maker.max_absolute_position_size = calculate_max_position(&self.config);
        self.market_maker.decimals = price_decimals;
        
        info!("Market maker parameters updated - half_spread: {}, target_liquidity: {}, max_position: {}, decimals: {}", 
            self.market_maker.half_spread, self.market_maker.target_liquidity, 
            self.market_maker.max_absolute_position_size, self.market_maker.decimals);
            
        // Force quote refresh on next iteration
        self.last_quote_time = 0;
        
        // Store the updated config in Redis to ensure it's available to other instances
        let redis_clone = self.redis_client.clone();
        let config_clone = config;
        tokio::spawn(async move {
            if let Err(e) = store_config_in_redis(&redis_clone, &config_clone).await {
                error!("Failed to store configuration in Redis: {}", e);
            }
        });
    }

    /// Get the appropriate price precision (number of decimal places) for a symbol
    fn get_price_decimals_for_symbol(&self, symbol: &str) -> u32 {
        // Different assets have different price precision requirements
        // These are typically standardized by the exchange
        match symbol {
            "BTC" => 0,  // Bitcoin typically uses 2 decimal places for price ($xx,xxx.xx)
            "ETH" => 2,  // Ethereum also 2 decimals
            "SOL" => 3,  // Solana with 3
            "AVAX" => 3,
            "MATIC" => 4,
            "DOGE" => 6,
            "SHIB" => 8, // Very low priced assets need more precision
            // Add more symbols as needed
            _ => 2,      // Default to 2 decimal places for other assets
        }
    }
}

/// Calculate the maximum position size based on configuration
fn calculate_max_position(config: &MarketMakerConfig) -> f64 {
    if config.hedge_only_mode {
        // In hedge mode, don't allow any net position
        0.0
    } else {
        // Use the higher of max long/short as the absolute position size
        // The actual direction limit will be enforced in the market making logic
        config.max_long_usd.max(config.max_short_usd)
    }
}

/// Get current timestamp in milliseconds
fn current_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_millis() as u64
}

/// Slow path function to check Redis for config updates
async fn slow_path_check_config(
    redis_client: &RedisClient,
    symbol: &str,
    params: Arc<Mutex<SharedMarketMakerParams>>,
    exchange_client: &Arc<Mutex<ExchangeClient>>,
    active_orders: &Arc<Mutex<HashMap<u64, EnhancedRestingOrder>>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Create a key for Redis lookup
    let key = format!("config:{}", symbol);
    
    // Connect to Redis
    let mut conn = redis_client.get_async_connection().await?;
    
    // Check if config exists
    if !conn.exists::<_, bool>(&key).await? {
        debug!("No config found in Redis for {}", symbol);
        return Ok(());
    }
    
    // Read the config JSON from Redis
    let config_json: String = conn.get(&key).await?;
    
    // Parse the config
    match serde_json::from_str::<MarketMakerConfig>(&config_json) {
        Ok(new_config) => {
            // First check if config has changed - use a separate scope for the lock
            let config_has_changed = {
                // Get current shared parameters
                let shared = params.lock().await;
                
                // Check if the config is different
                shared.config.daily_return_bps != new_config.daily_return_bps || 
                shared.config.notional_per_side != new_config.notional_per_side || 
                shared.config.daily_pnl_stop_loss != new_config.daily_pnl_stop_loss ||
                shared.config.trailing_take_profit != new_config.trailing_take_profit ||
                shared.config.trailing_stop_loss != new_config.trailing_stop_loss ||
                shared.config.hedge_only_mode != new_config.hedge_only_mode ||
                shared.config.force_quote_refresh_interval != new_config.force_quote_refresh_interval ||
                shared.config.max_long_usd != new_config.max_long_usd ||
                shared.config.max_short_usd != new_config.max_short_usd ||
                shared.config.enable_trading != new_config.enable_trading ||
                quote_levels_changed(&shared.config.quote_levels, &new_config.quote_levels) ||
                option_string_changed(&shared.config.vault_address, &new_config.vault_address)
            }; // Lock released here
            
            if config_has_changed {
                // Config has changed, log it
                info!("[SLOW PATH] Detected configuration change for {}", symbol);
                info!("[SLOW PATH] New config from Redis: daily_return_bps={}, notional={}, stop_loss={}, tp={}, sl={}, hedge={}, max_long={}, max_short={}, interval={}, enable_trading={}", 
                    new_config.daily_return_bps, 
                    new_config.notional_per_side,
                    new_config.daily_pnl_stop_loss,
                    new_config.trailing_take_profit,
                    new_config.trailing_stop_loss,
                    new_config.hedge_only_mode,
                    new_config.max_long_usd,
                    new_config.max_short_usd,
                    new_config.force_quote_refresh_interval,
                    new_config.enable_trading);
                
                // Check if vault address changed - use a separate scope to get the shared config
                let vault_address_changed = {
                    let shared = params.lock().await;
                    match (&shared.config.vault_address, &new_config.vault_address) {
                        (Some(old), Some(new)) => old != new,
                        (None, Some(_)) => true,
                        (Some(_), None) => true,
                        (None, None) => false,
                    }
                };

                // If vault address changed, we need to recreate the ExchangeClient
                if vault_address_changed {
                    info!("[SLOW PATH] Vault address changed to {:?}. Will recreate ExchangeClient.", 
                          new_config.vault_address);
                    
                    // Get the wallet from the existing exchange client
                    let wallet = {
                        let client = exchange_client.lock().await;
                        client.wallet.clone()
                    };
                    
                    // Convert vault address String to H160 if present
                    let vault_address = if let Some(addr_str) = &new_config.vault_address {
                        use ethers::types::H160;
                        use std::str::FromStr;
                        
                        match H160::from_str(addr_str) {
                            Ok(h160_addr) => {
                                info!("[SLOW PATH] Using vault address: {}", addr_str);
                                Some(h160_addr)
                            },
                            Err(e) => {
                                error!("[SLOW PATH] Invalid vault address format: {}. Error: {}", addr_str, e);
                                None
                            }
                        }
                    } else {
                        info!("[SLOW PATH] No vault address specified");
                        None
                    };
                    
                    // Create new exchange client with updated vault address
                    let base_url = if cfg!(feature = "testnet") { Some(BaseUrl::Testnet) } else { Some(BaseUrl::Mainnet) };
                    
                    match ExchangeClient::new(
                        None, 
                        wallet, 
                        base_url.clone(), 
                        None, 
                        vault_address
                    ).await {
                        Ok(new_client) => {
                            // Replace the exchange client
                            let mut client_lock = exchange_client.lock().await;
                            *client_lock = new_client;
                            info!("[SLOW PATH] Successfully recreated ExchangeClient with new vault address");
                        },
                        Err(e) => {
                            error!("[SLOW PATH] Failed to recreate ExchangeClient: {}", e);
                        }
                    }
                }
                
                // Update shared parameters in a separate lock
                let old_config = {
                    let mut shared = params.lock().await;
                    
                    // Store old config values for logging
                    let old_config_clone = shared.config.clone();
                    
                    // Calculate price precision for the symbol
                    let price_decimals = match symbol {
                        "BTC" => 0,
                        "ETH" => 2,
                        "SOL" => 3,
                        "AVAX" => 3,
                        "MATIC" => 4,
                        "DOGE" => 6,
                        "SHIB" => 8,
                        _ => 2,
                    };
                    
                    // Update the shared parameters
                    shared.config = new_config.clone();
                    shared.half_spread = new_config.daily_return_bps / 365;
                    shared.target_liquidity = new_config.notional_per_side;
                    shared.max_position_size = if new_config.hedge_only_mode {
                        0.0
                    } else {
                        new_config.max_long_usd.max(new_config.max_short_usd)
                    };
                    shared.price_decimals = price_decimals;
                    shared.needs_refresh = true;
                    
                    old_config_clone
                }; // Lock released here
                
                info!("[SLOW PATH] Updated market maker for {}", symbol);
                info!("[SLOW PATH] Old config: return_bps={}, notional={}, stop_loss={}, max_long={}, max_short={}, enable_trading={}", 
                      old_config.daily_return_bps, old_config.notional_per_side, 
                      old_config.daily_pnl_stop_loss, old_config.max_long_usd,
                      old_config.max_short_usd, old_config.enable_trading);
                info!("[SLOW PATH] New config: return_bps={}, notional={}, stop_loss={}, max_long={}, max_short={}, enable_trading={}", 
                      new_config.daily_return_bps, new_config.notional_per_side, 
                      new_config.daily_pnl_stop_loss, new_config.max_long_usd,
                      new_config.max_short_usd, new_config.enable_trading);
                
                // Cancel orders in a separate step to avoid holding multiple locks
                // First check if we have any orders to cancel
                let has_orders = {
                    let active_orders_lock = active_orders.lock().await;
                    !active_orders_lock.is_empty()
                }; // Lock released
                
                if has_orders {
                    // Collect oids to cancel
                    let cancels = {
                        let active_orders_lock = active_orders.lock().await;
                        active_orders_lock.keys()
                            .map(|oid| ClientCancelRequest {
                                asset: symbol.to_string(),
                                oid: *oid,
                            })
                            .collect::<Vec<_>>()
                    }; // Lock released
                    
                    // Cancel the orders
                    if !cancels.is_empty() {
                        let exchange_client_lock = exchange_client.lock().await;
                        if let Err(e) = exchange_client_lock.bulk_cancel(cancels, None).await {
                            warn!("[SLOW PATH] Failed to cancel orders: {}", e);
                        } else {
                            info!("[SLOW PATH] Cancelled existing orders to apply new config");
                            // Clear the order map
                            let mut active_orders_lock = active_orders.lock().await;
                            active_orders_lock.clear();
                        }
                    }
                }
            } else {
                debug!("[SLOW PATH] No configuration change detected for {}", symbol);
            }
        },
        Err(e) => {
            error!("[SLOW PATH] Failed to parse configuration from Redis: {}", e);
        }
    }
    
    Ok(())
}

/// Helper function to store configuration in Redis
async fn store_config_in_redis(redis_client: &RedisClient, config: &MarketMakerConfig) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Convert config to JSON
    let config_json = serde_json::to_string(config)?;
    
    // Connect to Redis
    let mut conn = redis_client.get_async_connection().await?;
    
    // Store configuration with symbol as key
    let key = format!("config:{}", config.symbol);
    conn.set::<_, _, ()>(key, &config_json).await?;
    
    debug!("Stored configuration in Redis for {}: {}", config.symbol, config_json);
    
    Ok(())
} 