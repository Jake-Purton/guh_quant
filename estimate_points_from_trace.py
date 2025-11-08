#!/usr/bin/env python3
"""Estimate client points from request_trace.jsonl and compare to actual evaluator points.

Produces `estimated_points.csv` and prints summary stats (Pearson correlation if numpy available).
"""
import json
import math
import csv
from statistics import mean, pstdev
from pathlib import Path

IN = 'request_trace.jsonl'
OUT = 'estimated_points.csv'

# Try to load stocks cache and points store to enrich features
STOCKS_CACHE = 'stocks_cache.json'
POINTS_STORE = 'points_store.json'

stocks_meta = {}
if Path(STOCKS_CACHE).exists():
    try:
        with open(STOCKS_CACHE) as sf:
            sc = json.load(sf)
            for s in sc.get('stocks', []):
                stocks_meta[s.get('ticker')] = s
    except Exception:
        stocks_meta = {}

points_store = {}
if Path(POINTS_STORE).exists():
    try:
        with open(POINTS_STORE) as pf:
            ps = json.load(pf)
            points_store = ps.get('scores', {}) if isinstance(ps, dict) else {}
    except Exception:
        points_store = {}

risk_map = {
    'Conservative': 0.9,
    'Moderate': 1.0,
    'Aggressive': 1.1,
}

rows = []
actuals = []
expecteds = []

p = Path(IN)
if not p.exists():
    print(f"Missing {IN}")
    raise SystemExit(1)

with p.open() as f:
    for line in f:
        line = line.strip()
        if not line or line.startswith('//'):
            continue
        try:
            entry = json.loads(line)
        except Exception:
            # sometimes a comment line precedes a JSON in the same file; skip
            continue

        parsed = entry.get('parsed_profile', {})
        budget = parsed.get('budget', 0.0) or 0.0
        eligible_count = entry.get('eligible_count', 0) or 0
        start_year = parsed.get('start_year')
        end_year = parsed.get('end_year')
        # compute period years
        try:
            if start_year is not None and end_year is not None and end_year >= start_year:
                period_years = float(end_year - start_year + 1)
            else:
                period_years = 1.0
        except Exception:
            period_years = 1.0

        # budget score: ln1p(budget) / 10
        budget_score = math.log1p(max(budget, 0.0)) / 10.0 if budget is not None else 0.0
        # return_score not available in trace; default to 0.0
        return_score = 0.0
        eligible_score = min(float(eligible_count), 200.0) / 20.0
        period_score = min(period_years / 5.0, 3.0)

        # Enriched portfolio features (from stocks_cache and points_store)
        port = entry.get('portfolio', [])
        total_qty = 0
        vol_sum = 0.0
        logcap_sum = 0.0
        pts_sum = 0.0
        distinct = 0
        for pos in port:
            t = pos.get('ticker')
            q = int(pos.get('quantity', 1) or 1)
            total_qty += q
            meta = stocks_meta.get(t)
            if meta:
                vol = meta.get('volatility') or 0.0
                vol_sum += vol * q
                mc = meta.get('market_cap') or 0.0
                if mc and mc > 0:
                    logcap_sum += math.log10(mc) * q
            # points store: average across buckets if present
            pscores = points_store.get(t, {})
            if pscores:
                # average across known buckets
                bucket_vals = [v for v in pscores.values() if isinstance(v, (int,float))]
                if bucket_vals:
                    avgp = sum(bucket_vals) / len(bucket_vals)
                else:
                    avgp = 0.0
            else:
                avgp = 0.0
            pts_sum += avgp * q
            distinct += 1

        if total_qty > 0:
            avg_vol = vol_sum / total_qty
            avg_logcap = (logcap_sum / total_qty) if logcap_sum > 0 else 0.0
            avg_pts_score = pts_sum / total_qty
        else:
            avg_vol = 0.0
            avg_logcap = 0.0
            avg_pts_score = 0.0

        base = (budget_score + return_score + eligible_score + period_score) * 10.0
        risk_str = parsed.get('risk_tolerance', 'Moderate')
        risk_multiplier = risk_map.get(risk_str, 1.0)
        expected = max(0.0, base * risk_multiplier)

        # attempt to parse actual points from nested response
        actual_points = None
        result = entry.get('result')
        if isinstance(result, dict):
            if result.get('ok'):
                resp = result.get('response')
                if isinstance(resp, str):
                    try:
                        inner = json.loads(resp)
                        if isinstance(inner, dict) and 'points' in inner:
                            actual_points = float(inner.get('points'))
                    except Exception:
                        # maybe it's already JSON object; try parsing as JSON value
                        pass
                elif isinstance(resp, dict) and 'points' in resp:
                    actual_points = float(resp.get('points'))
            else:
                # error
                actual_points = None

        rows.append({
            'ts': entry.get('ts',''),
            'name': parsed.get('name',''),
            'budget': budget,
            'eligible_count': eligible_count,
            'start_year': start_year,
            'end_year': end_year,
            'risk_tolerance': risk_str,
            'expected_points': expected,
            'actual_points': actual_points,
            'skipped': entry.get('skipped', False),
            'avg_volatility': avg_vol,
            'avg_log_marketcap': avg_logcap,
            'avg_points_score': avg_pts_score,
            'portfolio_size': distinct,
        })
        expecteds.append(expected)
        if actual_points is not None:
            actuals.append(actual_points)

# write CSV
with open(OUT, 'w', newline='') as csvfile:
    writer = csv.DictWriter(csvfile, fieldnames=['ts','name','budget','eligible_count','start_year','end_year','risk_tolerance','expected_points','actual_points','skipped','avg_volatility','avg_log_marketcap','avg_points_score','portfolio_size'])
    writer.writeheader()
    for r in rows:
        writer.writerow(r)

# summary
print(f"Wrote {len(rows)} rows to {OUT}")
print(f"Records with actual points: {len(actuals)}")
print(f"Expected points mean={mean(expecteds):.2f}, std={(pstdev(expecteds) if len(expecteds)>1 else 0.0):.2f}")
if actuals:
    print(f"Actual points mean={mean(actuals):.2f}, std={(pstdev(actuals) if len(actuals)>1 else 0.0):.2f}")

# Pearson correlation if numpy available and lengths match for aligned rows
try:
    import numpy as np
    # we can compute correlation using only rows that have actual_points
    ex = []
    ac = []
    for r in rows:
        if r['actual_points'] is not None:
            ex.append(r['expected_points'])
            ac.append(r['actual_points'])
    if len(ex) > 1:
        corr = np.corrcoef(np.array(ex), np.array(ac))[0,1]
        print(f"Pearson correlation (expected vs actual): {corr:.4f}")
    else:
        print("Not enough paired records for correlation.")
except Exception:
    print("numpy not available — skipping Pearson correlation. To enable, install numpy.")

print("Done.")
