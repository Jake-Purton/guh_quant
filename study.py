#!/usr/bin/env python3
"""Study script: find correlations between all JSON fields and the 'points' result.

Usage:
  study.py [files...] --out correlations.csv --top 20

If no files are provided, the script will try common JSON/JSONL files in the repo
such as `request_trace.jsonl` and `overbudget_events.jsonl`.

This script flattens each JSON object into dot-separated keys, extracts a
numeric `points` target (any flattened key named 'points'), and computes:
 - Pearson correlation for numeric features
 - For categorical features: group means, best category and delta vs overall mean

Outputs a CSV `correlations.csv` (or user-specified) and prints top positive/negative
features.
"""
from __future__ import annotations

import sys
import json
import argparse
from pathlib import Path
from typing import Any, Dict, Iterable, List, Tuple

import numpy as np
import pandas as pd


def flatten(obj: Any, prefix: str = "") -> Dict[str, Any]:
    """Flatten JSON-like structure into dot-separated keys.

    - dict -> recurse with keys
    - list -> if list of scalars, produce key_count and key_{i} for up to N items
             else if list of dicts, produce key_count and JSON-string summary
    - scalars -> emit directly
    """
    out: Dict[str, Any] = {}

    if isinstance(obj, dict):
        for k, v in obj.items():
            key = f"{prefix}.{k}" if prefix else k
            out.update(flatten(v, key))
    elif isinstance(obj, list):
        # record count
        out[f"{prefix}.__len__"] = len(obj)
        # try to inspect element types
        if not obj:
            return out
        if all(isinstance(x, (str, int, float, bool, type(None))) for x in obj):
            # scalar list: emit first few elements and unique count
            max_elements = 5
            for i, x in enumerate(obj[:max_elements]):
                out[f"{prefix}.{i}"] = x
            out[f"{prefix}.__unique_count"] = len(set(obj))
        elif all(isinstance(x, dict) for x in obj):
            # list of dicts: emit count and a JSON string of first element keys
            out[f"{prefix}.__sample_keys"] = ",".join(sorted(obj[0].keys()))
        else:
            # mixed types: stringify sample
            out[f"{prefix}.__sample"] = json.dumps(obj[:3])
    else:
        # scalar: normalize types
        if isinstance(obj, bool):
            out[prefix] = int(obj)
        elif obj is None:
            out[prefix] = None
        else:
            # If the scalar is a string that looks like nested JSON (e.g. the
            # evaluator stores JSON as a string in `result.response`), try to
            # parse it and flatten the nested object.
            if isinstance(obj, str):
                s = obj.strip()
                if (s.startswith('{') and s.endswith('}')) or (s.startswith('[') and s.endswith(']')):
                    try:
                        nested = json.loads(s)
                        out.update(flatten(nested, prefix))
                        return out
                    except Exception:
                        # not JSON, fall through to store raw string
                        pass
            out[prefix] = obj

    return out


def iter_json_records(paths: Iterable[Path]) -> Iterable[Dict[str, Any]]:
    """Yield JSON objects from files. Supports JSONL, JSON arrays, and single JSON objects."""
    for p in paths:
        if not p.exists():
            continue
        text = p.read_text(encoding="utf-8")
        # Try JSONL (line-delimited JSON)
        if "\n" in text and text.strip().startswith("{"):
            for line in text.splitlines():
                line = line.strip()
                if not line:
                    continue
                try:
                    yield json.loads(line)
                except Exception:
                    # fallback: try to parse as full JSON
                    break

        # Try full JSON
        try:
            data = json.loads(text)
        except Exception:
            continue

        if isinstance(data, list):
            for obj in data:
                if isinstance(obj, dict):
                    yield obj
        elif isinstance(data, dict):
            # Might be a top-level object with a 'records' or 'stocks' key
            # If so, yield nested items
            if any(isinstance(v, list) for v in data.values()):
                # yield each nested list element for all list-valued keys
                yielded = False
                for k, v in data.items():
                    if isinstance(v, list):
                        for item in v:
                            if isinstance(item, dict):
                                yield item
                                yielded = True
                if yielded:
                    continue
            # otherwise treat as single record
            yield data


def find_points_key(row: Dict[str, Any]) -> Tuple[str, Any] | Tuple[None, None]:
    """Find a flattened key named 'points' (exact match) and return (key, value)."""
    for k, v in row.items():
        if k.split(".")[-1].lower() == "points":
            return k, v
    return None, None


def coerce_numeric_series(s: pd.Series) -> Tuple[pd.Series, bool]:
    """Try to coerce a pandas Series to numeric. Returns (series, is_numeric)."""
    try:
        conv = pd.to_numeric(s, errors="coerce")
        nonnull = conv.notna().sum()
        return conv, nonnull > 0
    except Exception:
        return s, False


def analyze(paths: List[Path], out_csv: Path, top_n: int = 20):
    records: List[Dict[str, Any]] = []
    for rec in iter_json_records(paths):
        flat = flatten(rec)
        # locate points key
        k, v = find_points_key(flat)
        if k is None:
            # No explicit points found — check for error indicators and
            # treat any error as -400 points so errors are penalized.
            err_found = False

            # Raw result dict may contain ok/error fields
            if isinstance(rec, dict):
                res = rec.get("result")
                if isinstance(res, dict):
                    if res.get("ok") is False:
                        flat["__points__"] = -400
                        err_found = True
                    elif res.get("error"):
                        # Non-empty error string
                        flat["__points__"] = -400
                        err_found = True

            # Also inspect flattened keys: any key ending with '.error' that
            # has a non-empty value indicates an error to penalize.
            if not err_found:
                for fk, fv in flat.items():
                    if fk.split(".")[-1].lower() == "error":
                        if fv not in (None, "", []) :
                            flat["__points__"] = -400
                            err_found = True
                            break

            if not err_found:
                # No points and no error indicator — skip this record
                continue
        else:
            # attach the points under canonical name
            flat["__points__"] = v

        records.append(flat)

    if not records:
        print("No records with a 'points' key were found in the provided files.")
        return

    df = pd.DataFrame(records)

    # Ensure __points__ numeric
    df["__points__"] = pd.to_numeric(df["__points__"], errors="coerce")
    df = df[df["__points__"].notna()]

    target = df["__points__"]

    features = [c for c in df.columns if c != "__points__"]

    rows = []
    for f in features:
        series = df[f]
        # If all null, skip
        if series.notna().sum() == 0:
            continue

        num_series, is_numeric = coerce_numeric_series(series)
        if is_numeric:
            # compute Pearson correlation with target using pairwise non-null
            valid = num_series.notna() & target.notna()
            if valid.sum() < 3:
                corr = np.nan
            else:
                corr = np.corrcoef(num_series[valid].astype(float), target[valid].astype(float))[0, 1]
            rows.append({
                "feature": f,
                "type": "numeric",
                "n": int(valid.sum()),
                "correlation": float(corr) if not np.isnan(corr) else None,
                "notes": "",
            })
        else:
            # categorical: compute group means and delta
            grp = df.groupby(f)["__points__"].agg(["count", "mean"]).sort_values("count", ascending=False)
            if grp.empty:
                continue
            overall_mean = float(target.mean())
            best_cat = grp["mean"].idxmax()
            best_mean = float(grp.loc[best_cat, "mean"]) if "mean" in grp.columns else float(grp.iloc[0][1])
            delta = best_mean - overall_mean
            rows.append({
                "feature": f,
                "type": "categorical",
                "n": int(grp["count"].sum()),
                "correlation": None,
                "notes": f"best_cat={best_cat!s}; best_mean={best_mean:.4f}; delta={delta:.4f}",
            })

    out_df = pd.DataFrame(rows)
    # Rank numeric correlations by absolute value
    out_df["abs_corr"] = out_df["correlation"].abs()
    out_df_sorted = out_df.sort_values(["type", "abs_corr"], ascending=[True, False])

    out_df_sorted.to_csv(out_csv, index=False)

    # Print top results
    print(f"Wrote correlations to {out_csv}\n")

    num_rows = out_df[out_df["type"] == "numeric"].dropna(subset=["correlation"]).sort_values("correlation", ascending=False)
    if not num_rows.empty:
        print("Top positive numeric correlations:")
        print(num_rows.head(top_n)[["feature", "correlation", "n"]].to_string(index=False))
    else:
        print("No numeric features with valid correlations.")

    neg = out_df[out_df["type"] == "numeric"].dropna(subset=["correlation"]).sort_values("correlation", ascending=True)
    if not neg.empty:
        print("\nTop negative numeric correlations:")
        print(neg.head(top_n)[["feature", "correlation", "n"]].to_string(index=False))

    cat = out_df[out_df["type"] == "categorical"].copy()
    if not cat.empty:
        print("\nTop categorical effects (best category delta vs overall):")
        cat["delta"] = cat["notes"].str.extract(r"delta=([\-0-9\.]+)").astype(float)
        print(cat.sort_values("delta", ascending=False).head(top_n)[["feature", "n", "notes"]].to_string(index=False))


def main(argv: List[str] | None = None) -> int:
    p = argparse.ArgumentParser(description="Study JSON files for features correlated with points")
    p.add_argument("files", nargs="*", help="JSON/JSONL files to analyze")
    p.add_argument("--out", default="correlations.csv", help="Output CSV path")
    p.add_argument("--top", type=int, default=20, help="How many top features to print")
    args = p.parse_args(argv)

    paths: List[Path] = []
    if args.files:
        paths = [Path(f) for f in args.files]
    else:
        # default files commonly present in this repo
        defaults = ["request_trace.jsonl", "overbudget_events.jsonl", "stocks_cache_monthly.json", "stocks_cache.json"]
        paths = [Path(d) for d in defaults if Path(d).exists()]

    if not paths:
        print("No input files found. Pass JSON or JSONL files as arguments.")
        return 2

    analyze(paths, Path(args.out), top_n=args.top)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
