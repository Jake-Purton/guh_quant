# Quick Start Guide

## Step 1: Fetch Stock Data (One Time)

Run the Python script to fetch and cache 1000+ US equities:

```bash
python3 fetch_stocks.py
```

**What it does:**
- Fetches S&P 500, NASDAQ 100, and popular stocks
- Calculates 30-day volatility for each stock
- Maps stocks to sectors (Technology, Healthcare, etc.)
- Saves everything to `stocks_cache.json`

**Time:** ~10-15 minutes (only needs to be run once)

**Output:** `stocks_cache.json` with structure:
```json
{
  "metadata": {
    "generated_at": "2025-11-08T...",
    "stock_count": 987,
    "sector_keywords": {...},
    "sectors": ["Technology", "Healthcare", ...]
  },
  "stocks": [
    {
      "ticker": "AAPL",
      "name": "Apple Inc.",
      "price": 268.47,
      "sector": "Technology",
      "volatility": 0.0144,
      "market_cap": 4156000000000
    },
    ...
  ]
}
```

## Step 2: Run Rust Application

```bash
cargo run --release
```

**What it does:**
- Loads cached stock data (instant!)
- Gets investor context from API
- Parses investor profile (age, budget, excluded sectors)
- Filters stocks by risk tolerance
- Builds diversified portfolio
- Submits to competition API

**Speed:** <2 seconds total (vs 10+ minutes with live fetching)

## Benefits

| Feature | Live Fetching | Cached Data |
|---------|--------------|-------------|
| Speed | 10+ minutes | <1 second |
| Stocks | ~100 | 1000+ |
| Reliability | Rate limited | Always works |
| Offline | ❌ | ✅ |
| Volatility | Calculated | Pre-computed |

## Example Workflow

```bash
# One-time setup
pip3 install -r requirements.txt
python3 fetch_stocks.py

# Run competition (can run multiple times)
cargo run --release
```

## Refreshing Data

To update prices and volatilities:
```bash
python3 fetch_stocks.py
```

Recommended: Daily or before each competition session.
