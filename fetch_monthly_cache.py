#!/usr/bin/env python3
"""
Incremental monthly price cache builder with checkpoint/resume support.
Can be interrupted and resumed - picks up where it left off.

Run: python3 fetch_monthly_cache.py
Resume: python3 fetch_monthly_cache.py (automatically detects and resumes)
"""

import yfinance as yf
import pandas as pd
import json
import os
from datetime import datetime
from typing import Dict, List, Optional
import time

# File paths
CHECKPOINT_FILE = "monthly_cache_checkpoint.json"
FINAL_CACHE_FILE = "stocks_cache_monthly.json"
BASE_CACHE_FILE = "stocks_cache.json"

def load_base_stocks() -> List[Dict]:
    """Load stock list from existing cache."""
    print("📂 Loading base stock list from stocks_cache.json...")
    
    if not os.path.exists(BASE_CACHE_FILE):
        print(f"❌ Error: {BASE_CACHE_FILE} not found!")
        print("   Run 'python3 fetch_stocks.py' first to generate base cache.")
        exit(1)
    
    with open(BASE_CACHE_FILE, 'r') as f:
        cache = json.load(f)
    
    stocks = cache.get('stocks', [])
    print(f"✅ Loaded {len(stocks)} stocks\n")
    return stocks

def load_checkpoint() -> Dict:
    """Load existing checkpoint if it exists."""
    if os.path.exists(CHECKPOINT_FILE):
        print(f"🔄 Found checkpoint file: {CHECKPOINT_FILE}")
        with open(CHECKPOINT_FILE, 'r') as f:
            checkpoint = json.load(f)
        print(f"   Resuming from: {checkpoint['last_completed_ticker']}")
        print(f"   Progress: {checkpoint['completed_count']}/{checkpoint['total_count']} stocks")
        print(f"   Last saved: {checkpoint['last_updated']}\n")
        return checkpoint
    return None

def save_checkpoint(monthly_data: Dict, completed_ticker: str, completed_count: int, total_count: int):
    """Save checkpoint after each stock."""
    checkpoint = {
        "monthly_prices": monthly_data,
        "last_completed_ticker": completed_ticker,
        "completed_count": completed_count,
        "total_count": total_count,
        "last_updated": datetime.now().isoformat()
    }
    
    with open(CHECKPOINT_FILE, 'w') as f:
        json.dump(checkpoint, f, indent=2)

def fetch_monthly_prices_for_stock(ticker: str, start_year: int = 1980) -> Optional[Dict]:
    """Fetch end-of-month prices for a single stock."""
    try:
        stock = yf.Ticker(ticker)
        hist = stock.history(start=f"{start_year}-01-01", period="max")
        
        if hist.empty or len(hist) < 2:
            return None
        
        # Resample to month-end prices
        monthly = hist['Close'].resample('M').last()
        
        if len(monthly) == 0:
            return None
        
        # Format: YYYY-MM for compact storage
        dates = [d.strftime('%Y-%m') for d in monthly.index]
        prices = [round(float(p), 2) for p in monthly.values]
        
        # Also store first/last dates for quick reference
        first_date = hist.index[0].strftime('%Y-%m-%d')
        last_date = hist.index[-1].strftime('%Y-%m-%d')
        
        return {
            "dates": dates,
            "prices": prices,
            "first_trading": first_date,
            "last_trading": last_date,
            "data_points": len(dates)
        }
    
    except Exception as e:
        print(f"      ⚠️  Error: {e}")
        return None

def build_monthly_cache(stocks: List[Dict], start_year: int = 1980):
    """Build monthly price cache with checkpoint support."""
    print("=" * 70)
    print("INCREMENTAL MONTHLY PRICE CACHE BUILDER")
    print("=" * 70)
    print(f"Start year: {start_year}")
    print(f"Checkpoint file: {CHECKPOINT_FILE}")
    print(f"Output file: {FINAL_CACHE_FILE}")
    print("=" * 70)
    print()
    
    # Load checkpoint if exists
    checkpoint = load_checkpoint()
    
    if checkpoint:
        monthly_data = checkpoint["monthly_prices"]
        start_idx = checkpoint["completed_count"]
        completed_tickers = set(monthly_data.keys())
    else:
        print("🆕 Starting fresh (no checkpoint found)")
        monthly_data = {}
        start_idx = 0
        completed_tickers = set()
    
    total = len(stocks)
    success_count = len(monthly_data)
    failed_count = 0
    skipped_count = 0
    
    print(f"\n📊 Processing {total - start_idx} remaining stocks...\n")
    
    start_time = time.time()
    
    for i in range(start_idx, total):
        stock = stocks[i]
        ticker = stock['ticker']
        
        # Skip if already processed
        if ticker in completed_tickers:
            skipped_count += 1
            continue
        
        elapsed = time.time() - start_time
        avg_time = elapsed / (i - start_idx + 1) if i > start_idx else 0
        remaining = (total - i - 1) * avg_time
        
        print(f"[{i+1}/{total}] {ticker:6s} ", end='')
        print(f"(⏱️  {elapsed:.0f}s elapsed, ~{remaining:.0f}s remaining)...", end=' ')
        
        # Fetch monthly data
        data = fetch_monthly_prices_for_stock(ticker, start_year)
        
        if data:
            monthly_data[ticker] = data
            success_count += 1
            print(f"✅ {data['data_points']} months ({data['first_trading'][:4]}-{data['last_trading'][:4]})")
            
            # Save checkpoint every 5 stocks
            if (i + 1) % 5 == 0:
                save_checkpoint(monthly_data, ticker, i + 1, total)
                print(f"      💾 Checkpoint saved ({i+1}/{total})")
        else:
            failed_count += 1
            print(f"❌")
        
        # Rate limiting - be nice to Yahoo
        if i % 10 == 0 and i > start_idx:
            time.sleep(0.5)
        else:
            time.sleep(0.1)
        
        # Progress update every 25 stocks
        if (i + 1) % 25 == 0:
            print(f"\n   📈 Progress: {success_count} successful, {failed_count} failed")
            print(f"   💾 Cache size: ~{len(json.dumps(monthly_data)) / 1024:.0f} KB\n")
    
    # Final save
    print(f"\n{'=' * 70}")
    print("FETCHING COMPLETE")
    print(f"{'=' * 70}")
    print(f"✅ Success: {success_count}")
    print(f"❌ Failed: {failed_count}")
    print(f"⏭️  Skipped (already done): {skipped_count}")
    print()
    
    return monthly_data

def save_final_cache(monthly_data: Dict, stocks: List[Dict]):
    """Save final optimized cache file."""
    print("💾 Saving final cache file...")
    
    # Load metadata from base cache
    with open(BASE_CACHE_FILE, 'r') as f:
        base_cache = json.load(f)
    
    # Build final cache structure
    final_cache = {
        "metadata": {
            "generated_at": datetime.now().isoformat(),
            "format": "monthly_prices",
            "description": "End-of-month closing prices for all stocks",
            "start_year": 1980,
            "stock_count": len(stocks),
            "cached_tickers": len(monthly_data),
            "sector_keywords": base_cache["metadata"]["sector_keywords"],
            "sectors": base_cache["metadata"]["sectors"]
        },
        "stocks": stocks,  # Keep base stock info
        "monthly_prices": monthly_data  # Add monthly price data
    }
    
    # Save to file
    with open(FINAL_CACHE_FILE, 'w') as f:
        json.dump(final_cache, f, indent=2)
    
    # Get file size
    file_size = os.path.getsize(FINAL_CACHE_FILE) / (1024 * 1024)
    
    print(f"✅ Saved to: {FINAL_CACHE_FILE}")
    print(f"📦 File size: {file_size:.2f} MB")
    print(f"📊 Data coverage: {len(monthly_data)}/{len(stocks)} stocks")
    
    # Calculate total data points
    total_months = sum(data.get('data_points', 0) for data in monthly_data.values())
    print(f"📅 Total monthly data points: {total_months:,}")
    
    # Show sample
    if monthly_data:
        sample_ticker = list(monthly_data.keys())[0]
        sample_data = monthly_data[sample_ticker]
        print(f"\n📝 Sample (${sample_ticker}):")
        print(f"   First 3 months: {sample_data['dates'][:3]}")
        print(f"   First 3 prices: {sample_data['prices'][:3]}")
        print(f"   Trading period: {sample_data['first_trading']} to {sample_data['last_trading']}")
    
    # Keep checkpoint for future incremental updates
    print(f"\n💡 Checkpoint file kept at: {CHECKPOINT_FILE}")
    print("   (You can delete it if you want, or keep it for future updates)")

def cleanup_checkpoint():
    """Remove checkpoint file after successful completion."""
    if os.path.exists(CHECKPOINT_FILE):
        print(f"\n🧹 Cleaning up checkpoint file...")
        try:
            os.remove(CHECKPOINT_FILE)
            print(f"   ✅ Removed {CHECKPOINT_FILE}")
        except:
            print(f"   ⚠️  Could not remove {CHECKPOINT_FILE} (manual cleanup needed)")

def main():
    """Main execution with resume support."""
    print()
    print("╔" + "=" * 68 + "╗")
    print("║" + " " * 15 + "MONTHLY PRICE CACHE BUILDER" + " " * 25 + "║")
    print("║" + " " * 15 + "(Incremental with Resume)" + " " * 27 + "║")
    print("╚" + "=" * 68 + "╝")
    print()
    
    # Load base stock list
    stocks = load_base_stocks()
    
    # Build monthly cache (will resume if interrupted)
    monthly_data = build_monthly_cache(stocks, start_year=1980)
    
    # Save final cache
    save_final_cache(monthly_data, stocks)
    
    print()
    print("=" * 70)
    print("✅ ALL DONE!")
    print("=" * 70)
    print()
    print("Next steps:")
    print(f"  1. Use '{FINAL_CACHE_FILE}' in your Rust application")
    print(f"  2. Update src/stocks.rs to read from monthly_prices format")
    print()
    print("To resume if interrupted: Just run this script again!")
    print()

if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        print("\n\n⚠️  INTERRUPTED!")
        print(f"✅ Progress saved to: {CHECKPOINT_FILE}")
        print("   Run this script again to resume where you left off.")
        print()
        exit(0)
    except Exception as e:
        print(f"\n❌ Error: {e}")
        print(f"   Progress saved to: {CHECKPOINT_FILE}")
        print("   You can resume by running the script again.")
        exit(1)
