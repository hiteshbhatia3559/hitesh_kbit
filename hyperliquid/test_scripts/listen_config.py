#!/usr/bin/env python3
"""
Script to listen for configuration messages on the mm_config channel.
This helps diagnose if Redis pub/sub is working correctly.
"""

import redis
import argparse
import sys
import signal
import json

def listen_for_configs(redis_url):
    """Connect to Redis and listen for config updates."""
    try:
        # Connect to Redis
        print(f"Connecting to Redis at {redis_url}")
        r = redis.Redis.from_url(redis_url)
        
        # Test Redis connection
        r.ping()
        print("Connected to Redis successfully")
        
        # Create a pubsub instance
        pubsub = r.pubsub()
        
        # Subscribe to the mm_config channel
        pubsub.subscribe("mm_config")
        print("Subscribed to mm_config channel, listening for messages...")
        
        # Set up signal handler for clean exit
        def signal_handler(sig, frame):
            print("\nExiting...")
            pubsub.unsubscribe()
            sys.exit(0)
        
        signal.signal(signal.SIGINT, signal_handler)
        
        # Listen for messages
        for message in pubsub.listen():
            if message["type"] == "message":
                data = message["data"].decode("utf-8")
                print(f"Received message: {data}")
                
                # Try to parse as JSON for prettier display
                try:
                    config = json.loads(data)
                    print("Parsed configuration:")
                    for key, value in config.items():
                        print(f"  {key}: {value}")
                except json.JSONDecodeError:
                    print("Could not parse message as JSON")
                
                print("-" * 50)
        
        return True
    except redis.exceptions.ConnectionError as e:
        print(f"Error connecting to Redis: {e}", file=sys.stderr)
        return False
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        return False

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Listen for configurations on mm_config channel")
    parser.add_argument("--redis-url", default="redis://localhost:6379", help="Redis URL")
    
    args = parser.parse_args()
    
    success = listen_for_configs(args.redis_url)
    sys.exit(0 if success else 1) 