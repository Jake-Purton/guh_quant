#!/usr/bin/env python3
"""Train a regressor to predict evaluator points from features in estimated_points.csv

Outputs feature importances and cross-validated R^2. Requires scikit-learn. If not
available, the script will print instructions.
"""
import csv
import math
from pathlib import Path
from statistics import mean

IN = 'estimated_points.csv'

if not Path(IN).exists():
    print(f"Missing {IN} - run estimate_points_from_trace.py first")
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

print(f"Loaded {len(rows)} training rows with actual points")

# Prepare feature matrix
X = []
Y = []
for r in rows:
    # one-hot-ish risk encoding
    risk_encode = [1.0 if r['risk']=='Conservative' else 0.0,
                   1.0 if r['risk']=='Moderate' else 0.0,
                   1.0 if r['risk']=='Aggressive' else 0.0]
    feat = [r['budget_log'], r['eligible'], r['period'], r['avg_vol'], r['avg_logcap'], r['avg_pt'], r['psize']] + risk_encode
    X.append(feat)
    Y.append(r['actual'])

# Try sklearn
try:
    from sklearn.ensemble import RandomForestRegressor
    from sklearn.model_selection import cross_val_score
    import numpy as np

    X = np.array(X)
    Y = np.array(Y)

    model = RandomForestRegressor(n_estimators=200, random_state=42, n_jobs=-1)
    # 5-fold cross-validate R^2
    scores = cross_val_score(model, X, Y, cv=5, scoring='r2')
    print(f"Cross-validated R^2 scores: {scores}, mean={scores.mean():.4f}")

    # Fit on all data and show feature importances
    model.fit(X, Y)
    feature_names = ['budget_log','eligible','period','avg_vol','avg_logcap','avg_pt','psize','risk_cons','risk_mod','risk_aggr']
    importances = model.feature_importances_
    ranked = sorted(zip(feature_names, importances), key=lambda x: x[1], reverse=True)
    print("Feature importances:")
    for name, imp in ranked:
        print(f"  {name}: {imp:.4f}")

    # Save model
    try:
        import joblib
        joblib.dump(model, 'points_predictor.joblib')
        print('Saved model to points_predictor.joblib')
    except Exception:
        print('joblib not available; skipping model save')

except Exception as e:
    print('scikit-learn not available or error during training:', e)
    print('To enable training, install scikit-learn and numpy:')
    print('  pip install scikit-learn numpy joblib')
