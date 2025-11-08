use crate::investor::{InvestorProfile, RiskLevel};
use crate::stocks::Stock;

/// Get the first trading year for a ticker from hardcoded database
/// This is used as a fallback when cache data is unavailable
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
        ("AMZN", 1997), ("UPS", 1999), ("PLUG", 1999),
        ("BKNG", 2018), // Changed from PCLN in 2018, use conservative date
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

/// Tickers that are excluded due to API issues or data quality problems
const EXCLUDED_TICKERS: &[&str] = &["MTCH", "TFC", "ELV", "EA", "ES", "MDLZ", "NEE", "ZBH"];

/// Check if ticker should be excluded
fn is_ticker_excluded(ticker: &str) -> bool {
    // Filter out tickers with hyphens (API issues)
    if ticker.contains('-') {
        return true;
    }
    
    // Filter out manually excluded tickers
    EXCLUDED_TICKERS.contains(&ticker)
}

/// Check if stock volatility matches risk tolerance
fn matches_risk_tolerance(volatility: f64, risk_level: RiskLevel) -> bool {
    match risk_level {
        RiskLevel::Conservative => volatility < 0.03, // Low volatility only
        RiskLevel::Moderate => volatility < 0.05,     // Medium volatility
        RiskLevel::Aggressive => true,                // All stocks acceptable
    }
}

/// Check if stock was trading during the investment period
fn was_trading_during_period(stock: &Stock, start_year: Option<u32>) -> bool {
    let Some(required_start_year) = start_year else {
        return true; // No date restriction
    };
    
    // Try cache first (format: YYYY-MM-DD)
    if let Some(first_date) = &stock.first_trading_date {
        if let Some(year_str) = first_date.split('-').next() {
            if let Ok(first_year) = year_str.parse::<u32>() {
                return first_year <= required_start_year;
            }
        }
    }
    
    // Fallback to hardcoded database
    if let Some(first_year) = get_first_trading_year(&stock.ticker) {
        return first_year <= required_start_year;
    }
    
    // Conservative: exclude if we have no trading date info
    false
}

/// Filter stocks based on investor profile requirements
pub fn filter_stocks_by_profile(stocks: &[Stock], profile: &InvestorProfile) -> Vec<Stock> {
    stocks
        .iter()
        .filter(|s| !is_ticker_excluded(&s.ticker))
        .filter(|s| !profile.should_exclude_sector(&s.sector))
        .filter(|s| matches_risk_tolerance(s.volatility, profile.risk_tolerance))
        .filter(|s| was_trading_during_period(s, profile.start_year))
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
            (Some(ret_a), Some(ret_b)) => ret_a.partial_cmp(&ret_b).unwrap().reverse(), // Descending (highest first)
            (Some(_), None) => std::cmp::Ordering::Less,  // Stocks with returns first
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.volatility.partial_cmp(&b.volatility).unwrap(), // Fallback to volatility (lowest first)
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
    
    // Performance-weighted allocation for larger budgets
    build_weighted_portfolio(&sorted_stocks, budget, target_positions)
}

/// Calculate performance-based weights for stocks
fn calculate_performance_weights(stocks: &[&Stock]) -> Vec<f64> {
    let weights: Vec<f64> = stocks
        .iter()
        .map(|stock| {
            let return_pct = stock.historical_return.unwrap_or(0.0);
            if return_pct > 0.0 { return_pct } else { 1.0 } // Min weight for negative returns
        })
        .collect();
    
    let total: f64 = weights.iter().sum();
    
    // Normalize to sum to 1.0
    if total > 0.0 {
        weights.iter().map(|w| w / total).collect()
    } else {
        vec![1.0 / stocks.len() as f64; stocks.len()] // Equal weights fallback
    }
}

/// Build portfolio with performance-weighted allocation
fn build_weighted_portfolio(stocks: &[Stock], budget: f64, target_positions: usize) -> Vec<(String, i32)> {
    let num_positions = target_positions.min(stocks.len());
    let top_stocks: Vec<&Stock> = stocks.iter().take(num_positions).collect();
    
    if top_stocks.is_empty() {
        return Vec::new();
    }
    
    let weights = calculate_performance_weights(&top_stocks);
    let mut portfolio = Vec::new();
    let mut allocated = 0.0;
    
    // Allocate budget proportionally to each stock
    for (i, stock) in top_stocks.iter().enumerate() {
        let purchase_price = stock.get_purchase_price();
        let target_allocation = budget * weights[i];
        let quantity = (target_allocation / purchase_price).floor() as i32;
        
        if quantity > 0 {
            portfolio.push((stock.ticker.clone(), quantity));
            allocated += (quantity as f64) * purchase_price;
        }
    }
    
    // Deploy remaining budget into top performer
    deploy_remaining_budget(&mut portfolio, budget - allocated, top_stocks[0]);
    
    portfolio
}

/// Deploy remaining budget into the best performing stock
fn deploy_remaining_budget(portfolio: &mut Vec<(String, i32)>, remaining: f64, top_stock: &Stock) {
    if remaining <= 0.0 {
        return;
    }
    
    let price = top_stock.get_purchase_price();
    let extra_qty = (remaining / price).floor() as i32;
    
    if extra_qty > 0 {
        // Add to existing position or create new one
        if let Some(pos) = portfolio.iter_mut().find(|(t, _)| t == &top_stock.ticker) {
            pos.1 += extra_qty;
        } else {
            portfolio.push((top_stock.ticker.clone(), extra_qty));
        }
    }
}

fn build_greedy_portfolio(stocks: &[Stock], budget: f64) -> Vec<(String, i32)> {
    let mut portfolio = Vec::new();
    let mut remaining_budget = budget;
    
    // Filter to only affordable stocks (use historical price if available)
    let mut affordable_stocks: Vec<&Stock> = stocks
        .iter()
        .filter(|s| s.get_purchase_price() <= budget)  // Use original budget, not remaining
        .collect();
    
    if affordable_stocks.is_empty() {
        return portfolio;
    }
    
    // Sort affordable stocks by price (cheapest first for small budgets)
    affordable_stocks.sort_by(|a, b| {
        a.get_purchase_price().partial_cmp(&b.get_purchase_price()).unwrap()
    });
    
    // Greedy approach: buy as many shares as possible, diversifying when we can
    let mut stock_index = 0;
    let mut shares_per_stock = vec![0; affordable_stocks.len()];
    
    // First pass: buy at least 1 share of as many stocks as we can afford
    for (i, stock) in affordable_stocks.iter().enumerate() {
        let price = stock.get_purchase_price();
        if remaining_budget >= price {
            shares_per_stock[i] = 1;
            remaining_budget -= price;
        }
    }
    
    // Second pass: keep buying more shares round-robin style
    while remaining_budget > 0.0 {
        let stock = affordable_stocks[stock_index];
        let price = stock.get_purchase_price();
        if remaining_budget >= price {
            shares_per_stock[stock_index] += 1;
            remaining_budget -= price;
        }
        
        // Move to next affordable stock
        stock_index = (stock_index + 1) % affordable_stocks.len();
        
        // Check if we can't afford anything anymore
        if affordable_stocks.iter().all(|s| s.get_purchase_price() > remaining_budget) {
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
