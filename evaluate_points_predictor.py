#!/usr/bin/env python3
"""Evaluate the saved RandomForest predictor against estimated_points.csv.

Outputs:
 - predicted_points.csv (ts,name,actual,predicted)
 - prints Pearson correlation, R^2, MAE
"""
import csv
from pathlib import Path
import math

IN = 'estimated_points.csv'
MODEL = 'points_predictor.joblib'
OUT = 'predicted_points.csv'

if not Path(IN).exists():
    print(f"Missing {IN}. Run estimate_points_from_trace.py first.")
    raise SystemExit(1)
if not Path(MODEL).exists():
    print(f"Missing {MODEL}. Run train_points_predictor.py first.")
    raise SystemExit(1)

rows = []
with open(IN) as f:
    reader = csv.DictReader(f)
    for r in reader:
        ap = r.get('actual_points')
        if ap is None or ap == '' or ap.lower() == 'none':
            continue
        try:
            actual = float(ap)
        except Exception:
            continue
        budget = float(r.get('budget') or 0.0)
        eligible = float(r.get('eligible_count') or 0.0)
        start = r.get('start_year')
        end = r.get('end_year')
        try:
            start = int(start) if start not in ('', 'None', 'null') and start is not None else None
            end = int(end) if end not in ('', 'None', 'null') and end is not None else None
        except Exception:
            start = None
            end = None
        if start is not None and end is not None and end >= start:
            period = float(end - start + 1)
        else:
            period = 1.0
        risk = r.get('risk_tolerance') or 'Moderate'
        avg_vol = float(r.get('avg_volatility') or 0.0)
        avg_logcap = float(r.get('avg_log_marketcap') or 0.0)
        avg_pt = float(r.get('avg_points_score') or 0.0)
        psize = float(r.get('portfolio_size') or 0.0)

        rows.append({
            'ts': r.get('ts',''),
            'name': r.get('name',''),
            'actual': actual,
            'budget_log': math.log1p(max(budget, 0.0)),
            'eligible': eligible,
            'period': period,
            'risk': risk,
            'avg_vol': avg_vol,
            'avg_logcap': avg_logcap,
            'avg_pt': avg_pt,
            'psize': psize,
        })

# Build feature matrix as in training script
X = []
Y = []
for r in rows:
    risk_encode = [1.0 if r['risk']=='Conservative' else 0.0,
                   1.0 if r['risk']=='Moderate' else 0.0,
                   1.0 if r['risk']=='Aggressive' else 0.0]
    feat = [r['budget_log'], r['eligible'], r['period'], r['avg_vol'], r['avg_logcap'], r['avg_pt'], r['psize']] + risk_encode
    X.append(feat)
    Y.append(r['actual'])

try:
    import joblib
    import numpy as np
    from sklearn.metrics import r2_score, mean_absolute_error
except Exception as e:
    print('Requirements missing: joblib, numpy, sklearn. Install with: pip install joblib numpy scikit-learn')
    raise

model = joblib.load(MODEL)
X = np.array(X)
Y = np.array(Y)

pred = model.predict(X)

# Write predictions
with open(OUT, 'w', newline='') as csvfile:
    writer = csv.writer(csvfile)
    writer.writerow(['ts','name','actual','predicted'])
    for r, p_val in zip(rows, pred):
        writer.writerow([r['ts'], r['name'], r['actual'], float(p_val)])

# Metrics
from scipy.stats import pearsonr
try:
    pearson_r, _ = pearsonr(Y, pred)
except Exception:
    pearson_r = float('nan')

r2 = r2_score(Y, pred)
mae = mean_absolute_error(Y, pred)

print(f"Wrote {len(pred)} predictions to {OUT}")
print(f"Pearson r: {pearson_r:.4f}")
print(f"R^2: {r2:.4f}")
print(f"MAE: {mae:.4f}")
