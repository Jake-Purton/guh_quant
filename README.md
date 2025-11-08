# Quantitative Trading Project

A Rust-based quantitative trading system that builds portfolios based on investor profiles and risk tolerance.

## Setup

### 1. Python Setup (One-time data fetch)

Install Python dependencies:
```bash
pip3 install -r requirements.txt
```

Fetch and cache 1000+ US equities with pre-computed volatilities:
```bash
python3 fetch_stocks.py
```

This will create `stocks_cache.json` with:
- 1000+ US stocks from S&P 500, NASDAQ 100, and popular stocks
- Current prices
- 30-day historical volatility calculations
- Sector classifications
- Market cap data

**Note:** This takes ~10-15 minutes but only needs to be run once (or periodically to refresh data).

### 2. Rust Application

Build and run:
```bash
cargo build --release
cargo run
```

## How It Works

### Investor Profiling
The system automatically parses investor context and extracts:
- **Age** → Risk tolerance (Conservative/Moderate/Aggressive)
- **Budget** → Investment capital
- **Excluded sectors** → Filters out unwanted industries
- **Risk-based allocation** → % of capital in stocks vs bonds

### Risk Levels
- **Conservative (60+)**: 25% stocks, low volatility (<3%)
- **Moderate (40-59)**: 65% stocks, medium volatility (<5%)
- **Aggressive (<40)**: 85% stocks, all volatility levels

### Portfolio Construction
1. Loads 1000+ stocks from cache (instant!)
2. Filters by investor's excluded sectors
3. Filters by risk tolerance (volatility threshold)
4. Builds diversified portfolio (7-15 stocks based on risk)
5. Equal-weight allocation within budget
6. Submits to competition API

## File Structure

```
src/
├── main.rs       - Main app & API integration
├── investor.rs   - Investor profiling & parsing
├── stocks.rs     - Stock data loading & caching
└── portfolio.rs  - Portfolio construction logic

fetch_stocks.py   - Python script to fetch & cache stock data
stocks_cache.json - Cached stock data (generated)
```

## Advantages of Caching

✅ **Speed**: Load 1000 stocks instantly vs 10+ minutes live fetching  
✅ **Reliability**: No API rate limits during competition  
✅ **Offline**: Works without internet after initial fetch  
✅ **Consistency**: Same data across multiple runs  
✅ **Rich data**: Pre-computed volatilities for smart filtering

## Refreshing Data

To update stock prices and volatilities:
```bash
python3 fetch_stocks.py
```

Recommended: Refresh daily or before competition runs.

## API Endpoints

- `GET /request` - Get investor context
- `GET /info` - Get team information
- `POST /submit` - Submit portfolio for evaluation

## Example Output

```
📊 Investor Profile:
  Name: John Doe
  Age: 45 (Moderate)
  Budget: $50000.00
  Stock allocation: 65%
  Excluded: ["Real Estate", "Energy"]
  Stock budget: $32500.00

📂 Loading stocks from cache: stocks_cache.json
✅ Loaded 987 stocks from cache

📋 Eligible stocks after filtering: 423

💼 Proposed Portfolio:
  AAPL x24 @ $268.47 = $6443.28 (vol: 1.44%)
  MSFT x6 @ $496.82 = $2980.92 (vol: 1.14%)
  ...
  Total: $31847.56 / $32500.00

✅ Portfolio submitted!
```
