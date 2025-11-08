//! Stock data management and historical price fetching
//! 
//! This module handles:
//! - Loading stock data from cache
//! - Fetching historical returns with interpolation
//! - Updating current prices from Yahoo Finance API

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
    #[allow(dead_code)]
    stock_count: usize,
    #[allow(dead_code)]
    sector_keywords: HashMap<String, Vec<String>>,
    #[allow(dead_code)]
    sectors: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct HistoricalData {
    start_price: f64,
    #[allow(dead_code)]
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
    println!("[CACHE] Loading stocks from cache: {}", cache_file);
    
    let contents = fs::read_to_string(cache_file)
        .map_err(|e| format!("Failed to read cache file '{}': {}. Run fetch_stocks.py first!", cache_file, e))?;
    
    let cache: StockCache = serde_json::from_str(&contents)?;
    
    println!("[CACHE] Loaded {} stocks from cache (generated: {})", 
             cache.stocks.len(), 
             cache.metadata.generated_at);
    
    // Store historical periods in global cache
    if let Some(periods) = cache.historical_periods {
        println!("[CACHE] Loaded {} historical periods from cache", periods.len());
        unsafe {
            HISTORICAL_PERIODS_CACHE = Some(periods);
        }
    } else {
        println!("[WARN] No historical periods in cache - will use API fallback");
    }
    
    Ok(cache.stocks)
}

pub async fn prefetch_all_stocks() -> Result<Vec<Stock>, Box<dyn Error>> {
    // Try to load from cache first
    match load_stocks_from_cache("stocks_cache.json") {
        Ok(stocks) => {
            println!("[CACHE] Using cached stock data\n");
            Ok(stocks)
        }
        Err(e) => {
            println!("[WARN] Cache not found: {}", e);
            println!("[INFO] Run 'python3 fetch_stocks.py' to generate the cache file\n");
            Err(e)
        }
    }
}

/// Parse a period key (format: "YYYY-MM-DD_YYYY-MM-DD") into start and end dates
fn parse_period_key(period_key: &str) -> Option<(chrono::NaiveDate, chrono::NaiveDate)> {
    let parts: Vec<&str> = period_key.split('_').collect();
    if parts.len() != 2 {
        return None;
    }
    
    let start = chrono::NaiveDate::parse_from_str(parts[0], "%Y-%m-%d").ok()?;
    let end = chrono::NaiveDate::parse_from_str(parts[1], "%Y-%m-%d").ok()?;
    Some((start, end))
}

/// Find periods surrounding a target date for interpolation
/// Returns (before_period_key, after_period_key) where before <= target < after
fn find_surrounding_periods(target_date: &str) -> Option<(String, String)> {
    let target = chrono::NaiveDate::parse_from_str(target_date, "%Y-%m-%d").ok()?;
    
    unsafe {
        let cache = HISTORICAL_PERIODS_CACHE.as_ref()?;
        
        let mut before_period: Option<(String, chrono::NaiveDate)> = None;
        let mut after_period: Option<(String, chrono::NaiveDate)> = None;
        
        for period_key in cache.keys() {
            let (p_start, _p_end) = parse_period_key(period_key)?;
            
            if p_start <= target {
                // This period starts before or at target - candidate for "before"
                if before_period.is_none() || p_start > before_period.as_ref()?.1 {
                    before_period = Some((period_key.clone(), p_start));
                }
            } else {
                // This period starts after target - candidate for "after"
                if after_period.is_none() || p_start < after_period.as_ref()?.1 {
                    after_period = Some((period_key.clone(), p_start));
                }
            }
        }
        
        match (before_period, after_period) {
            (Some((before_key, _)), Some((after_key, _))) => Some((before_key, after_key)),
            _ => None,
        }
    }
}

/// Linear interpolation between two values
fn linear_interpolate(start_value: f64, end_value: f64, ratio: f64) -> f64 {
    start_value + (end_value - start_value) * ratio
}

/// Interpolate stock price between two cached periods using linear interpolation
fn interpolate_price(ticker: &str, target_date: &str, before_period: &str, after_period: &str) -> Option<f64> {
    let target = chrono::NaiveDate::parse_from_str(target_date, "%Y-%m-%d").ok()?;
    let (before_date, _) = parse_period_key(before_period)?;
    let (after_date, _) = parse_period_key(after_period)?;
    
    unsafe {
        let cache = HISTORICAL_PERIODS_CACHE.as_ref()?;
        let before_data = cache.get(before_period)?.get(ticker)?;
        let after_data = cache.get(after_period)?.get(ticker)?;
        
        // Calculate interpolation ratio based on time position
        let total_days = (after_date - before_date).num_days() as f64;
        let target_days = (target - before_date).num_days() as f64;
        let ratio = target_days / total_days;
        
        let interpolated = linear_interpolate(
            before_data.start_price,
            after_data.start_price,
            ratio
        );
        
        Some(interpolated)
    }
}

/// Find the best matching historical period for the given date range
/// Priority: 1) Exact match, 2) Period containing start date, 3) Closest period to start date
fn find_matching_period(start_date: &str, end_date: &str) -> Option<String> {
    let exact_key = format!("{}_{}", start_date, end_date);
    let start = chrono::NaiveDate::parse_from_str(start_date, "%Y-%m-%d").ok()?;
    
    unsafe {
        let cache = HISTORICAL_PERIODS_CACHE.as_ref()?;
        
        // Priority 1: Exact match
        if cache.contains_key(&exact_key) {
            return Some(exact_key);
        }
        
        let mut best_match: Option<(String, i64)> = None;
        
        // Priority 2: Period containing start date, Priority 3: Closest period
        for period_key in cache.keys() {
            let (p_start, p_end) = parse_period_key(period_key)?;
            
            // Check if period contains the start date
            if p_start <= start && p_end >= start {
                return Some(period_key.clone());
            }
            
            // Track closest period by distance to start date
            let distance = (start - p_start).num_days().abs();
            if best_match.is_none() || distance < best_match.as_ref()?.1 {
                best_match = Some((period_key.clone(), distance));
            }
        }
        
        best_match.map(|(key, _)| key)
    }
}

/// Apply cached historical data to stocks from a specific period
fn apply_cached_period_data(stocks: &mut [Stock], period_key: &str) -> (usize, usize) {
    let mut hits = 0;
    let mut misses = 0;
    
    unsafe {
        if let Some(ref cache) = HISTORICAL_PERIODS_CACHE {
            if let Some(period_data) = cache.get(period_key) {
                for stock in stocks.iter_mut() {
                    if let Some(hist_data) = period_data.get(&stock.ticker) {
                        stock.historical_return = Some(hist_data.return_pct);
                        stock.historical_start_price = Some(hist_data.start_price);
                        hits += 1;
                    } else {
                        misses += 1;
                    }
                }
            }
        }
    }
    
    (hits, misses)
}

/// Refine stock prices using interpolation for better accuracy
fn apply_interpolation_refinement(stocks: &mut [Stock], start_date: &str, before_period: &str, after_period: &str) -> usize {
    let mut refined_count = 0;
    
    for stock in stocks.iter_mut() {
        if stock.historical_start_price.is_none() {
            continue;
        }
        
        if let Some(interpolated_price) = interpolate_price(&stock.ticker, start_date, before_period, after_period) {
            // Recalculate return with more accurate interpolated start price
            if let (Some(original_start), Some(original_return)) = (stock.historical_start_price, stock.historical_return) {
                let end_price = original_start * (1.0 + original_return / 100.0);
                let new_return = ((end_price - interpolated_price) / interpolated_price) * 100.0;
                
                stock.historical_start_price = Some(interpolated_price);
                stock.historical_return = Some(new_return);
                refined_count += 1;
            }
        }
    }
    
    refined_count
}

/// Fetch historical returns from cache (Phase 1: Fast selection using cached data)
fn fetch_from_cache(stocks: &mut [Stock], start_date: &str, end_date: &str) -> Result<bool, Box<dyn Error>> {
    let period_key = match find_matching_period(start_date, end_date) {
        Some(key) => key,
        None => return Ok(false), // No cache available
    };
    
    println!("[CACHE] Using cached historical period: {}", period_key);
    
    let (hits, misses) = apply_cached_period_data(stocks, &period_key);
    println!("[CACHE] Loaded from cached period: {} hits, {} misses", hits, misses);
    
    // Try interpolation for better accuracy
    if let Some((before_period, after_period)) = find_surrounding_periods(start_date) {
        println!("[INTERP] Refining with interpolation between {} and {}", before_period, after_period);
        let refined = apply_interpolation_refinement(stocks, start_date, &before_period, &after_period);
        if refined > 0 {
            println!("[INTERP] Interpolated {} stock prices for better accuracy", refined);
        }
    }
    
    // Success if we got data for most stocks
    Ok(hits > misses)
}

/// Fetch historical returns for stocks during a specific date range
/// First tries cached data with interpolation, falls back to API if unavailable
pub async fn fetch_historical_returns(
    stocks: &mut [Stock], 
    start_date: &str,  // Format: YYYY-MM-DD
    end_date: &str     // Format: YYYY-MM-DD
) -> Result<(), Box<dyn Error>> {
    // Try cache first
    if fetch_from_cache(stocks, start_date, end_date)? {
        return Ok(());
    }
    
    // Fallback to Yahoo Finance API (slow)
    println!("[WARN] Falling back to API for historical data...");
    println!("[WARN] This will be VERY SLOW (~10 seconds per stock)");
    println!("[WARN] RECOMMENDATION: Run 'python3 fetch_stocks.py' to generate cache first!");
    
    fetch_from_yahoo_api(stocks, start_date, end_date).await
}

/// Fetch historical data from Yahoo Finance API (fallback when cache unavailable)
async fn fetch_from_yahoo_api(stocks: &mut [Stock], start_date: &str, end_date: &str) -> Result<(), Box<dyn Error>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    
    let start_timestamp = chrono::NaiveDate::parse_from_str(start_date, "%Y-%m-%d")?
        .and_hms_opt(0, 0, 0).unwrap()
        .and_utc()
        .timestamp();
    
    let end_timestamp = chrono::NaiveDate::parse_from_str(end_date, "%Y-%m-%d")?
        .and_hms_opt(0, 0, 0).unwrap()
        .and_utc()
        .timestamp();
    
    let stocks_to_fetch: Vec<&mut Stock> = stocks.iter_mut()
        .filter(|s| s.historical_return.is_none())
        .collect();
    
    let total = stocks_to_fetch.len();
    println!("[API] Fetching data for {} stocks via API...", total);
    
    let mut success = 0;
    let mut failed = 0;
    
    for (i, stock) in stocks_to_fetch.into_iter().enumerate() {
        if i % 10 == 0 {
            println!("   Progress: {}/{} stocks...", i, total);
        }
        
        let url = format!(
            "https://query1.finance.yahoo.com/v8/finance/chart/{}?period1={}&period2={}&interval=1d",
            stock.ticker, start_timestamp, end_timestamp
        );
        
        if let Ok(resp) = client.get(&url).send().await {
            if let Ok(text) = resp.text().await {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                    if let Some(closes) = extract_close_prices(&json) {
                        if let (Some(start_price), Some(end_price)) = (closes.first(), closes.last()) {
                            if *start_price > 0.0 {
                                let return_pct = ((end_price - start_price) / start_price) * 100.0;
                                stock.historical_return = Some(return_pct);
                                stock.historical_start_price = Some(*start_price);
                                success += 1;
                                continue;
                            }
                        }
                    }
                }
            }
        }
        
        failed += 1;
        
        // Rate limiting
        if success % 10 == 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }
    
    println!("[API] Fetch complete: {} success, {} failed", success, failed);
    Ok(())
}

/// Extract close prices from Yahoo Finance API response
fn extract_close_prices(json: &serde_json::Value) -> Option<Vec<f64>> {
    let result = json["chart"]["result"].as_array()?.first()?;
    let quotes = result["indicators"]["quote"].as_array()?.first()?;
    let closes = quotes["close"].as_array()?;
    
    closes.iter()
        .filter_map(|v| v.as_f64())
        .collect::<Vec<f64>>()
        .into()
}
