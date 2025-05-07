use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use redis::Client as RedisClient;
use log::{info, error, warn, debug};

// Import the SymbolScanner from the main crate
use hyperliquid_market_maker::SymbolScanner;

#[tokio::main]
async fn main() {
    // Initialize logging
    env_logger::init();
    info!("Starting Symbol Scanner Test");

    // Initialize Redis connection
    let redis_url = env::var("REDIS_URL").unwrap_or_else(|_| "redis://redis:6379".to_string());
    info!("Connecting to Redis at {}", redis_url);
    
    let redis_client = match redis::Client::open(redis_url) {
        Ok(client) => client,
        Err(e) => {
            error!("Failed to connect to Redis: {}", e);
            return;
        }
    };
    
    // Test connection
    match redis_client.get_async_connection().await {
        Ok(mut conn) => {
            if let Err(e) = redis::cmd("PING").query_async::<_, ()>(&mut conn).await {
                error!("Failed to ping Redis: {}", e);
                return;
            }
            info!("Successfully connected to Redis");
        },
        Err(e) => {
            error!("Failed to get Redis connection: {}", e);
            return;
        }
    }
    
    // Create shared state for metrics
    let symbol_metrics = Arc::new(RwLock::new(HashMap::new()));
    
    // Create and start the symbol scanner
    match SymbolScanner::new(
        redis_client,
        symbol_metrics.clone(),
        Duration::from_secs(3600), // 1 hour scan interval
        10, // Top 10 symbols
        true, // Use testnet
    ).await {
        Ok(scanner) => {
            info!("Symbol Scanner created successfully");
            
            // Subscribe to Redis channel to monitor updates
            let redis_client_sub = match redis::Client::open(env::var("REDIS_URL").unwrap_or_else(|_| "redis://redis:6379".to_string())) {
                Ok(client) => client,
                Err(e) => {
                    error!("Failed to create second Redis connection: {}", e);
                    return;
                }
            };
            
            // Create a task to monitor Redis updates
            let metrics_clone = symbol_metrics.clone();
            tokio::spawn(async move {
                // Subscribe to Redis updates
                match redis_client_sub.get_async_connection().await {
                    Ok(conn) => {
                        let mut pubsub = conn.into_pubsub();
                        
                        // Subscribe to position updates channel
                        if let Err(e) = pubsub.subscribe("mm_position_updates").await {
                            error!("Failed to subscribe to Redis channel: {}", e);
                            return;
                        }
                        
                        info!("Subscribed to Redis channel 'mm_position_updates'");
                        
                        let mut stream = pubsub.on_message();
                        while let Some(msg) = stream.next().await {
                            let payload: String = match msg.get_payload() {
                                Ok(payload) => payload,
                                Err(e) => {
                                    error!("Failed to get message payload: {}", e);
                                    continue;
                                }
                            };
                            
                            info!("Received Redis message: {}", payload);
                        }
                    },
                    Err(e) => {
                        error!("Failed to get Redis connection for PubSub: {}", e);
                    }
                }
            });
            
            // Start the scanner in a background task
            let scanner_clone = scanner;
            tokio::spawn(async move {
                if let Err(e) = scanner_clone.start().await {
                    error!("Symbol scanner error: {}", e);
                }
            });
            
            // Main monitoring loop
            loop {
                // Periodically dump metrics to console for visibility
                let metrics = symbol_metrics.read().await;
                if !metrics.is_empty() {
                    info!("Current metrics - {} symbols tracked", metrics.len());
                    
                    // Print top 5 by volume
                    let mut symbols: Vec<_> = metrics.values().collect();
                    symbols.sort_by(|a, b| b.volume_24h.partial_cmp(&a.volume_24h).unwrap_or(std::cmp::Ordering::Equal));
                    
                    for (i, metric) in symbols.iter().take(5).enumerate() {
                        info!("  #{}: {} - Vol: ${:.2}, OI: ${:.2}", 
                            i+1, metric.symbol, metric.volume_24h, metric.open_interest);
                    }
                } else {
                    info!("No metrics available yet");
                }
                
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        },
        Err(e) => {
            error!("Failed to create symbol scanner: {}", e);
        }
    }
} 