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
    pub ipo_year: Option<u32>,
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

fn get_ipo_year(ticker: &str) -> Option<u32> {
    // Known IPO years for common stocks (approximate)
    let ipo_years = [
        // Pre-2000
        ("AAPL", 1980), ("MSFT", 1986), ("JPM", 1980), ("JNJ", 1944), ("WMT", 1972),
        ("PG", 1890), ("KO", 1919), ("PFE", 1942), ("XOM", 1882), ("CVX", 1926),
        ("BAC", 1904), ("WFC", 1852), ("GE", 1892), ("IBM", 1911), ("INTC", 1971),
        ("CSCO", 1990), ("T", 1983), ("VZ", 2000), ("MCD", 1965), ("DIS", 1957),
        ("BA", 1934), ("CAT", 1929), ("MMM", 1946), ("HON", 1970), ("UPS", 1999),
        
        // 2000-2010
        ("GOOGL", 2004), ("GOOG", 2004), ("V", 2008), ("MA", 2006), ("AMZN", 1997),
        ("NFLX", 2002), ("TSLA", 2010), ("BKNG", 1999), ("CRM", 2004), ("ADBE", 1986),
        
        // 2010-2015
        ("META", 2012), ("TWTR", 2013), ("ABBV", 2013), ("ZTS", 2013),
        
        // 2015-2020  
        ("SNAP", 2017), ("ROKU", 2017), ("SHOP", 2015), ("ZM", 2019),
        ("UBER", 2019), ("PINS", 2019), ("DDOG", 2019), ("CRWD", 2019),
        ("WORK", 2019), ("DOCU", 2018), ("OKTA", 2017), ("TWLO", 2016),
        ("PLTR", 2020), ("SNOW", 2020), ("ABNB", 2020), ("DASH", 2020),
        ("COIN", 2021), ("RBLX", 2021), ("U", 2021),
        
        // 2020+
        ("RIVN", 2021), ("LCID", 2021), ("SOFI", 2021), ("HOOD", 2021),
        
        // Biotechs
        ("MRNA", 2018), ("BNTX", 2019), ("VRTX", 1991), ("REGN", 1991),
        ("BIIB", 1991), ("GILD", 1992),
        
        // REITs
        ("VICI", 2018), ("O", 1994), ("AMT", 1998), ("PLD", 1997), ("EQIX", 2000),
        
        // Regional banks
        ("TFC", 2001), ("USB", 1929), ("PNC", 1983),
        
        // Others
        ("F", 1956), ("GM", 2010), ("LYFT", 2019), ("PLUG", 1999),
        ("ENPH", 2012), ("FSLR", 2006), ("NVDA", 1999), ("AMD", 1979),
    ];
    
    ipo_years.iter()
        .find(|(t, _)| *t == ticker)
        .map(|(_, year)| *year)
}

pub fn load_stocks_from_cache(cache_file: &str) -> Result<Vec<Stock>, Box<dyn Error>> {
    println!("📂 Loading stocks from cache: {}", cache_file);
    
    let contents = fs::read_to_string(cache_file)
        .map_err(|e| format!("Failed to read cache file '{}': {}. Run fetch_stocks.py first!", cache_file, e))?;
    
    let mut cache: StockCache = serde_json::from_str(&contents)?;
    
    // Add IPO years if missing
    for stock in &mut cache.stocks {
        if stock.ipo_year.is_none() {
            stock.ipo_year = get_ipo_year(&stock.ticker);
        }
    }
    
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
