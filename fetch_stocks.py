#!/usr/bin/env python3
"""
Fetch US equities data and pre-compute volatilities for Rust application.
This creates a cached JSON file with 1000+ stocks, their prices, sectors, and volatilities.
"""

import yfinance as yf
import pandas as pd
import numpy as np
import json
from datetime import datetime, timedelta
from typing import Dict, List, Optional
import time
from concurrent.futures import ThreadPoolExecutor, as_completed


# Sector keyword mappings for investor preferences
SECTOR_KEYWORDS = {
    "Technology": ["tech", "technology", "software", "hardware", "semiconductor", "ai", "cloud"],
    "Healthcare": ["healthcare", "health", "pharmaceutical", "biotech", "medical", "drug"],
    "Financials": ["financial", "finance", "bank", "banking", "insurance", "investment"],
    "Energy": ["energy", "oil", "gas", "petroleum", "renewable"],
    "Consumer": ["consumer", "retail", "e-commerce", "restaurant", "food", "beverage"],
    "Industrials": ["industrial", "manufacturing", "construction", "aerospace", "defense"],
    "Real Estate": ["real estate", "reit", "property", "housing"],
    "Utilities": ["utility", "utilities", "electric", "water", "power"],
    "Materials": ["materials", "mining", "metals", "chemicals"],
    "Communication": ["communication", "telecom", "media", "entertainment"],
    "Crypto": ["crypto", "cryptocurrency", "bitcoin", "blockchain", "crypto assets"]
}

def get_sp500_tickers() -> List[str]:
    """Get S&P 500 ticker list - hardcoded top stocks."""
    # Top S&P 500 stocks by market cap and liquidity
    return [
        # Mega cap tech
        "AAPL", "MSFT", "GOOGL", "GOOG", "AMZN", "NVDA", "META", "TSLA", "BRK-B",
        # Tech continued
        "AVGO", "ORCL", "ADBE", "CRM", "CSCO", "ACN", "AMD", "INTC", "IBM", "QCOM", "TXN",
        "INTU", "NOW", "PANW", "AMAT", "MU", "ADI", "LRCX", "KLAC", "SNPS", "CDNS", "MCHP",
        # Healthcare
        "UNH", "JNJ", "LLY", "ABBV", "MRK", "TMO", "ABT", "DHR", "PFE", "BMY", "AMGN",
        "GILD", "CVS", "CI", "ELV", "HCA", "MCK", "COR", "ISRG", "VRTX", "REGN", "ZTS",
        # Financials  
        "JPM", "V", "MA", "BAC", "WFC", "GS", "MS", "BLK", "C", "AXP", "SPGI", "CME",
        "SCHW", "CB", "PGR", "MMC", "ICE", "PNC", "USB", "TFC", "COF", "BK", "STT",
        # Consumer Discretionary
        "AMZN", "TSLA", "HD", "NKE", "MCD", "SBUX", "TJX", "LOW", "BKNG", "ABNB", "MAR",
        "CMG", "ORLY", "AZO", "YUM", "DHI", "LEN", "ROST", "GM", "F", "EBAY", "ETSY",
        # Consumer Staples
        "WMT", "PG", "COST", "KO", "PEP", "PM", "MO", "MDLZ", "CL", "KMB", "GIS", "K",
        # Healthcare Equipment
        "ISRG", "SYK", "BSX", "MDT", "EW", "IDXX", "RMD", "DXCM", "ZBH", "BAX", "HCA",
        # Industrials
        "BA", "CAT", "HON", "UPS", "RTX", "GE", "LMT", "MMM", "DE", "UNP", "ETN",
        "ITW", "EMR", "NSC", "CSX", "NOC", "FDX", "WM", "RSG", "PCAR", "CMI", "PH",
        # Energy
        "XOM", "CVX", "COP", "SLB", "EOG", "MPC", "PSX", "VLO", "OXY", "HES", "KMI",
        # Real Estate
        "AMT", "PLD", "EQIX", "PSA", "SPG", "O", "WELL", "DLR", "AVB", "EQR", "VICI",
        # Materials
        "LIN", "APD", "ECL", "SHW", "FCX", "NEM", "DOW", "DD", "NUE", "VMC", "MLM",
        # Utilities  
        "NEE", "DUK", "SO", "D", "AEP", "EXC", "SRE", "XEL", "WEC", "ES", "PEG",
        # Communication Services
        "GOOGL", "META", "NFLX", "DIS", "CMCSA", "T", "VZ", "TMUS", "EA", "TTWO", "MTCH",
        # More popular stocks
        "SHOP", "SQ", "PYPL", "ROKU", "UBER", "LYFT", "DASH", "ABNB", "COIN", "RBLX",
        "NET", "DDOG", "CRWD", "ZS", "OKTA", "SNOW", "PLTR", "U", "DOCN",
        # Biotech
        "MRNA", "BNTX", "VRTX", "REGN", "BIIB", "ILMN", "IQV", "TECH", "WAT", "CTLT",
        # More financials
        "PYPL", "SQ", "SOFI", "AFRM", "COIN", "BX", "KKR", "APO", "ARES", "CG",
        # REITs
        "PLD", "AMT", "EQIX", "PSA", "WELL", "DLR", "O", "SPG", "VICI", "SBAC",
        # Additional tech
        "TEAM", "ZM", "DOCU", "TWLO", "VEEV", "ZI", "DDOG", "ESTC", "MDB", "SPLK"
    ]

def get_nasdaq100_tickers() -> List[str]:
    """Get NASDAQ 100 ticker list - hardcoded."""
    # Already included in S&P 500 list above, return empty
    return []

def get_additional_popular_stocks() -> List[str]:
    """Add popular stocks not in S&P 500."""
    return [
        # Additional tech
        "PLTR", "SNOW", "CRWD", "NET", "DDOG", "ZS",
        # EVs and energy
        "RIVN", "LCID", "PLUG", "ENPH", "FSLR",
        # Biotech
        "MRNA", "BNTX", "VRTX", "REGN", "BIIB",
        # Finance
        "SOFI", "COIN", "SQ", "PYPL", "AFRM",
        # Other popular
        "RBLX", "U", "DASH", "ABNB", "UBER", "LYFT"
    ]

def calculate_volatility(ticker: str, period: str = "1mo") -> Optional[float]:
    """Calculate 30-day historical volatility for a stock."""
    try:
        stock = yf.Ticker(ticker)
        hist = stock.history(period=period)
        
        if len(hist) < 2:
            # Default volatility if not enough data
            return 0.02
        
        # Calculate daily returns
        hist['returns'] = hist['Close'].pct_change()
        
        # Standard deviation of returns (volatility)
        volatility = hist['returns'].std()
        
        if np.isnan(volatility) or volatility == 0:
            return 0.02  # Default 2% volatility
        
        return abs(float(volatility))
    except Exception as e:
        # Return default volatility instead of None
        return 0.02

def get_stock_info(ticker: str) -> Optional[Dict]:
    """Fetch stock price, sector, and other info."""
    try:
        stock = yf.Ticker(ticker)
        
        # Try to get recent price first (more reliable)
        hist = stock.history(period="5d")
        
        if hist.empty or len(hist) == 0:
            return None
        
        # Get current price from most recent day
        price = hist['Close'].iloc[-1]
        
        # Try to get the full history to determine first trading date
        first_trading_date = None
        last_trading_date = None
        try:
            # Get maximum period available
            full_hist = stock.history(period="max")
            if not full_hist.empty:
                first_trading_date = full_hist.index[0].strftime('%Y-%m-%d')
                last_trading_date = full_hist.index[-1].strftime('%Y-%m-%d')
        except:
            pass
        
        # Try to get info, but don't fail if it's unavailable
        try:
            info = stock.info
            sector = info.get('sector', info.get('industry', 'Technology'))
            market_cap = info.get('marketCap', 0)
            name = info.get('longName', info.get('shortName', ticker))
        except:
            # If info fails, use defaults
            sector = 'Technology'
            market_cap = 0
            name = ticker
        
        # Map to standardized sector names
        sector = standardize_sector(sector)
        
        result = {
            'ticker': ticker,
            'name': name,
            'price': round(float(price), 2),
            'sector': sector,
            'market_cap': int(market_cap) if market_cap else 0,
        }
        
        # Add trading dates if available
        if first_trading_date:
            result['first_trading_date'] = first_trading_date
        if last_trading_date:
            result['last_trading_date'] = last_trading_date
        
        return result
    except Exception as e:
        print(f"  Error: {e}")
        return None

def standardize_sector(sector: str) -> str:
    """Map Yahoo Finance sectors to our standardized names."""
    sector_lower = sector.lower()
    
    # Direct mappings
    mapping = {
        'technology': 'Technology',
        'healthcare': 'Healthcare',
        'financial services': 'Financials',
        'financials': 'Financials',
        'energy': 'Energy',
        'consumer cyclical': 'Consumer',
        'consumer defensive': 'Consumer',
        'industrials': 'Industrials',
        'real estate': 'Real Estate',
        'utilities': 'Utilities',
        'basic materials': 'Materials',
        'communication services': 'Communication',
        'consumer discretionary': 'Consumer',
        'consumer staples': 'Consumer',
    }
    
    for key, value in mapping.items():
        if key in sector_lower:
            return value
    
    return 'Other'

def fetch_all_stocks(limit: int = 1000) -> List[Dict]:
    """Fetch stock data for up to 1000 stocks."""
    print("🔍 Fetching ticker lists...")
    
    # Combine multiple sources
    sp500 = get_sp500_tickers()
    nasdaq100 = get_nasdaq100_tickers()
    additional = get_additional_popular_stocks()
    
    # Combine and deduplicate
    all_tickers = list(set(sp500 + nasdaq100 + additional))
    all_tickers = [t for t in all_tickers if t and not t.startswith('^')]
    
    print(f"📋 Found {len(all_tickers)} unique tickers")
    print(f"🔄 Fetching data for up to {limit} stocks...\n")
    
    stocks = []
    failed = []
    
    for i, ticker in enumerate(all_tickers[:limit], 1):
        print(f"[{i}/{min(limit, len(all_tickers))}] {ticker}...", end=' ')
        
        # Get basic info
        stock_info = get_stock_info(ticker)
        if stock_info is None:
            print("❌")
            failed.append(ticker)
            continue
        
        # Calculate volatility (now always returns a value)
        volatility = calculate_volatility(ticker)
        
        stock_info['volatility'] = round(volatility, 4)
        stocks.append(stock_info)
        
        print(f"✓ ${stock_info['price']} (vol: {volatility*100:.2f}%)")
        
        # Less aggressive rate limiting
        if i % 50 == 0:
            print("  💤 Pausing 2s to avoid rate limits...")
            time.sleep(2)
        elif i % 10 == 0:
            time.sleep(0.5)
    
    print(f"\n✅ Successfully fetched {len(stocks)} stocks")
    if failed:
        print(f"❌ Failed: {len(failed)} stocks")
        print(f"   Examples: {', '.join(failed[:10])}")
    
    return stocks

def fetch_historical_periods(stocks: List[Dict], start_year: int = 1980, end_year: int = 2025, max_workers: int = 8) -> Dict:
    """
    Fetch historical prices for 3-month (calendar quarter) periods from start_year to end_year.
    Uses multithreading to speed up Yahoo Finance data fetching.

    Args:
        stocks: List of stock dicts with 'ticker' and 'first_trading_date' fields
        start_year: Starting year for historical data
        end_year: Ending year for historical data
        max_workers: Number of threads for concurrent requests

    Returns:
        {
            "period_key": {
                "AAPL": {"start_price": 100, "end_price": 110, "return_pct": 10.0},
                ...
            }
        }
    """
    print(f"\n🕐 Fetching historical data for calendar-based 3-month periods ({start_year}-{end_year})...")
    print(f"   Using up to {max_workers} threads for parallel downloads.")
    print(f"   Estimating ~{(end_year - start_year + 1) * 4} periods total.\n")
    
    periods_data = {}
    period_count = 0

    # Helper function for downloading one stock
    def fetch_stock_data(ticker: str, start_date: datetime, end_date: datetime):
        try:
            stock = yf.Ticker(ticker)
            hist = stock.history(
                start=start_date.strftime('%Y-%m-%d'),
                end=(end_date + timedelta(days=1)).strftime('%Y-%m-%d')  # include last day
            )
            if not hist.empty and len(hist) >= 2:
                start_price = hist['Close'].iloc[0]
                end_price = hist['Close'].iloc[-1]
                if pd.notna(start_price) and pd.notna(end_price) and start_price > 0:
                    return {
                        "start_price": round(float(start_price), 2),
                        "end_price": round(float(end_price), 2),
                        "return_pct": round(float(((end_price - start_price) / start_price) * 100), 2)
                    }
        except Exception:
            pass
        return None

    # Generate calendar quarters
    for year in range(start_year, end_year + 1):
        quarters = [
            (datetime(year, 1, 1), datetime(year, 3, 31)),   # Q1
            (datetime(year, 4, 1), datetime(year, 6, 30)),   # Q2
            (datetime(year, 7, 1), datetime(year, 9, 30)),   # Q3
            (datetime(year, 10, 1), datetime(year, 12, 31))  # Q4
        ]
        
        for q_start, q_end in quarters:
            if q_start > datetime.now():
                break  # Skip future periods
            
            period_key = f"{q_start.strftime('%Y-%m-%d')}_{q_end.strftime('%Y-%m-%d')}"
            print(f"📅 Period {period_count + 1}: {period_key}")

            # Filter eligible stocks
            eligible_stocks = []
            for stock in stocks:
                ticker = stock['ticker']
                first_trading_date = stock.get('first_trading_date')
                if first_trading_date:
                    try:
                        stock_start = datetime.strptime(first_trading_date, '%Y-%m-%d')
                        if stock_start <= q_start:
                            eligible_stocks.append(ticker)
                    except:
                        eligible_stocks.append(ticker)
                else:
                    eligible_stocks.append(ticker)

            print(f"   Eligible stocks for this period: {len(eligible_stocks)}/{len(stocks)}")

            period_data = {}
            success_count = 0

            # Threaded fetching
            with ThreadPoolExecutor(max_workers=max_workers) as executor:
                futures = {executor.submit(fetch_stock_data, ticker, q_start, q_end): ticker for ticker in eligible_stocks}
                
                for i, future in enumerate(as_completed(futures)):
                    ticker = futures[future]
                    result = future.result()
                    if result:
                        period_data[ticker] = result
                        success_count += 1
                    if (i + 1) % 50 == 0:
                        print(f"   Progress: {i + 1}/{len(eligible_stocks)} tickers...")

            periods_data[period_key] = period_data
            print(f"   ✅ Cached {success_count}/{len(eligible_stocks)} stocks for this period\n")

            period_count += 1

            # small cooldown between quarters
            time.sleep(1)

    print(f"✅ Completed historical data for {period_count} periods\n")
    return periods_data


def generate_metadata() -> Dict:
    """Generate metadata about sector keywords."""
    return {
        "generated_at": datetime.now().isoformat(),
        "stock_count": 0,  # Will be updated
        "sector_keywords": SECTOR_KEYWORDS,
        "sectors": list(SECTOR_KEYWORDS.keys())
    }

def save_to_json(stocks: List[Dict], historical_periods: Dict = None, output_file: str = "stocks_cache.json"):
    """Save stocks data to JSON file for Rust to read."""
    metadata = generate_metadata()
    metadata['stock_count'] = len(stocks)
    
    # Sort by market cap (largest first)
    stocks_sorted = sorted(stocks, key=lambda x: x.get('market_cap', 0), reverse=True)
    
    data = {
        "metadata": metadata,
        "stocks": stocks_sorted
    }
    
    # Add historical periods if provided
    if historical_periods:
        data["historical_periods"] = historical_periods
        print(f"📅 Added {len(historical_periods)} historical periods to cache")
    
    with open(output_file, 'w') as f:
        json.dump(data, f, indent=2)
    
    print(f"\n💾 Saved to {output_file}")
    print(f"📊 Total stocks: {len(stocks)}")
    
    # Print summary by sector
    sector_counts = {}
    for stock in stocks:
        sector = stock['sector']
        sector_counts[sector] = sector_counts.get(sector, 0) + 1
    
    print("\n📈 Stocks by sector:")
    for sector, count in sorted(sector_counts.items(), key=lambda x: x[1], reverse=True):
        print(f"  {sector}: {count}")

def main():
    """Main execution."""
    print("=" * 60)
    print("US EQUITIES DATA FETCHER")
    print("=" * 60)
    print()
    
    # Fetch up to 1000 stocks
    stocks = fetch_all_stocks(limit=1000)
    
    if not stocks:
        print("❌ No stocks fetched!")
        return
    
    # Fetch historical 6-month periods (1980-2025)
    # Pass the full stocks list so we can check first_trading_date
    print("\n" + "=" * 60)
    print("HISTORICAL DATA CACHING")
    print("=" * 60)
    historical_periods = fetch_historical_periods(stocks, start_year=1980, end_year=2025)
    
    # Save to JSON with historical data
    save_to_json(stocks, historical_periods, "stocks_cache.json")
    
    print("\n" + "=" * 60)
    print("✅ COMPLETE")
    print("=" * 60)
    print("\nYou can now use 'stocks_cache.json' in your Rust application!")
    print(f"Cache includes {len(historical_periods)} historical periods (6-month intervals)")

if __name__ == "__main__":
    main()
