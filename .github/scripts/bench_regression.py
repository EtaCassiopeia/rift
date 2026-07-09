#!/usr/bin/env python3
"""Gate a PR on matcher benchmark regressions (issue #298; hardened for noise in #446).

Reads criterion's saved baselines from `target/criterion` and compares each benchmark's mean
estimate between the PR base and head. Posts a markdown table to the GitHub job summary and exits
non-zero if any benchmark regressed beyond REGRESSION_THRESHOLD (a relative multiple).

Noise handling (issue #446): the matcher benches run in ~150 ns, and criterion on a shared CI
runner swings ±20-25% run-to-run at that scale (verified: re-running the *identical* build against
its own baseline reported a 23% "change"). A single base-vs-head comparison therefore produced
false +50-67% "regressions" on PRs that never touched the matching path. To fix that at the source
without losing the gate, the workflow now records N independent runs per side (baseline names
`base-1..N` / `pr-1..N`), and this script compares the **minimum** mean per side: noise only ever
makes a run slower, so the fastest run is the least-contended, most-representative measurement, and
min-of-N collapses the cross-run jitter. Legacy single `base`/`pr` baselines are still accepted.

Reading criterion's `estimates.json` directly avoids brittle text parsing; an unpaired or
unparseable benchmark is skipped rather than failing the run.
"""

import glob
import json
import os
import sys

CRITERION_DIR = "target/criterion"
# Relative multiple beyond which the job fails, applied to the min-of-N per side. With best-of-N
# sampling the residual noise is well under 10%, so this catches genuine large regressions (issue
# #446 raised it from 1.25 to give clear headroom over the measured noise floor).
THRESHOLD = float(os.environ.get("REGRESSION_THRESHOLD", "1.5"))


def mean_ns(estimates_path):
    try:
        with open(estimates_path) as fh:
            return float(json.load(fh)["mean"]["point_estimate"])
    except (OSError, ValueError, KeyError, TypeError):
        return None


def collect(side_prefix):
    """Map each benchmark id -> the minimum mean (ns) across every `<side_prefix>*` baseline.

    A criterion path is `target/criterion/<bench id...>/<baseline>/estimates.json`; the baseline
    segment is the one directly above `estimates.json`. We bucket by the bench id (everything before
    that baseline segment) and keep the fastest run per bench, so `base`, `base-1`, `base-2`, ...
    all fold into one min for that side.
    """
    best = {}
    for path in glob.glob(f"{CRITERION_DIR}/**/estimates.json", recursive=True):
        rel = path[len(CRITERION_DIR) + 1 : -len("/estimates.json")]
        if "/" not in rel:
            continue
        bench_id, baseline = rel.rsplit("/", 1)
        if not (baseline == side_prefix or baseline.startswith(side_prefix + "-")):
            continue
        m = mean_ns(path)
        if m is None or m <= 0:
            continue
        if bench_id not in best or m < best[bench_id]:
            best[bench_id] = m
    return best


def main():
    base = collect("base")
    pr = collect("pr")

    rows = []  # (name, base_ns, pr_ns, ratio)
    for name, base_ns in base.items():
        if name in pr:
            rows.append((name, base_ns, pr[name], pr[name] / base_ns))

    if not rows:
        print("No paired benchmarks found to compare; skipping the perf gate.")
        return 0

    rows.sort(key=lambda r: r[3], reverse=True)

    def fmt_ns(v):
        return f"{v/1000:.2f} µs" if v >= 1000 else f"{v:.1f} ns"

    lines = [
        "## Matcher benchmark regression gate",
        "",
        f"Threshold: fail if any benchmark is more than **{(THRESHOLD-1)*100:.0f}%** slower than "
        "base (best-of-N per side; see #446).",
        "",
        "| Benchmark | base (min) | PR (min) | change |",
        "|---|--:|--:|--:|",
    ]
    regressed = []
    for name, base_ns, pr_ns, ratio in rows:
        pct = (ratio - 1) * 100
        flag = " ⚠️" if ratio > THRESHOLD else (" 🟢" if ratio < 0.9 else "")
        lines.append(f"| `{name}` | {fmt_ns(base_ns)} | {fmt_ns(pr_ns)} | {pct:+.1f}%{flag} |")
        if ratio > THRESHOLD:
            regressed.append((name, pct))

    summary = "\n".join(lines) + "\n"
    summary_path = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary_path:
        with open(summary_path, "a") as fh:
            fh.write(summary)
    print(summary)

    if regressed:
        worst = ", ".join(f"{n} (+{p:.1f}%)" for n, p in regressed)
        print(f"::error::Benchmark regression beyond {(THRESHOLD-1)*100:.0f}% threshold: {worst}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
