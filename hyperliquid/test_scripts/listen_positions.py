#!/usr/bin/env python3
"""
Script to listen for position updates from the market maker on Redis.
This simulates what the UI would do to display real-time position data.
"""

import json
import argparse
import redis
import sys
import time

def listen_for_positions(redis_url, timeout=60):
    """Connect to Redis and listen for position updates."""
    try:
        # Connect to Redis
        print(f"Connecting to Redis at {redis_url}")
        r = redis.Redis.from_url(redis_url)
        
        # Test Redis connection
        r.ping()
        print("Connected to Redis successfully")
        
        # Subscribe to the position updates channel
        pubsub = r.pubsub()
        pubsub.subscribe("mm_position_updates")
        print(f"Subscribed to mm_position_updates channel")
        print(f"Listening for position updates for {timeout} seconds...")
        
        # Start time for timeout
        start_time = time.time()
        
        # Listen for messages until timeout
        while time.time() - start_time < timeout:
            message = pubsub.get_message(timeout=1)
            if message and message["type"] == "message":
                data = message["data"].decode("utf-8")
                try:
                    position_data = json.loads(data)
                    print("\nReceived position update:")
                    print(json.dumps(position_data, indent=2))
                except json.JSONDecodeError:
                    print(f"Received non-JSON data: {data}")
            time.sleep(0.1)
        
        print("\nTimeout reached. Exiting.")
        return True
    except redis.exceptions.ConnectionError as e:
        print(f"Error connecting to Redis: {e}", file=sys.stderr)
        return False
    except KeyboardInterrupt:
        print("\nListener stopped by user")
        return True
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        return False

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Listen for position updates from market maker")
    parser.add_argument("--redis-url", default="redis://localhost:6379", help="Redis URL")
    parser.add_argument("--timeout", type=int, default=60, help="Listen timeout in seconds")
    
    args = parser.parse_args()
    
    success = listen_for_positions(args.redis_url, args.timeout)
    sys.exit(0 if success else 1) 