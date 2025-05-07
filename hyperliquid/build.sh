#!/bin/bash
set -e

echo "Building Hyperliquid Market Maker Application"

# Check if hyperliquid-rust-sdk exists
if [ ! -d "hyperliquid-rust-sdk" ]; then
  echo "Error: hyperliquid-rust-sdk directory not found."
  echo "Please make sure it exists in the project root."
  exit 1
fi

# Create Cargo.lock if it doesn't exist
if [ ! -f "Cargo.lock" ]; then
  echo "Creating initial Cargo.lock file..."
  touch Cargo.lock
fi

# Check for .env file and create from sample if needed
if [ ! -f ".env" ]; then
  if [ -f "env.sample" ]; then
    echo "Creating .env file from env.sample template..."
    cp env.sample .env
    echo "WARNING: Using sample wallet key. For production, update your .env file with a secure key."
  else
    echo "WARNING: No .env file found. Creating an empty one."
    touch .env
    echo "# Create your own .env file with HYPERLIQUID_WALLET_KEY" > .env
    echo "HYPERLIQUID_WALLET_KEY=e908f86dbb4d55ac876378565aafeabc187f6690f046459397b17d9b9a19688e" >> .env
  fi
fi

# Build the project
echo "Building with Docker Compose..."
docker-compose build

echo "Build completed successfully!"
echo "To start the application, run: docker-compose up" 