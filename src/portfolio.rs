use crate::investor::{InvestorProfile, RiskLevel};
use crate::stocks::Stock;

fn get_first_trading_year(ticker: &str) -> Option<u32> {
    // Hardcoded first trading years for stocks that frequently cause issues
    // or don't have data in cache yet
    let trading_years = [
        // Recent spinoffs/name changes that may not be valid for older periods
        ("COR", 2023),  // Cencora (formerly AmerisourceBergen, changed name 2023)
        ("TECH", 2014), // Bio-Techne
        ("K", 2012),    // Kellanova (spun from Kellogg 2023)
        ("DOW", 2019),  // Dow Inc. (spun from DowDuPont)
        ("DD", 2019),   // DuPont (spun from DowDuPont)
        
        // Old established companies  
        ("PG", 1890), ("JNJ", 1944), ("PFE", 1942), ("XOM", 1882),
        ("BAC", 1904), ("WFC", 1852), ("IBM", 1911), ("GE", 1892),
        ("KO", 1919), ("DIS", 1957), ("MCD", 1965), ("CAT", 1929),
        ("MMM", 1946), ("BA", 1934), ("F", 1956), ("T", 1983),
        
        // 1970s-1990s
        ("AAPL", 1980), ("MSFT", 1986), ("INTC", 1971), ("WMT", 1972),
        ("CSCO", 1990), ("AMD", 1979), ("ADBE", 1986), ("NVDA", 1999),
        ("AMZN", 1997), ("BKNG", 1999), ("UPS", 1999), ("PLUG", 1999),
        ("EA", 2008),  // Electronic Arts - being conservative due to 2007 ticker issues
        
        // 2000s
        ("GOOGL", 2004), ("GOOG", 2004), ("VZ", 2000), ("NFLX", 2002),
        ("CRM", 2004), ("MA", 2006), ("V", 2008), ("FSLR", 2006),
        ("TSLA", 2010), ("GM", 2010),
        
        // 2010s  
        ("META", 2012), ("ABBV", 2013), ("ZTS", 2013), ("TWTR", 2013),
        ("KMI", 2011), ("MDLZ", 2012), ("ENPH", 2012),
        ("SHOP", 2015), ("MTCH", 2016), ("ETSY", 2015),  // MTCH actually IPO'd late 2015, be conservative
        ("TWLO", 2016), ("SNAP", 2017), ("ROKU", 2017), ("OKTA", 2017),
        ("MRNA", 2018), ("DOCU", 2018), ("VICI", 2018),
        ("ZM", 2019), ("UBER", 2019), ("LYFT", 2019), ("PINS", 2019),
        ("DDOG", 2019), ("CRWD", 2019), ("BNTX", 2019),
        
        // 2020+
        ("PLTR", 2020), ("SNOW", 2020), ("ABNB", 2020), ("DASH", 2020),
        ("RIVN", 2021), ("LCID", 2021), ("SOFI", 2021), ("COIN", 2021),
        ("RBLX", 2021), ("U", 2021), ("HOOD", 2021),
        
        // Additional banks/healthcare that cause issues
        ("TFC", 2004),  // Truist (BB&T legacy ticker, merger was 2019 but BB&T IPO 2004)
        ("ZBH", 2001),  // Zimmer Biomet
        ("USB", 1929),  // US Bancorp (old bank)
        ("PNC", 1983),  // PNC Financial
    ];
    
    trading_years.iter()
        .find(|(t, _)| *t == ticker)
        .map(|(_, year)| *year)
}

pub fn filter_stocks_by_profile(stocks: &[Stock], profile: &InvestorProfile) -> Vec<Stock> {
    stocks
        .iter()
        .filter(|s| {
            // Filter out tickers with hyphens - they often cause API issues
            !s.ticker.contains('-')
        })
        .filter(|s| !profile.should_exclude_sector(&s.sector))
        .filter(|s| {
            // Filter by risk tolerance
            match profile.risk_tolerance {
                RiskLevel::Conservative => s.volatility < 0.03, // Low volatility only
                RiskLevel::Moderate => s.volatility < 0.05,     // Medium volatility
                RiskLevel::Aggressive => true,                   // All stocks ok
            }
        })
        .filter(|s| {
            // Filter by trading date - stock must have been trading during entire investment period
            if let Some(start_year) = profile.start_year {
                // Try cache first
                if let Some(first_date) = &s.first_trading_date {
                    // Extract year from first_trading_date (format: YYYY-MM-DD)
                    if let Some(first_year_str) = first_date.split('-').next() {
                        if let Ok(first_year) = first_year_str.parse::<u32>() {
                            // Stock must have started trading before or at start of investment period
                            return first_year <= start_year;
                        }
                    }
                }
                
                // Fallback to hardcoded database
                if let Some(first_year) = get_first_trading_year(&s.ticker) {
                    return first_year <= start_year;
                }
                
                // If we have start year but no trading date info at all, be conservative and exclude
                false
            } else {
                true // If no investment date info, include it
            }
        })
        .cloned()
        .collect()
}

pub fn build_portfolio(stocks: &[Stock], budget: f64, risk_level: RiskLevel) -> Vec<(String, i32)> {
    if stocks.is_empty() {
        return Vec::new();
    }
    
    // Sort by historical return if available, otherwise by inverse volatility
    let mut sorted_stocks = stocks.to_vec();
    sorted_stocks.sort_by(|a, b| {
        // If both have historical returns, sort by return (highest first)
        match (a.historical_return, b.historical_return) {
            (Some(ret_a), Some(ret_b)) => ret_b.partial_cmp(&ret_a).unwrap(), // Descending
            (Some(_), None) => std::cmp::Ordering::Less,  // Stocks with returns first
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.volatility.partial_cmp(&b.volatility).unwrap(), // Fallback to volatility
        }
    });
    
    // Target number of positions based on risk tolerance
    let target_positions = match risk_level {
        RiskLevel::Conservative => 15, // More diversification
        RiskLevel::Moderate => 10,
        RiskLevel::Aggressive => 7,    // More concentrated
    };
    
    // For small budgets, use greedy allocation instead of equal weight
    if budget < 5000.0 {
        return build_greedy_portfolio(&sorted_stocks, budget);
    }
    
    // Equal weight allocation for larger budgets
    let num_positions = target_positions.min(sorted_stocks.len());
    let allocation_per_stock = budget / num_positions as f64;
    let mut portfolio = Vec::new();
    
    for stock in sorted_stocks.iter().take(num_positions) {
        let quantity = (allocation_per_stock / stock.price).floor() as i32;
        if quantity > 0 {
            portfolio.push((stock.ticker.clone(), quantity));
        }
    }
    
    portfolio
}

fn build_greedy_portfolio(stocks: &[Stock], budget: f64) -> Vec<(String, i32)> {
    let mut portfolio = Vec::new();
    let mut remaining_budget = budget;
    
    // Filter to only affordable stocks (price <= budget)
    let mut affordable_stocks: Vec<&Stock> = stocks
        .iter()
        .filter(|s| s.price <= budget)  // Use original budget, not remaining
        .collect();
    
    if affordable_stocks.is_empty() {
        return portfolio;
    }
    
    // Sort affordable stocks by price (cheapest first for small budgets)
    affordable_stocks.sort_by(|a, b| a.price.partial_cmp(&b.price).unwrap());
    
    // Greedy approach: buy as many shares as possible, diversifying when we can
    let mut stock_index = 0;
    let mut shares_per_stock = vec![0; affordable_stocks.len()];
    
    // First pass: buy at least 1 share of as many stocks as we can afford
    for (i, stock) in affordable_stocks.iter().enumerate() {
        if remaining_budget >= stock.price {
            shares_per_stock[i] = 1;
            remaining_budget -= stock.price;
        }
    }
    
    // Second pass: keep buying more shares round-robin style
    while remaining_budget > 0.0 {
        let stock = affordable_stocks[stock_index];
        if remaining_budget >= stock.price {
            shares_per_stock[stock_index] += 1;
            remaining_budget -= stock.price;
        }
        
        // Move to next affordable stock
        stock_index = (stock_index + 1) % affordable_stocks.len();
        
        // Check if we can't afford anything anymore
        if affordable_stocks.iter().all(|s| s.price > remaining_budget) {
            break;
        }
    }
    
    // Build final portfolio
    for (i, stock) in affordable_stocks.iter().enumerate() {
        if shares_per_stock[i] > 0 {
            portfolio.push((stock.ticker.clone(), shares_per_stock[i]));
        }
    }
    
    portfolio
}
