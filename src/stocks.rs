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
}

#[derive(Debug, Deserialize)]
struct StockCache {
    metadata: Metadata,
    stocks: Vec<Stock>,
}

#[derive(Debug, Deserialize)]
struct Metadata {
    generated_at: String,
    stock_count: usize,
    sector_keywords: HashMap<String, Vec<String>>,
    sectors: Vec<String>,
}

pub fn load_stocks_from_cache(cache_file: &str) -> Result<Vec<Stock>, Box<dyn Error>> {
    println!("📂 Loading stocks from cache: {}", cache_file);
    
    let contents = fs::read_to_string(cache_file)
        .map_err(|e| format!("Failed to read cache file '{}': {}. Run fetch_stocks.py first!", cache_file, e))?;
    
    let cache: StockCache = serde_json::from_str(&contents)?;
    
    println!("✅ Loaded {} stocks from cache (generated: {})", 
             cache.stocks.len(), 
             cache.metadata.generated_at);
    
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
