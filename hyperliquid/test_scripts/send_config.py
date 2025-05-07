#!/usr/bin/env python3
"""
Script to send a test configuration update to the market maker via Redis.
This simulates what the UI would do when a user updates configuration.
"""

import json
import argparse
import redis
import sys

def send_config(redis_url, symbol="BTC", num_levels=1, enable_trading=False, vault_address=None):
    """Connect to Redis and set a test configuration directly in Redis."""
    try:
        # Generate quote levels based on num_levels
        quote_levels = []
        for i in range(1, num_levels + 1):
            # For each level, increase the spread and potentially the size
            quote_levels.append({
                "level": i,
                "spread_multiplier": i * 1.0,  # Level 1: 1.0x, Level 2: 2.0x, etc.
                "size_multiplier": 1.5 / i     # Level 1: 1.5x, Level 2: 0.75x, Level 3: 0.5x, etc.
            })
            
        # Sample configuration
        config = {
            "symbol": symbol,
            "daily_return_bps": 200,
            "notional_per_side": 150.0,
            "daily_pnl_stop_loss": 200.0,
            "trailing_take_profit": 0.05,
            "trailing_stop_loss": 0.02,
            "hedge_only_mode": False,
            "force_quote_refresh_interval": 100,
            "max_long_usd": 10000.0,
            "max_short_usd": 10000.0,
            "enable_trading": enable_trading,
            "quote_levels": quote_levels
        }
        
        # Add vault address if provided
        if vault_address:
            config["vault_address"] = vault_address
            print(f"Using vault address: {vault_address}")
        
        # Connect to Redis
        print(f"Connecting to Redis at {redis_url}")
        r = redis.Redis.from_url(redis_url)
        
        # Test Redis connection
        r.ping()
        print("Connected to Redis successfully")
        
        # Convert config to JSON
        config_json = json.dumps(config)
        
        # Set the value directly in Redis with the correct key format
        key = f"config:{symbol}"
        r.set(key, config_json)
        print(f"Stored configuration for {symbol} directly in Redis with key '{key}'")
        print(f"Configuration: {config_json}")
        
        # Print detail about the quote levels
        print("\nQuote Levels:")
        for level in quote_levels:
            print(f"  Level {level['level']}: Spread Multiplier = {level['spread_multiplier']}x, Size Multiplier = {level['size_multiplier']}x")
        
        return True
    except redis.exceptions.ConnectionError as e:
        print(f"Error connecting to Redis: {e}", file=sys.stderr)
        return False
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        return False

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Send test configuration to market maker")
    parser.add_argument("--redis-url", default="redis://localhost:6379", help="Redis URL")
    parser.add_argument("--symbol", default="BTC", help="Trading symbol to configure")
    parser.add_argument("--levels", type=int, default=1, help="Number of quote levels to configure")
    parser.add_argument("--enable-trading", action="store_true", help="Enable actual trading")
    parser.add_argument("--vault-address", help="Ethereum address of the vault to use for trading")
    
    args = parser.parse_args()
    
    success = send_config(args.redis_url, args.symbol, args.levels, args.enable_trading, args.vault_address)
    sys.exit(0 if success else 1) 