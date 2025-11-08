#!/usr/bin/env python3
"""
Simple feature-importance analysis for `study_correlations.csv`.
- Tries sklearn RandomForest + permutation importance.
- Falls back to Pearson correlations if sklearn isn't available.
Outputs CSV with feature/importances and prints a short summary.
"""
import argparse
import sys
import pandas as pd
import numpy as np


def find_target(df):
    candidates = ['__points__', 'result.response.points', 'points']
    for c in candidates:
        if c in df.columns:
            return c
    # fallback: try any column that looks like points
    for c in df.columns:
        if 'point' in c.lower():
            return c
    return None


def run_with_sklearn(X, y, top):
    from sklearn.ensemble import RandomForestRegressor
    from sklearn.model_selection import train_test_split
    from sklearn.inspection import permutation_importance

    X_train, X_test, y_train, y_test = train_test_split(X, y, test_size=0.25, random_state=42)
    rf = RandomForestRegressor(n_estimators=200, max_depth=8, random_state=42, n_jobs=-1)
    rf.fit(X_train, y_train)
    importances = rf.feature_importances_
    perm = permutation_importance(rf, X_test, y_test, n_repeats=10, random_state=42, n_jobs=-1)
    perm_means = perm.importances_mean
    df = pd.DataFrame({
        'feature': X.columns,
        'rf_importance': importances,
        'perm_importance': perm_means
    })
    df['importance'] = df[['rf_importance','perm_importance']].mean(axis=1)
    df = df.sort_values('importance', ascending=False)
    return df.head(top)


def run_fallback_corr(X, y, top):
    # Compute Pearson correlation per column, handle NaNs
    corrs = X.apply(lambda col: col.corr(y))
    corrs = corrs.abs().sort_values(ascending=False)
    df = pd.DataFrame({'feature': corrs.index, 'importance': corrs.values})
    return df.head(top)


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--in', dest='infile', default='study_correlations.csv')
    p.add_argument('--out', dest='outfile', default='feature_importances.csv')
    p.add_argument('--top', dest='top', type=int, default=20)
    args = p.parse_args()

    infile = args.infile

    # If input is a CSV, load it directly. Otherwise assume JSON/JSONL and try
    # to build a flattened records DataFrame using study.py helpers.
    if infile.lower().endswith('.csv'):
        df = pd.read_csv(infile)
    else:
        # Build flattened records from JSON/JSONL using study.py helpers when available.
        try:
            import study as study_helpers  # type: ignore
            from pathlib import Path
            paths = [Path(infile)]
            records = []
            for rec in study_helpers.iter_json_records(paths):
                flat = study_helpers.flatten(rec)
                # Find and canonicalize points as __points__ if present (mirrors study.py behavior)
                k, v = study_helpers.find_points_key(flat)
                if k is None:
                    # check for error markers and set -400 if error-like (same heuristics)
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
                    if not err_found:
                        continue
                else:
                    flat['__points__'] = v
                records.append(flat)
            if not records:
                print('No flattened records extracted from', infile, file=sys.stderr)
                sys.exit(2)
            df = pd.DataFrame(records)
        except Exception as e:
            print('Failed to build flattened dataset from JSON input:', e, file=sys.stderr)
            sys.exit(3)

    target_col = find_target(df)
    if target_col is None and '__points__' in df.columns:
        target_col = '__points__'
    if target_col is None:
        print('ERROR: no target column like __points__ or points found in', infile, file=sys.stderr)
        sys.exit(2)
    print('Using target column:', target_col)

    # Keep numeric columns only
    numeric = df.select_dtypes(include=[np.number]).copy()
    if target_col not in numeric.columns:
        # maybe target was non-numeric string; try to coerce
        numeric[target_col] = pd.to_numeric(df[target_col], errors='coerce')
    numeric = numeric.dropna(axis=0, subset=[target_col])
    y = numeric[target_col]
    X = numeric.drop(columns=[target_col])

    if X.shape[1] == 0:
        print('No numeric features found to analyze.', file=sys.stderr)
        sys.exit(3)

    try:
        import sklearn  # type: ignore
        print('sklearn detected; running RandomForest + permutation importance')
        topdf = run_with_sklearn(X, y, args.top)
        # normalize importance to 0..1
        topdf['importance'] = (topdf['importance'] - topdf['importance'].min()) / (topdf['importance'].max() - topdf['importance'].min() + 1e-12)
        topdf[['feature','importance']].to_csv(args.outfile, index=False)
        print('\nTop features (method: mean of RF and permutation):')
        print(topdf[['feature','importance']].to_string(index=False))
    except Exception as e:
        print('sklearn not available or failed; falling back to Pearson correlations. (', e,')')
        topdf = run_fallback_corr(X, y, args.top)
        topdf.to_csv(args.outfile, index=False)
        print('\nTop features by absolute Pearson correlation:')
        print(topdf.to_string(index=False))

if __name__ == '__main__':
    main()
