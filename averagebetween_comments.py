#!/usr/bin/env python3
"""
Compute average points at comment events and average points in the intervals between comments.

Usage:
  averagebetween_comments.py [--in request_trace.jsonl] [--out intervals.csv]

Outputs a small summary to stdout and (optionally) writes a CSV of interval means.

Heuristics:
- A record is considered a "comment event" when any flattened key ends with
  'comment' or 'comments' (case-insensitive) and has a non-empty value.
- Points are extracted the same way as `study.py`: any flattened key ending in
  'points' is used; when missing, if `result.ok` is false or an `error` is
  present the record is assigned -400 points.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any, Dict, List, Tuple
import statistics


def is_comment_flat(flat: Dict[str, Any]) -> bool:
    for k, v in flat.items():
        key_tail = k.split('.')[-1].lower()
        # Key-name based comment markers (existing behavior)
        if key_tail.endswith('comment') or key_tail.endswith('comments'):
            if v not in (None, '', [], {}):
                return True
        # Value-based marker: treat any line starting with '//' as a comment event
        if isinstance(v, str):
            for line in v.splitlines():
                if line.strip().startswith('//'):
                    return True
    return False


def find_points_key(flat: Dict[str, Any]) -> Tuple[str, Any] | Tuple[None, None]:
    for k, v in flat.items():
        if k.split('.')[-1].lower() == 'points':
            return k, v
    return None, None


def main(argv: List[str] | None = None) -> int:
    p = argparse.ArgumentParser(description="Average points between comments")
    p.add_argument('--in', dest='infile', default='request_trace.jsonl')
    p.add_argument('--out', dest='outcsv', default=None, help='Optional CSV of interval means')
    p.add_argument('--comments-out', dest='comments_out', default=None, help='Optional output path for per-comment averages (text)')
    args = p.parse_args(argv)

    infile = Path(args.infile)
    if not infile.exists():
        print('Input file not found:', infile)
        return 2

    # reuse study.py helpers (flatten + iter_json_records)
    try:
        import study as study_helpers  # type: ignore
    except Exception as e:
        print('Failed to import study helper (ensure study.py is present):', e)
        return 3

    # We'll parse the JSONL file line-by-line so we can detect standalone
    # comment lines that start with '//' and keep their position relative to
    # records. Events is a list of tuples: ('COMMENT', text) or ('RECORD', pts).
    events: List[Tuple[str, Any]] = []

    with infile.open('r', encoding='utf-8') as fh:
        for raw in fh:
            line = raw.rstrip('\n')
            if not line.strip():
                continue
            if line.lstrip().startswith('//'):
                # standalone comment line
                events.append(('COMMENT', line.strip()))
                continue
            # try to parse JSON on this line
            try:
                rec = json.loads(line)
            except Exception:
                # not a JSON object; skip
                continue

            flat = study_helpers.flatten(rec)
            k, v = find_points_key(flat)
            if k is None:
                # apply error heuristic
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
                            break
                if not err_found:
                    # skip records without points or errors
                    continue
            else:
                flat['__points__'] = v

            try:
                pts = float(flat['__points__'])
            except Exception:
                # skip if points cannot be coerced
                continue

            # also detect inline comment markers within flattened string values
            inline_comment = is_comment_flat(flat)
            if inline_comment:
                # treat this as both a record and an immediate comment marker
                events.append(('RECORD', pts))
                events.append(('COMMENT', '//inline'))
            else:
                events.append(('RECORD', pts))

    # Extract record points and comment indices from events
    record_points = [val for typ, val in events if typ == 'RECORD']
    if not record_points:
        print('No records with points found in', infile)
        return 0

    overall_mean = statistics.mean(record_points)

    # indices of comment events in the events list
    comment_idxs = [i for i, (typ, _) in enumerate(events) if typ == 'COMMENT']

    # mean of record immediately following each comment (if any)
    following_points: List[float] = []
    for ci in comment_idxs:
        # find next RECORD after ci
        for j in range(ci + 1, len(events)):
            if events[j][0] == 'RECORD':
                following_points.append(events[j][1])
                break

    mean_following_comments = statistics.mean(following_points) if following_points else None

    # intervals between consecutive comment events: mean of RECORDs strictly between them
    interval_means: List[float] = []
    for i in range(len(comment_idxs) - 1):
        start = comment_idxs[i]
        end = comment_idxs[i + 1]
        between_pts = [val for typ, val in events[start + 1:end] if typ == 'RECORD']
        if between_pts:
            interval_means.append(statistics.mean(between_pts))

    mean_between_comments = statistics.mean(interval_means) if interval_means else None

    print('Input:', infile)
    print('Total records with points:', len(record_points))
    print('Overall mean points:', f"{overall_mean:.3f}")
    if mean_following_comments is not None:
        print('Mean points of record immediately after comments:', f"{mean_following_comments:.3f}")
        print('Number of comment events with following record:', len(following_points))
    else:
        print('No comment events found')

    if mean_between_comments is not None:
        print('Mean points between comment events (average of per-interval means):', f"{mean_between_comments:.3f}")
        print('Number of intervals considered:', len(interval_means))
    else:
        print('No intervals between comment events found')

    # optionally write intervals csv
    if args.outcsv:
        import csv
        outp = Path(args.outcsv)
        with outp.open('w', newline='', encoding='utf-8') as fh:
            writer = csv.writer(fh)
            writer.writerow(['interval_index', 'start_comment_event_idx', 'end_comment_event_idx', 'interval_mean', 'n_records'])
            for idx in range(len(comment_idxs) - 1):
                start = comment_idxs[idx]
                end = comment_idxs[idx + 1]
                between_pts = [val for typ, val in events[start + 1:end] if typ == 'RECORD']
                if not between_pts:
                    continue
                writer.writerow([idx, start, end, statistics.mean(between_pts), len(between_pts)])
        print('Wrote intervals to', outp)

    # For each comment, compute the mean of RECORDs between this comment and the next comment
    per_comment_lines: List[str] = []
    for idx, ci in enumerate(comment_idxs):
        # determine the index of the next comment event (if any)
        next_ci = comment_idxs[idx + 1] if idx + 1 < len(comment_idxs) else len(events)
        # collect RECORD values strictly after ci up to next_ci
        between_pts = [val for typ, val in events[ci + 1:next_ci] if typ == 'RECORD']
        avg = statistics.mean(between_pts) if between_pts else None
        comment_text = events[ci][1]
        # shorten comment text for display
        short = comment_text.strip()
        if len(short) > 120:
            short = short[:117] + '...'
        line = f"{short}: {avg if avg is not None else 'NaN'}"
        per_comment_lines.append(line)
        print(line)

    if args.comments_out:
        outp = Path(args.comments_out)
        outp.write_text('\n'.join(per_comment_lines) + '\n', encoding='utf-8')
        print('Wrote per-comment averages to', outp)

    return 0


if __name__ == '__main__':
    raise SystemExit(main())
