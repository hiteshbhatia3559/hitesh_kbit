use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use serde::{Serialize, Deserialize};
use log::{info, error, warn, debug};
use redis::{Client as RedisClient, AsyncCommands};
use tokio::time::Instant;
use ethers::types::H160;

use hyperliquid_rust_sdk::{
    InfoClient, 
    BaseUrl,
    Error as SdkError,
    Meta,
    CandlesSnapshotResponse
};

/// Metrics for a trading symbol
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SymbolMetrics {
    pub symbol: String,
    pub volume_24h: f64,
    pub is_active: bool,
    pub last_updated: u64,
}

/// Complete candle data for storage
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CandleData {
    pub time_open: u64,
    pub time_close: u64,
    pub coin: String,
    pub candle_interval: String,
    pub open: String,
    pub close: String,
    pub high: String,
    pub low: String,
    pub vlm: String,
    pub num_trades: u64,
    pub volume_24h: f64,  // Calculated 24h volume
    pub last_updated: u64, // Timestamp of when this data was updated
    pub price: f64,
}

/// Symbol Scanner service that periodically scans for new listings and tracks metrics
pub struct SymbolScanner {
    info_client: InfoClient,
    redis_client: RedisClient,
    metrics: Arc<RwLock<HashMap<String, SymbolMetrics>>>,
    scan_interval: Duration,
    top_n_symbols: usize,
    last_api_call: Instant,
}

impl SymbolScanner {
    /// Create a new symbol scanner
    pub async fn new(
        redis_client: RedisClient,
        metrics: Arc<RwLock<HashMap<String, SymbolMetrics>>>,
        scan_interval: Duration,
        top_n_symbols: usize,
        testnet: bool,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Create InfoClient to get market data
        let base_url = if testnet { Some(BaseUrl::Testnet) } else { Some(BaseUrl::Mainnet) };
        let info_client = InfoClient::new(None, base_url).await?;
        info!("base_url: {:?}", info_client.http_client.base_url);
        
        Ok(SymbolScanner {
            info_client,
            redis_client,
            metrics,
            scan_interval,
            top_n_symbols,
            last_api_call: Instant::now(),
        })
    }
    
    /// Start the scanner service
    pub async fn start(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("Starting Symbol Scanner Service using REST APIs only");
        info!("Monitoring top {} symbols with rate limit of 1 request per second", self.top_n_symbols);
        
        // Main scanning loop
        loop {
            info!("Performing scheduled scan for symbol metrics");
            if let Err(e) = self.scan_symbols().await {
                error!("Failed to scan symbols: {}", e);
            }
            
            // Wait for the next scan interval
            tokio::time::sleep(self.scan_interval).await;
        }
    }
    
    /// Apply rate limit before making an API call
    async fn apply_rate_limit(&mut self) {
        // Calculate time since last API call
        let elapsed = self.last_api_call.elapsed();
        let one_second = Duration::from_secs(1);
        
        // If less than 1 second has passed, sleep for the remaining time
        if elapsed < one_second {
            let sleep_duration = one_second - elapsed;
            debug!("Rate limiting: sleeping for {:?} before next API call", sleep_duration);
            tokio::time::sleep(sleep_duration).await;
        }
        
        // Update the last API call timestamp
        self.last_api_call = Instant::now();
    }
    
    /// Rate-limited meta call
    async fn get_meta(&mut self) -> Result<Meta, Box<dyn std::error::Error + Send + Sync>> {
        self.apply_rate_limit().await;
        self.info_client.meta().await.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
    }
    
    /// Rate-limited all_mids call
    async fn get_all_mids(&mut self) -> Result<HashMap<String, String>, Box<dyn std::error::Error + Send + Sync>> {
        self.apply_rate_limit().await;
        self.info_client.all_mids().await.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
    }
    
    /// Rate-limited candles call
    async fn get_candles(&mut self, symbol: String, interval: String, start_time: u64, end_time: u64) 
        -> Result<Vec<CandlesSnapshotResponse>, Box<dyn std::error::Error + Send + Sync>> {
        self.apply_rate_limit().await;
        self.info_client.candles_snapshot(symbol, interval, start_time, end_time).await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
    }
    
    /// Main method to scan for symbol metrics using REST APIs
    async fn scan_symbols(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Step 1: Get metadata for all available assets with rate limiting
        let meta = self.get_meta().await?;
        
        // Step 2: Get all mid prices with rate limiting
        let all_mids = self.get_all_mids().await?;
        
        // Extract symbols from meta
        let mut symbol_metrics = HashMap::new();
        let mut candle_map = HashMap::new();  // For storing complete candle data
        let timestamp = current_timestamp_ms();
        
        // Process each asset
        let universe = &meta.universe;
        for asset_info in universe[0..1].iter() {
            // Get symbol name
            let symbol = asset_info.name.clone();
            
            // Find mid price for this symbol
            let price = match all_mids.get(&symbol) {
                Some(mid_price_str) => mid_price_str.parse::<f64>().unwrap_or(0.0),
                None => {
                    warn!("No price data available for {}, skipping", symbol);
                    continue;
                }
            };
            
            // Calculate time range for the 24h candle (now - 24 hours to now)
            let now = current_timestamp_ms();
            let twenty_four_hours_ago = now - (24 * 60 * 60 * 1000); // 24 hours in milliseconds
            
            // Initialize volume
            let mut volume_24h = 0.0;
            let mut latest_candle: Option<CandleData> = None;
            
            // Try to get 24h candle data with rate limiting
            let candles = self.get_candles(
                symbol.clone(),
                "1d".to_string(),  // Daily candles
                twenty_four_hours_ago,
                now
            ).await;
            
            match candles {
                Ok(candle_data) => {
                    if !candle_data.is_empty() {
                        // Sum up the volume from all candles in the period
                        for (i, candle) in candle_data.iter().enumerate() {
                            if let Ok(candle_volume) = candle.vlm.parse::<f64>() {
                                volume_24h += candle_volume * price;
                            }
                            
                            // Save the most recent candle data
                            if i == candle_data.len() - 1 {
                                latest_candle = Some(CandleData {
                                    time_open: candle.time_open,
                                    time_close: candle.time_close,
                                    coin: candle.coin.clone(),
                                    candle_interval: candle.candle_interval.clone(),
                                    open: candle.open.clone(),
                                    close: candle.close.clone(),
                                    high: candle.high.clone(),
                                    low: candle.low.clone(),
                                    vlm: candle.vlm.clone(), // in base currency
                                    num_trades: candle.num_trades,
                                    volume_24h: volume_24h, 
                                    last_updated: timestamp,
                                    price: price,
                                });
                            }
                        }
                        info!("Found 24h volume for {}: ${:.2}", symbol, volume_24h);
                    }
                },
                Err(e) => {
                    warn!("Could not get candle data for {}: {}", symbol, e);
                }
            }
            
            info!("Processed symbol: {} - Price: {}, Volume 24h: {}", 
                symbol, price, volume_24h);
            
            // Create metrics with information we have
            let metrics = SymbolMetrics {
                symbol: symbol.clone(),
                volume_24h,
                is_active: true,
                last_updated: timestamp,
            };
            
            // Insert into metrics map
            symbol_metrics.insert(symbol.clone(), metrics);
            
            // Add the candle data to the candle map if available
            if let Some(candle) = latest_candle {
                candle_map.insert(symbol, candle);
            }
        }
        
        // Update shared state with all symbols
        {
            let mut metrics_write = self.metrics.write().await;
            *metrics_write = symbol_metrics.clone();
        }
        
        // Update Redis with both metrics and complete candle data
        update_redis_with_candles(&self.redis_client, &symbol_metrics, &candle_map, self.top_n_symbols).await?;
        
        Ok(())
    }
}

/// Update Redis with symbol metrics and complete candle data
async fn update_redis_with_candles(
    redis_client: &RedisClient,
    symbol_metrics: &HashMap<String, SymbolMetrics>,
    candle_map: &HashMap<String, CandleData>,
    top_n_symbols: usize
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut conn = redis_client.get_async_connection().await?;
    
    // Sort symbols by volume and take the top N
    let mut symbols: Vec<_> = symbol_metrics.values().cloned().collect();
    symbols.sort_by(|a, b| b.volume_24h.partial_cmp(&a.volume_24h).unwrap_or(std::cmp::Ordering::Equal));
    let top_symbols: Vec<_> = symbols.into_iter().take(top_n_symbols).collect();
    
    // Log the top symbols
    info!("Top {} symbols by volume:", top_symbols.len());
    for (idx, symbol) in top_symbols.iter().enumerate() {
        info!("  {}. {} - Volume: ${:.2}", 
            idx + 1, symbol.symbol, symbol.volume_24h);
    }
    
    // Store top symbols in Redis for quick access
    let top_symbols_json = serde_json::to_string(&top_symbols)?;
    conn.set::<_, _, ()>("top_symbols", &top_symbols_json).await?;
    
    // Update each symbol's candle data in the Redis hash
    const HASH_KEY: &str = "symbol_candles";
    
    // For each new candle, update or add it to the Redis hash
    for (symbol, candle) in candle_map {
        // Serialize the candle data
        let candle_json = serde_json::to_string(candle)?;
        
        // Store it in the hash with the symbol as the field name
        conn.hset::<_, _, _, ()>(HASH_KEY, symbol.clone(), candle_json).await?;
    }
    
    // Get the total number of symbols in the hash for logging
    let hash_size: usize = conn.hlen(HASH_KEY).await?;
    
    info!("Updated metrics and candle data for {} symbols, stored {} total symbols in Redis hash", 
        candle_map.len(), hash_size);
    
    Ok(())
}

/// Get current timestamp in milliseconds
fn current_timestamp_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_millis() as u64
} 