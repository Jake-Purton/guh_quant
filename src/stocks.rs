use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stock {
    pub ticker: String,
    pub price: f64,
    pub sector: String,
    pub volatility: f64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub market_cap: u64,
    #[serde(default)]
    pub first_trading_date: Option<String>,
    #[serde(default)]
    pub last_trading_date: Option<String>,
    #[serde(skip)]
    pub historical_return: Option<f64>, // Actual return % during investment period
}

#[derive(Debug, Deserialize)]
struct StockCache {
    metadata: Metadata,
    stocks: Vec<Stock>,
    #[serde(default)]
    historical_periods: Option<HashMap<String, HashMap<String, HistoricalData>>>,
}

#[derive(Debug, Deserialize)]
struct Metadata {
    generated_at: String,
    stock_count: usize,
    sector_keywords: HashMap<String, Vec<String>>,
    sectors: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct HistoricalData {
    start_price: f64,
    end_price: f64,
    return_pct: f64,
}

// Global cache for historical periods
static mut HISTORICAL_PERIODS_CACHE: Option<HashMap<String, HashMap<String, HistoricalData>>> = None;

pub fn load_stocks_from_cache(cache_file: &str) -> Result<Vec<Stock>, Box<dyn Error>> {
    println!("📂 Loading stocks from cache: {}", cache_file);
    
    let contents = fs::read_to_string(cache_file)
        .map_err(|e| format!("Failed to read cache file '{}': {}. Run fetch_stocks.py first!", cache_file, e))?;
    
    let cache: StockCache = serde_json::from_str(&contents)?;
    
    println!("✅ Loaded {} stocks from cache (generated: {})", 
             cache.stocks.len(), 
             cache.metadata.generated_at);
    
    // Store historical periods in global cache
    if let Some(periods) = cache.historical_periods {
        println!("📊 Loaded {} historical periods from cache", periods.len());
        unsafe {
            HISTORICAL_PERIODS_CACHE = Some(periods);
        }
    } else {
        println!("⚠️  No historical periods in cache - will use API fallback");
    }
    
    Ok(cache.stocks)
}

pub async fn prefetch_all_stocks() -> Result<Vec<Stock>, Box<dyn Error>> {
    // Try to load from cache first
    match load_stocks_from_cache("stocks_cache.json") {
        Ok(stocks) => {
            println!("📊 Using cached stock data\n");
            Ok(stocks)
        }
        Err(e) => {
            println!("⚠️  Cache not found: {}", e);
            println!("ℹ️  Run 'python3 fetch_stocks.py' to generate the cache file\n");
            Err(e)
        }
    }
}

/// Update stock prices from Yahoo Finance API
pub async fn update_stock_prices(stocks: &mut [Stock]) -> Result<(), Box<dyn Error>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;
    
    let mut success_count = 0;
    let mut fail_count = 0;
    
    for stock in stocks.iter_mut() {
        // Yahoo Finance quote API endpoint
        let url = format!(
            "https://query1.finance.yahoo.com/v8/finance/chart/{}?interval=1d&range=1d",
            stock.ticker
        );
        
        match client.get(&url).send().await {
            Ok(resp) => {
                if let Ok(text) = resp.text().await {
                    // Parse the JSON response to extract current price
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                        if let Some(result) = json["chart"]["result"].as_array() {
                            if let Some(first) = result.first() {
                                if let Some(quote) = first["meta"]["regularMarketPrice"].as_f64() {
                                    stock.price = (quote * 100.0).round() / 100.0; // Round to 2 decimals
                                    success_count += 1;
                                    continue;
                                }
                            }
                        }
                    }
                }
                fail_count += 1;
            }
            Err(_) => {
                fail_count += 1;
            }
        }
    }
    
    if success_count > 0 {
        println!("🔄 Updated {} stock prices ({} failed)", success_count, fail_count);
    }
    
    Ok(())
}

/// Find the best matching historical period for the given date range
fn find_matching_period(start_date: &str, end_date: &str) -> Option<String> {
    // Try exact match first
    let exact_key = format!("{}_{}", start_date, end_date);
    
    unsafe {
        if let Some(ref cache) = HISTORICAL_PERIODS_CACHE {
            if cache.contains_key(&exact_key) {
                return Some(exact_key);
            }
            
            // Parse input dates
            let start = chrono::NaiveDate::parse_from_str(start_date, "%Y-%m-%d").ok()?;
            let end = chrono::NaiveDate::parse_from_str(end_date, "%Y-%m-%d").ok()?;
            
            // Find overlapping period with best coverage
            let mut best_match: Option<(String, i64)> = None;
            
            for period_key in cache.keys() {
                let parts: Vec<&str> = period_key.split('_').collect();
                if parts.len() != 2 {
                    continue;
                }
                
                if let (Ok(p_start), Ok(p_end)) = (
                    chrono::NaiveDate::parse_from_str(parts[0], "%Y-%m-%d"),
                    chrono::NaiveDate::parse_from_str(parts[1], "%Y-%m-%d")
                ) {
                    // Check for overlap
                    if p_start <= end && p_end >= start {
                        // Calculate overlap duration
                        let overlap_start = std::cmp::max(p_start, start);
                        let overlap_end = std::cmp::min(p_end, end);
                        let overlap_days = (overlap_end - overlap_start).num_days();
                        
                        if overlap_days > 0 {
                            match best_match {
                                Some((_, best_days)) if overlap_days > best_days => {
                                    best_match = Some((period_key.clone(), overlap_days));
                                }
                                None => {
                                    best_match = Some((period_key.clone(), overlap_days));
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            
            return best_match.map(|(key, _)| key);
        }
    }
    
    None
}

/// Fetch historical returns for stocks during a specific date range
/// First tries to use cached data, falls back to API if not available
pub async fn fetch_historical_returns(
    stocks: &mut [Stock], 
    start_date: &str,  // Format: YYYY-MM-DD
    end_date: &str     // Format: YYYY-MM-DD
) -> Result<(), Box<dyn Error>> {
    let mut cache_hits = 0;
    let mut cache_misses = 0;
    
    // Try to use cached historical data first
    if let Some(period_key) = find_matching_period(start_date, end_date) {
        println!("📊 Using cached historical period: {}", period_key);
        
        unsafe {
            if let Some(ref cache) = HISTORICAL_PERIODS_CACHE {
                if let Some(period_data) = cache.get(&period_key) {
                    for stock in stocks.iter_mut() {
                        if let Some(hist_data) = period_data.get(&stock.ticker) {
                            stock.historical_return = Some(hist_data.return_pct);
                            cache_hits += 1;
                        } else {
                            cache_misses += 1;
                        }
                    }
                }
            }
        }
        
        println!("✅ Loaded historical returns from cache: {} hits, {} misses", cache_hits, cache_misses);
        
        // If we got data for most stocks, we're done
        if cache_hits > cache_misses {
            return Ok(());
        }
    }
    
    // Fallback to API if cache unavailable or incomplete
    println!("⚠️  Falling back to API for historical data...");
    
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    
    // Convert dates to Unix timestamps
    let start_timestamp = chrono::NaiveDate::parse_from_str(start_date, "%Y-%m-%d")?
        .and_hms_opt(0, 0, 0).unwrap()
        .and_utc()
        .timestamp();
    
    let end_timestamp = chrono::NaiveDate::parse_from_str(end_date, "%Y-%m-%d")?
        .and_hms_opt(0, 0, 0).unwrap()
        .and_utc()
        .timestamp();
    
    let mut success_count = 0;
    let mut fail_count = 0;
    
    for stock in stocks.iter_mut() {
        // Skip if we already have data from cache
        if stock.historical_return.is_some() {
            continue;
        }
        
        // Yahoo Finance historical data endpoint
        let url = format!(
            "https://query1.finance.yahoo.com/v8/finance/chart/{}?period1={}&period2={}&interval=1d",
            stock.ticker, start_timestamp, end_timestamp
        );
        
        match client.get(&url).send().await {
            Ok(resp) => {
                if let Ok(text) = resp.text().await {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                        if let Some(result) = json["chart"]["result"].as_array() {
                            if let Some(first) = result.first() {
                                if let Some(quotes) = first["indicators"]["quote"].as_array() {
                                    if let Some(quote_data) = quotes.first() {
                                        if let Some(closes) = quote_data["close"].as_array() {
                                            // Get first and last close prices
                                            let first_close = closes.iter()
                                                .find_map(|v| v.as_f64());
                                            let last_close = closes.iter()
                                                .rev()
                                                .find_map(|v| v.as_f64());
                                            
                                            if let (Some(start_price), Some(end_price)) = (first_close, last_close) {
                                                if start_price > 0.0 {
                                                    let return_pct = ((end_price - start_price) / start_price) * 100.0;
                                                    stock.historical_return = Some(return_pct);
                                                    success_count += 1;
                                                    continue;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                fail_count += 1;
            }
            Err(_) => {
                fail_count += 1;
            }
        }
        
        // Small delay to avoid rate limiting
        if success_count % 10 == 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }
    
    println!("📈 Fetched historical returns from API: {} success, {} failed", success_count, fail_count);
    
    Ok(())
}
