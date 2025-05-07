use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Serialize, Deserialize};
use log::{info, error, warn};
use redis::{Client as RedisClient, AsyncCommands};
use futures::StreamExt;

// Import our enhanced market maker config
use crate::enhanced_market_maker::MarketMakerConfig;

/// Configuration service that handles dynamic configuration via Redis
pub struct ConfigService {
    redis_client: RedisClient,
    configs: Arc<RwLock<HashMap<String, MarketMakerConfig>>>,
    config_channel: String,
}

impl ConfigService {
    /// Create a new configuration service
    pub fn new(
        redis_client: RedisClient,
        configs: Arc<RwLock<HashMap<String, MarketMakerConfig>>>,
        config_channel: String,
    ) -> Self {
        ConfigService {
            redis_client,
            configs,
            config_channel,
        }
    }
    
    /// Start the configuration service
    pub async fn start(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("Starting Configuration Service");
        info!("Listening for configuration updates on channel: {}", self.config_channel);
        
        // Subscribe to Redis channel for configuration updates
        let mut conn = self.redis_client.get_async_connection().await?;
        let mut pubsub = conn.into_pubsub();
        pubsub.subscribe(&self.config_channel).await?;
        
        let mut stream = pubsub.on_message();
        
        // Process configuration messages
        while let Some(msg) = stream.next().await {
            let payload: String = msg.get_payload()?;
            info!("Received configuration update: {}", payload);
            
            // Parse the configuration
            match serde_json::from_str::<MarketMakerConfig>(&payload) {
                Ok(config) => {
                    // Store symbol before moving config
                    let symbol = config.symbol.clone();
                    
                    // Validate configuration
                    if let Err(e) = self.validate_config(&config) {
                        error!("Invalid configuration for {}: {}", symbol, e);
                        continue;
                    }
                    
                    // Store configuration
                    info!("Updating configuration for {}", symbol);
                    info!("Configuration parameters: daily_return_bps={}, notional_per_side={}, interval={}", 
                        config.daily_return_bps, config.notional_per_side, config.force_quote_refresh_interval);
                    
                    let mut configs_write = self.configs.write().await;
                    configs_write.insert(symbol.clone(), config);
                    
                    // Also store in Redis for persistence
                    if let Err(e) = self.store_config_in_redis(&symbol, &payload).await {
                        error!("Failed to store configuration in Redis: {}", e);
                    }
                },
                Err(e) => {
                    error!("Failed to parse configuration: {}", e);
                }
            }
        }
        
        Ok(())
    }
    
    /// Load all stored configurations from Redis
    pub async fn load_stored_configs(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("Loading stored configurations from Redis");
        
        let mut conn = self.redis_client.get_async_connection().await?;
        
        // Get all keys with config: prefix
        let keys: Vec<String> = redis::cmd("KEYS")
            .arg("config:*")
            .query_async(&mut conn)
            .await?;
        
        if keys.is_empty() {
            info!("No stored configurations found");
            return Ok(());
        }
        
        // Load each configuration
        let mut configs_write = self.configs.write().await;
        
        for key in keys {
            let config_json: String = conn.get(&key).await?;
            match serde_json::from_str::<MarketMakerConfig>(&config_json) {
                Ok(config) => {
                    let symbol = config.symbol.clone();
                    configs_write.insert(symbol.clone(), config);
                    info!("Loaded configuration for {}", symbol);
                },
                Err(e) => {
                    error!("Failed to parse stored configuration: {}", e);
                }
            }
        }
        
        info!("Loaded {} stored configurations", configs_write.len());
        
        Ok(())
    }
    
    /// Store configuration in Redis for persistence
    async fn store_config_in_redis(&self, symbol: &str, config_json: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut conn = self.redis_client.get_async_connection().await?;
        
        // Store configuration with symbol as key
        let key = format!("config:{}", symbol);
        conn.set::<_, _, ()>(key, config_json).await?;
        
        Ok(())
    }
    
    /// Validate configuration parameters
    fn validate_config(&self, config: &MarketMakerConfig) -> Result<(), String> {
        // Check required fields
        if config.symbol.is_empty() {
            return Err("Symbol cannot be empty".to_string());
        }
        
        // Check reasonable values
        if config.daily_return_bps == 0 {
            return Err("Daily return BPS must be greater than 0".to_string());
        }
        
        if config.notional_per_side <= 0.0 {
            return Err("Notional per side must be greater than 0".to_string());
        }
        
        if config.daily_pnl_stop_loss <= 0.0 {
            return Err("Daily PNL stop loss must be greater than 0".to_string());
        }
        
        if config.trailing_take_profit <= 0.0 || config.trailing_take_profit >= 1.0 {
            return Err("Trailing take profit must be between 0 and 1".to_string());
        }
        
        if config.trailing_stop_loss <= 0.0 || config.trailing_stop_loss >= 1.0 {
            return Err("Trailing stop loss must be between 0 and 1".to_string());
        }
        
        if config.force_quote_refresh_interval < 100 {
            return Err("Force quote refresh interval must be at least 100ms".to_string());
        }
        
        if config.max_long_usd < 0.0 || config.max_short_usd < 0.0 {
            return Err("Position limits cannot be negative".to_string());
        }
        
        Ok(())
    }
} 