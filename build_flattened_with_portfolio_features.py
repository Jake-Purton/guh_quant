#!/usr/bin/env python3
"""
Build a flattened CSV from JSONL traces and augment each record with portfolio-level features.
Outputs `study_augmented.csv` by default.

Features added (when available):
 - portfolio_size
 - portfolio_tickers_count
 - portfolio_avg_hist_return
 - portfolio_std_hist_return
 - portfolio_max_hist_return
 - portfolio_total_cost
 - portfolio_sector_<sector>_count  (one column per seen sector, sparse)

This script uses `study.py`'s `iter_json_records` and `flatten` helpers to ensure consistency.
"""
from __future__ import annotations
import json
from pathlib import Path
from typing import Any, Dict, List
import argparse
import math

import pandas as pd
import numpy as np


def safe_get_numeric(d: Dict[str, Any], keys: List[str]):
    for k in keys:
        if k in d:
            try:
                return float(d[k])
            except Exception:
                pass
    return None


def extract_portfolio_features(rec: Dict[str, Any]) -> Dict[str, Any]:
    out: Dict[str, Any] = {}
    portfolio = rec.get('portfolio') or rec.get('portfolio_positions') or rec.get('portfolio_items')
    if not isinstance(portfolio, list):
        # maybe flattened or missing
        return out

    out['portfolio_size'] = len(portfolio)
    tickers = set()
    hist_returns = []
    total_cost = 0.0
    sector_counts: Dict[str, int] = {}

    for item in portfolio:
        if not isinstance(item, dict):
            continue
        # ticker field heuristics
        ticker = item.get('ticker') or item.get('symbol') or item.get('id') or item.get('name')
        if isinstance(ticker, str):
            tickers.add(ticker)
        # historical return heuristics
        hr = safe_get_numeric(item, ['historical_return', 'return_pct', 'return', 'hist_return', 'historical_pct'])
        if hr is not None and math.isfinite(hr):
            hist_returns.append(hr)
        # cost heuristics
        qty = safe_get_numeric(item, ['quantity', 'qty', 'shares']) or 1.0
        price = safe_get_numeric(item, ['price', 'purchase_price', 'current_price', 'last_price'])
        if price is not None:
            total_cost += price * qty
        else:
            # fallback: if item has 'cost' directly
            cost = safe_get_numeric(item, ['cost', 'position_cost', 'value'])
            if cost is not None:
                total_cost += cost
        # sector
        sector = item.get('sector') or item.get('industry')
        if isinstance(sector, str) and sector:
            sector_counts[sector] = sector_counts.get(sector, 0) + 1

    out['portfolio_tickers_count'] = len(tickers)
    if hist_returns:
        arr = np.array(hist_returns, dtype=float)
        out['portfolio_avg_hist_return'] = float(np.nanmean(arr))
        out['portfolio_std_hist_return'] = float(np.nanstd(arr))
        out['portfolio_max_hist_return'] = float(np.nanmax(arr))
    else:
        out['portfolio_avg_hist_return'] = None
        out['portfolio_std_hist_return'] = None
        out['portfolio_max_hist_return'] = None

    out['portfolio_total_cost'] = float(total_cost) if total_cost != 0.0 else None

    # sector counts
    for s, cnt in sector_counts.items():
        key = f'portfolio_sector_{s}_count'
        out[key] = cnt

    return out


def main(argv=None):
    p = argparse.ArgumentParser()
    p.add_argument('files', nargs='*', help='JSON/JSONL files to read (defaults to request_trace.jsonl)')
    p.add_argument('--out', default='study_augmented.csv')
    args = p.parse_args(argv)

    files = args.files or ['request_trace.jsonl']
    paths = [Path(f) for f in files if Path(f).exists()]
    if not paths:
        print('No input files found:', files)
        return 2

    # import study helpers
    import study as study_helpers  # type: ignore

    def is_overbudget_text(text: str) -> bool:
        if not text:
            return False
        tl = str(text).lower()
        if 'budget' in tl and any(k in tl for k in ('breach', 'breached', 'exceed', 'exceeded')):
            return True
        if 'budget breached' in tl or 'budget breach' in tl:
            return True
        if 'would exceed budget' in tl or 'exceeds budget' in tl or 'exceed budget' in tl:
            return True
        return False

    records = []
    for rec in study_helpers.iter_json_records(paths):
        flat = study_helpers.flatten(rec)
        # canonicalize points if exists
        k, v = study_helpers.find_points_key(flat)
        if k is None:
            # apply same error heuristic but penalize budget breaches more heavily
            err_found = False
            if isinstance(rec, dict):
                res = rec.get('result')
                if isinstance(res, dict):
                    if res.get('ok') is False or res.get('error'):
                        err_text = None
                        if isinstance(res.get('error'), str) and res.get('error'):
                            err_text = res.get('error')
                        elif isinstance(res.get('response'), str) and res.get('response'):
                            err_text = res.get('response')
                        if is_overbudget_text(err_text):
                            flat['__points__'] = -2000
                        else:
                            flat['__points__'] = -400
                        err_found = True
            if not err_found:
                for fk, fv in flat.items():
                    if fk.split('.')[-1].lower() == 'error' and fv not in (None, '', []):
                        if is_overbudget_text(fv):
                            flat['__points__'] = -2000
                        else:
                            flat['__points__'] = -400
                        err_found = True
                        break
            if not err_found:
                continue
        else:
            flat['__points__'] = v

        # compute portfolio features and merge
        pf = extract_portfolio_features(rec)
        flat.update(pf)
        records.append(flat)

    if not records:
        print('No records extracted')
        return 3

    df = pd.DataFrame(records)
    # ensure columns are stable; fill missing sector columns with NaN
    out_path = Path(args.out)
    df.to_csv(out_path, index=False)
    print('Wrote augmented CSV to', out_path)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
#!/usr/bin/env python3
"""
Build a flattened CSV from JSONL traces and augment each record with portfolio-level features.
Outputs `study_augmented.csv` by default.

Features added (when available):
 - portfolio_size
 - portfolio_tickers_count
 - portfolio_avg_hist_return
 - portfolio_std_hist_return
 - portfolio_max_hist_return
 - portfolio_total_cost
 - portfolio_sector_<sector>_count  (one column per seen sector, sparse)

This script uses `study.py`'s `iter_json_records` and `flatten` helpers to ensure consistency.
"""
from __future__ import annotations
import json
from pathlib import Path
from typing import Any, Dict, List
import argparse
import math

import pandas as pd
import numpy as np


def safe_get_numeric(d: Dict[str, Any], keys: List[str]):
    for k in keys:
        if k is None:
            # apply same error heuristic but penalize budget breaches more heavily
            err_found = False
            def is_overbudget_text(text: str) -> bool:
                if not text:
                    return False
                tl = str(text).lower()
                if 'budget' in tl and any(k in tl for k in ('breach', 'breached', 'exceed', 'exceeded')):
                    return True
                if 'budget breached' in tl or 'budget breach' in tl:
                    return True
                if 'would exceed budget' in tl or 'exceeds budget' in tl or 'exceed budget' in tl:
                    return True
                return False

            if isinstance(rec, dict):
                res = rec.get('result')
                if isinstance(res, dict):
                    if res.get('ok') is False or res.get('error'):
                        err_text = None
                        if isinstance(res.get('error'), str) and res.get('error'):
                            err_text = res.get('error')
                        elif isinstance(res.get('response'), str) and res.get('response'):
                            err_text = res.get('response')
                        if is_overbudget_text(err_text):
                            flat['__points__'] = -2000
                        else:
                            flat['__points__'] = -400
                        err_found = True
            if not err_found:
                for fk, fv in flat.items():
                    if fk.split('.')[-1].lower() == 'error' and fv not in (None, '', []):
                        if is_overbudget_text(fv):
                            flat['__points__'] = -2000
                        else:
                            flat['__points__'] = -400
                        err_found = True
            if not err_found:
                continue
    tickers = set()
    hist_returns = []
    total_cost = 0.0
    sector_counts: Dict[str, int] = {}

    for item in portfolio:
        if not isinstance(item, dict):
            continue
        # ticker field heuristics
        ticker = item.get('ticker') or item.get('symbol') or item.get('id') or item.get('name')
        if isinstance(ticker, str):
            tickers.add(ticker)
        # historical return heuristics
        hr = safe_get_numeric(item, ['historical_return', 'return_pct', 'return', 'hist_return', 'historical_pct'])
        if hr is not None and math.isfinite(hr):
            hist_returns.append(hr)
        # cost heuristics
        qty = safe_get_numeric(item, ['quantity', 'qty', 'shares']) or 1.0
        price = safe_get_numeric(item, ['price', 'purchase_price', 'current_price', 'last_price'])
        if price is not None:
            total_cost += price * qty
        else:
            # fallback: if item has 'cost' directly
            cost = safe_get_numeric(item, ['cost', 'position_cost', 'value'])
            if cost is not None:
                total_cost += cost
        # sector
        sector = item.get('sector') or item.get('industry')
        if isinstance(sector, str) and sector:
            sector_counts[sector] = sector_counts.get(sector, 0) + 1

            for k in keys:
                if k in d:
                    try:
                        return float(d[k])
                    except Exception:
                        pass
            return None
        out['portfolio_avg_hist_return'] = None
        out['portfolio_std_hist_return'] = None
        out['portfolio_max_hist_return'] = None

    out['portfolio_total_cost'] = float(total_cost) if total_cost != 0.0 else None

    # sector counts
    for s, cnt in sector_counts.items():
        key = f'portfolio_sector_{s}_count'
        out[key] = cnt

    return out


def main(argv=None):
    p = argparse.ArgumentParser()
    p.add_argument('files', nargs='*', help='JSON/JSONL files to read (defaults to request_trace.jsonl)')
    p.add_argument('--out', default='study_augmented.csv')
    args = p.parse_args(argv)

    files = args.files or ['request_trace.jsonl']
    paths = [Path(f) for f in files if Path(f).exists()]
    if not paths:
        print('No input files found:', files)
        return 2

    # import study helpers
    import study as study_helpers  # type: ignore

    records = []
    for rec in study_helpers.iter_json_records(paths):
        flat = study_helpers.flatten(rec)
        # canonicalize points if exists
        k, v = study_helpers.find_points_key(flat)
        if k is None:
            # apply same error heuristic
            err_found = False
            if isinstance(rec, dict):
                res = rec.get('result')
                if isinstance(res, dict):
                    if res.get('ok') is False or res.get('error'):
                        flat['__points__'] = -400
                        err_found = True
            if not err_found:
                for fk, fv in flat.items():
                    if fk.split('.')[-1].lower() == 'error' and fv not in (None, '', []):
                        flat['__points__'] = -400
                        err_found = True
                        break
            if not err_found:
                continue
        else:
            flat['__points__'] = v

        # compute portfolio features and merge
        pf = extract_portfolio_features(rec)
        flat.update(pf)
        records.append(flat)

    if not records:
        print('No records extracted')
        return 3

    df = pd.DataFrame(records)
    # ensure columns are stable; fill missing sector columns with NaN
    out_path = Path(args.out)
    df.to_csv(out_path, index=False)
    print('Wrote augmented CSV to', out_path)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
