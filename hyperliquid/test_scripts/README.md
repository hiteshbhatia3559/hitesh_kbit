# Testing Scripts

These scripts help test the market maker application without needing the full UI.

## Available Scripts

- `send_config.py`: Sends a test configuration to the market maker
- `listen_positions.py`: Listens for position updates from the market maker

## Prerequisites

You need to have Python 3 and the Redis library installed:

```bash
pip install redis
```

## Usage

### Sending Test Configuration

To send a test configuration (simulating what the UI would do):

```bash
./send_config.py --redis-url redis://localhost:6379 --symbol BTC
```

This will send a sample market making configuration for BTC to the Redis `mm_config` channel.

### Listening for Position Updates

To listen for position updates:

```bash
./listen_positions.py --redis-url redis://localhost:6379 --timeout 120
```

This will listen for position updates on the `mm_position_updates` channel for 120 seconds.

## Example Flow

1. Start the market maker application:
   ```bash
   docker-compose up
   ```

2. In another terminal, listen for position updates:
   ```bash
   ./listen_positions.py
   ```

3. In a third terminal, send a test configuration:
   ```bash
   ./send_config.py --symbol BTC
   ```

4. Watch as the market maker processes the configuration and starts publishing position updates. 