use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use serde::{Serialize, Deserialize};
use log::{info, error, warn, debug};
use redis::{Client as RedisClient, AsyncCommands};

use crate::enhanced_market_maker::Position;

/// Position summary with aggregated metrics
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PositionSummary {
    pub timestamp: u64,
    pub positions: Vec<Position>,
    pub total_pnl: f64,
    pub total_long_exposure: f64,
    pub total_short_exposure: f64,
}

/// Position Manager for tracking and updating positions
pub struct PositionManager {
    redis_client: RedisClient,
    positions: Arc<RwLock<HashMap<String, Position>>>,
    update_interval: Duration,
    update_channel: String,
}

impl PositionManager {
    /// Create a new position manager
    pub fn new(
        redis_client: RedisClient,
        positions: Arc<RwLock<HashMap<String, Position>>>,
        update_interval: Duration,
        update_channel: String,
    ) -> Self {
        PositionManager {
            redis_client,
            positions,
            update_interval,
            update_channel,
        }
    }
    
    /// Start the position manager
    pub async fn start(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("Starting Position Manager");
        info!("Publishing position updates every {:?} to channel: {}", 
            self.update_interval, self.update_channel);
        
        loop {
            // Publish position updates
            if let Err(e) = self.publish_position_updates().await {
                error!("Failed to publish position updates: {}", e);
            }
            
            // Wait for next update interval
            tokio::time::sleep(self.update_interval).await;
        }
    }
    
    /// Publish position updates to Redis
    async fn publish_position_updates(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Read current positions
        let positions_read = self.positions.read().await;
        
        // Skip if no positions
        if positions_read.is_empty() {
            return Ok(());
        }
        
        // Calculate summary metrics
        let mut total_pnl = 0.0;
        let mut total_long_exposure = 0.0;
        let mut total_short_exposure = 0.0;
        
        let positions: Vec<Position> = positions_read.values().cloned().collect();
        
        for position in &positions {
            total_pnl += position.unrealized_pnl;
            
            if position.size > 0.0 {
                total_long_exposure += position.notional_usd;
            } else if position.size < 0.0 {
                total_short_exposure += position.notional_usd;
            }
        }
        
        // Create summary
        let summary = PositionSummary {
            timestamp: current_timestamp_ms(),
            positions,
            total_pnl,
            total_long_exposure,
            total_short_exposure,
        };
        
        // Convert to JSON
        let summary_json = serde_json::to_string(&summary)?;
        
        // Get Redis connection
        let mut conn = self.redis_client.get_async_connection().await?;
        
        // Publish to Redis channel
        conn.publish(&self.update_channel, &summary_json).await?;
        
        // Add to Redis stream for history
        let stream_data = vec![("data", summary_json.clone())];
        conn.xadd("position_history", "*", &stream_data).await?;
        
        debug!("Published position update with {} positions", summary.positions.len());
        
        Ok(())
    }
    
    /// Update position information
    pub async fn update_position(
        &self,
        symbol: &str,
        size: f64,
        entry_price: f64,
        current_price: f64,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut positions_write = self.positions.write().await;
        
        // Calculate PnL and notional value
        let unrealized_pnl = (current_price - entry_price) * size;
        let notional_usd = current_price.abs() * size.abs();
        
        // Update or create position
        let position = Position {
            symbol: symbol.to_string(),
            size,
            entry_price,
            current_price,
            unrealized_pnl,
            notional_usd,
        };
        
        positions_write.insert(symbol.to_string(), position);
        
        Ok(())
    }
}

/// Get current timestamp in milliseconds
fn current_timestamp_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_millis() as u64
} 