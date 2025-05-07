#!/usr/bin/env python3
"""
Script to toggle the enable_trading flag for a market maker via Redis.
This allows quickly enabling or disabling trading without stopping the service.
"""

import json
import argparse
import redis
import sys

def toggle_trading(redis_url, symbol="BTC", enable=True):
    """Connect to Redis, get existing config, toggle the trading flag, and save."""
    try:
        # Connect to Redis
        print(f"Connecting to Redis at {redis_url}")
        r = redis.Redis.from_url(redis_url)
        
        # Test Redis connection
        r.ping()
        print("Connected to Redis successfully")
        
        # Get existing configuration
        key = f"config:{symbol}"
        config_json = r.get(key)
        
        if not config_json:
            print(f"No configuration found for {symbol}. Please run send_config.py first.")
            return False
        
        # Parse the config - decode if it's bytes
        if isinstance(config_json, bytes):
            config_json = config_json.decode('utf-8')
        config = json.loads(config_json)
        
        # Update the enable_trading flag
        old_value = config.get('enable_trading', True)  # Default to True if not present
        config['enable_trading'] = enable
        
        # Convert updated config to JSON
        updated_config_json = json.dumps(config)
        
        # Save back to Redis
        r.set(key, updated_config_json)
        print(f"Updated configuration for {symbol}: enable_trading {old_value} -> {enable}")
        print(f"Configuration: {updated_config_json}")
        
        return True
    except redis.exceptions.ConnectionError as e:
        print(f"Error connecting to Redis: {e}", file=sys.stderr)
        return False
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        return False

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Toggle trading for a market maker")
    parser.add_argument("--redis-url", default="redis://localhost:6379", help="Redis URL")
    parser.add_argument("--symbol", default="BTC", help="Trading symbol to configure")
    parser.add_argument("--enable", action="store_true", help="Enable trading (default)")
    parser.add_argument("--disable", action="store_true", help="Disable trading")
    
    args = parser.parse_args()
    
    # Determine the desired trading state
    enable_trading = not args.disable if args.disable else True
    
    success = toggle_trading(args.redis_url, args.symbol, enable_trading)
    sys.exit(0 if success else 1) 