#!/usr/bin/env python3
"""Fit a linear regression surrogate on the training features to produce coefficients
that can be ported into Rust for fast runtime scoring.

Reads `estimated_points.csv`, fits LinearRegression(X -> actual), prints intercept and coefficients
and writes them to `linear_surrogate.json`.
"""
import csv
import math
from pathlib import Path
import json

IN = 'estimated_points.csv'
OUT = 'linear_surrogate.json'

if not Path(IN).exists():
    print(f"Missing {IN}")
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

print(f"Loaded {len(rows)} rows")

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
    import numpy as np
    from sklearn.linear_model import LinearRegression
except Exception as e:
    print('Missing numpy/sklearn; install with: pip install numpy scikit-learn')
    raise

X = np.array(X)
Y = np.array(Y)

lr = LinearRegression()
lr.fit(X, Y)
coeffs = lr.coef_.tolist()
intercept = float(lr.intercept_)
print('Intercept:', intercept)
print('Coefficients:', coeffs)

res = {'intercept': intercept, 'coefficients': coeffs, 'feature_names': ['budget_log','eligible','period','avg_vol','avg_logcap','avg_pt','psize','risk_cons','risk_mod','risk_aggr']}
with open(OUT, 'w') as f:
    json.dump(res, f, indent=2)
print('Wrote', OUT)
