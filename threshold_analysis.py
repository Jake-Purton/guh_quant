#!/usr/bin/env python3
"""Analyze predicted_points.csv to recommend a skip threshold.

Outputs a CSV 'threshold_summary.csv' and prints a small table showing
for each threshold: skip_fraction, kept_count, skipped_count, total_actual_kept,
mean_actual_kept, total_actual_skipped, mean_actual_skipped.

It also suggests a threshold that maximizes total_actual_kept / kept_count (avg points per kept request)
and a threshold that keeps the top X% (default keep 70%).
"""
import csv
from pathlib import Path
import numpy as np

PRED = 'predicted_points.csv'
OUT = 'threshold_summary.csv'

if not Path(PRED).exists():
    print(f"Missing {PRED}. Run evaluate_points_predictor.py first.")
    raise SystemExit(1)

rows = []
with open(PRED) as f:
    reader = csv.DictReader(f)
    for r in reader:
        try:
            actual = float(r['actual'])
            pred = float(r['predicted'])
        except Exception:
            continue
        rows.append({'ts': r.get('ts',''), 'name': r.get('name',''), 'actual': actual, 'pred': pred})

preds = np.array([r['pred'] for r in rows])
actuals = np.array([r['actual'] for r in rows])

# thresholds from min-10 to max+10
min_t = float(np.floor(preds.min()))
max_t = float(np.ceil(preds.max()))
thresholds = np.linspace(min_t-10, max_t+10, 50)

summary = []
for t in thresholds:
    keep_mask = preds >= t
    kept_count = int(keep_mask.sum())
    skipped_count = len(preds) - kept_count
    if kept_count > 0:
        total_actual_kept = actuals[keep_mask].sum()
        mean_actual_kept = actuals[keep_mask].mean()
    else:
        total_actual_kept = 0.0
        mean_actual_kept = float('nan')
    if skipped_count > 0:
        total_actual_skipped = actuals[~keep_mask].sum()
        mean_actual_skipped = actuals[~keep_mask].mean()
    else:
        total_actual_skipped = 0.0
        mean_actual_skipped = float('nan')
    skip_fraction = skipped_count/len(preds)
    avg_points_per_kept = mean_actual_kept if kept_count>0 else 0.0
    summary.append({'threshold': t, 'skip_fraction': skip_fraction, 'kept_count': kept_count, 'skipped_count': skipped_count, 'total_actual_kept': total_actual_kept, 'mean_actual_kept': avg_points_per_kept, 'total_actual_skipped': total_actual_skipped, 'mean_actual_skipped': mean_actual_skipped})

# write CSV
with open(OUT, 'w', newline='') as csvfile:
    fieldnames = ['threshold','skip_fraction','kept_count','skipped_count','total_actual_kept','mean_actual_kept','total_actual_skipped','mean_actual_skipped']
    writer = csv.DictWriter(csvfile, fieldnames=fieldnames)
    writer.writeheader()
    for s in summary:
        writer.writerow(s)

# Print a small readable table for selected thresholds (percentiles and suggested)
print('Summary (threshold, skip%, kept, mean_actual_kept, total_actual_kept)')
for s in summary[::5]:
    print(f"{s['threshold']:.2f}, {s['skip_fraction']*100:.1f}%, {s['kept_count']}, {s['mean_actual_kept']:.1f}, {s['total_actual_kept']:.1f}")

# Suggest threshold that maximizes mean_actual_kept (avg points per processed request)
valid = [s for s in summary if not np.isnan(s['mean_actual_kept']) and s['kept_count']>0]
if valid:
    best = max(valid, key=lambda x: x['mean_actual_kept'])
    print('\nSuggested thresholds:')
    print(f" - Max average points per kept request: threshold={best['threshold']:.2f}, mean_actual_kept={best['mean_actual_kept']:.1f}, skip%={best['skip_fraction']*100:.1f}%")

# Also suggest a threshold that keeps ~70% of requests
keep_target = 0.7
closest = min(summary, key=lambda s: abs(1.0 - s['skip_fraction'] - keep_target))
print(f" - Keep ~{int(keep_target*100)}% threshold ~ {closest['threshold']:.2f}, skip%={closest['skip_fraction']*100:.1f}%")

print(f"Wrote threshold summary to {OUT}")
