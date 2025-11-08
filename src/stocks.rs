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
    #[serde(skip)]
    pub historical_start_price: Option<f64>, // Price at start of investment period
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

impl Stock {
    /// Get the price to use for portfolio quantity calculations.
    /// For backtesting competitions, use historical start price from the investment period.
    pub fn get_purchase_price(&self) -> f64 {
        // Use historical start price if available (backtesting scenario)
        // Otherwise fall back to current price
        self.historical_start_price.unwrap_or(self.price)
    }
}

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

/// Find periods surrounding a target date for interpolation
fn find_surrounding_periods(target_date: &str) -> Option<(String, String)> {
    let target = chrono::NaiveDate::parse_from_str(target_date, "%Y-%m-%d").ok()?;
    
    unsafe {
        if let Some(ref cache) = HISTORICAL_PERIODS_CACHE {
            let mut before_period: Option<(String, chrono::NaiveDate)> = None;
            let mut after_period: Option<(String, chrono::NaiveDate)> = None;
            
            for period_key in cache.keys() {
                let parts: Vec<&str> = period_key.split('_').collect();
                if parts.len() != 2 {
                    continue;
                }
                
                if let Ok(p_start) = chrono::NaiveDate::parse_from_str(parts[0], "%Y-%m-%d") {
                    if p_start <= target {
                        // This period is before or at target
                        match before_period {
                            Some((_, before_date)) if p_start > before_date => {
                                before_period = Some((period_key.clone(), p_start));
                            }
                            None => {
                                before_period = Some((period_key.clone(), p_start));
                            }
                            _ => {}
                        }
                    } else {
                        // This period is after target
                        match after_period {
                            Some((_, after_date)) if p_start < after_date => {
                                after_period = Some((period_key.clone(), p_start));
                            }
                            None => {
                                after_period = Some((period_key.clone(), p_start));
                            }
                            _ => {}
                        }
                    }
                }
            }
            
            if let (Some((before_key, _)), Some((after_key, _))) = (before_period, after_period) {
                return Some((before_key, after_key));
            }
        }
    }
    
    None
}

/// Interpolate stock price between two cached periods
fn interpolate_price(ticker: &str, target_date: &str, before_period: &str, after_period: &str) -> Option<f64> {
    let target = chrono::NaiveDate::parse_from_str(target_date, "%Y-%m-%d").ok()?;
    
    // Parse period dates
    let before_parts: Vec<&str> = before_period.split('_').collect();
    let after_parts: Vec<&str> = after_period.split('_').collect();
    
    let before_date = chrono::NaiveDate::parse_from_str(before_parts[0], "%Y-%m-%d").ok()?;
    let after_date = chrono::NaiveDate::parse_from_str(after_parts[0], "%Y-%m-%d").ok()?;
    
    unsafe {
        if let Some(ref cache) = HISTORICAL_PERIODS_CACHE {
            let before_data = cache.get(before_period)?.get(ticker)?;
            let after_data = cache.get(after_period)?.get(ticker)?;
            
            // Linear interpolation
            let total_days = (after_date - before_date).num_days() as f64;
            let target_days = (target - before_date).num_days() as f64;
            let ratio = target_days / total_days;
            
            let interpolated = before_data.start_price + 
                              (after_data.start_price - before_data.start_price) * ratio;
            
            Some(interpolated)
        } else {
            None
        }
    }
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
            
            // Find period that contains or is closest to the start date
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
                    // Calculate distance from period start to investment start
                    let distance = (start - p_start).num_days().abs();
                    
                    // Prefer periods that contain or are close to the start date
                    if p_start <= start && p_end >= start {
                        // Period contains start date - perfect match
                        return Some(period_key.clone());
                    }
                    
                    // Otherwise, track closest period
                    match best_match {
                        Some((_, best_distance)) if distance < best_distance => {
                            best_match = Some((period_key.clone(), distance));
                        }
                        None => {
                            best_match = Some((period_key.clone(), distance));
                        }
                        _ => {}
                    }
                }
            }
            
            return best_match.map(|(key, _)| key);
        }
    }
    
    None
}

/// Fetch historical returns for stocks during a specific date range
/// First tries to use cached data with interpolation, falls back to API if not available
pub async fn fetch_historical_returns(
    stocks: &mut [Stock], 
    start_date: &str,  // Format: YYYY-MM-DD
    end_date: &str     // Format: YYYY-MM-DD
) -> Result<(), Box<dyn Error>> {
    let mut cache_hits = 0;
    let mut interpolated_hits = 0;
    let mut cache_misses = 0;
    
    // Try to use cached historical data first (exact match or close period)
    if let Some(period_key) = find_matching_period(start_date, end_date) {
        println!("📊 Using cached historical period: {}", period_key);
        
        unsafe {
            if let Some(ref cache) = HISTORICAL_PERIODS_CACHE {
                if let Some(period_data) = cache.get(&period_key) {
                    for stock in stocks.iter_mut() {
                        if let Some(hist_data) = period_data.get(&stock.ticker) {
                            stock.historical_return = Some(hist_data.return_pct);
                            stock.historical_start_price = Some(hist_data.start_price);
                            cache_hits += 1;
                        } else {
                            cache_misses += 1;
                        }
                    }
                }
            }
        }
        
        println!("✅ Loaded from nearest cached period: {} hits, {} misses", cache_hits, cache_misses);
        
        // Try interpolation for better accuracy if we have surrounding periods
        if let Some((before_period, after_period)) = find_surrounding_periods(start_date) {
            println!("📊 Refining with interpolation between {} and {}", before_period, after_period);
            
            for stock in stocks.iter_mut() {
                if stock.historical_start_price.is_some() {
                    // Try to interpolate a more accurate start price
                    if let Some(interpolated_price) = interpolate_price(&stock.ticker, start_date, &before_period, &after_period) {
                        // Recalculate return with interpolated start price
                        if let Some(original_start) = stock.historical_start_price {
                            // Estimate end price proportionally
                            if let Some(original_return) = stock.historical_return {
                                let end_price = original_start * (1.0 + original_return / 100.0);
                                let new_return = ((end_price - interpolated_price) / interpolated_price) * 100.0;
                                
                                stock.historical_start_price = Some(interpolated_price);
                                stock.historical_return = Some(new_return);
                                interpolated_hits += 1;
                            }
                        }
                    }
                }
            }
            
            if interpolated_hits > 0 {
                println!("✅ Interpolated {} stock prices for better accuracy", interpolated_hits);
            }
        }
        
        // If we got data for most stocks, we're done with Phase 1 (selection)
        if cache_hits > cache_misses {
            return Ok(());
        }
    }
    
    // Fallback to API if cache unavailable or incomplete
    println!("⚠️  Falling back to API for historical data...");
    println!("⚠️  WARNING: This will be VERY SLOW (~10 seconds per stock)");
    println!("⚠️  RECOMMENDATION: Run 'python3 fetch_stocks.py' to generate cache first!");
    
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
    let stocks_to_fetch: Vec<&mut Stock> = stocks.iter_mut()
        .filter(|s| s.historical_return.is_none())
        .collect();
    
    let total_to_fetch = stocks_to_fetch.len();
    println!("📊 Fetching data for {} stocks via API...", total_to_fetch);
    
    for (i, stock) in stocks_to_fetch.into_iter().enumerate() {
        if i % 10 == 0 {
            println!("   Progress: {}/{} stocks...", i, total_to_fetch);
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
                                                    stock.historical_start_price = Some(start_price);
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

/// Fetch exact historical prices for specific stocks (Phase 2: after selection)
/// This is called only for the stocks chosen for the portfolio to get precise pricing
pub async fn fetch_exact_prices_for_selected(
    stocks: &mut [Stock],
    start_date: &str,
    end_date: &str
) -> Result<(), Box<dyn Error>> {
    println!("💎 Fetching exact historical prices for {} selected stocks...", stocks.len());
    
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
        let url = format!(
            "https://query1.finance.yahoo.com/v8/finance/chart/{}?period1={}&period2={}&interval=1d",
            stock.ticker, start_timestamp, end_timestamp
        );
        
        match client.get(&url).send().await {
            Ok(resp) => {
                if let Ok(text) = resp.text().await {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                        // Check for API error message
                        if let Some(error) = json["chart"]["error"].as_object() {
                            if let Some(description) = error["description"].as_str() {
                                fail_count += 1;
                                println!("  ✗ {}: Yahoo API error: {}", stock.ticker, description);
                                continue;
                            }
                        }
                        
                        if let Some(result) = json["chart"]["result"].as_array() {
                            if result.is_empty() {
                                fail_count += 1;
                                println!("  ✗ {}: No data returned (possibly didn't exist in {} or delisted)", 
                                        stock.ticker, start_date.split('-').next().unwrap_or(""));
                                continue;
                            }
                            
                            if let Some(first) = result.first() {
                                if let Some(quotes) = first["indicators"]["quote"].as_array() {
                                    if let Some(quote_data) = quotes.first() {
                                        if let Some(closes) = quote_data["close"].as_array() {
                                            if closes.is_empty() {
                                                fail_count += 1;
                                                println!("  ✗ {}: No price data for this period", stock.ticker);
                                                continue;
                                            }
                                            
                                            let first_close = closes.iter()
                                                .find_map(|v| v.as_f64());
                                            let last_close = closes.iter()
                                                .rev()
                                                .find_map(|v| v.as_f64());
                                            
                                            if let (Some(start_price), Some(end_price)) = (first_close, last_close) {
                                                if start_price > 0.0 {
                                                    let return_pct = ((end_price - start_price) / start_price) * 100.0;
                                                    stock.historical_return = Some(return_pct);
                                                    stock.historical_start_price = Some(start_price);
                                                    success_count += 1;
                                                    println!("  ✓ {}: ${:.2} → ${:.2} ({:+.1}%)", 
                                                            stock.ticker, start_price, end_price, return_pct);
                                                    continue;
                                                }
                                            } else {
                                                fail_count += 1;
                                                println!("  ✗ {}: Could not parse price data", stock.ticker);
                                                continue;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                fail_count += 1;
                println!("  ✗ {}: Failed to parse response", stock.ticker);
            }
            Err(e) => {
                fail_count += 1;
                println!("  ✗ {}: Network error: {}", stock.ticker, e);
            }
        }
        
        // Rate limiting
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    }
    
    println!("✅ Exact prices: {} success, {} failed\n", success_count, fail_count);
    
    Ok(())
}
