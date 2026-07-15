#!/usr/bin/env bash
# Record BENCH_SAMPLES independent criterion runs of every gated bench target, saving each under
# the baseline name "<prefix>-<i>" so bench_regression.py can take the min per side (issue #446).
#
# Why the existence probe: this gate benchmarks the PR *base* as well as the head, and a gated
# bench may legitimately not exist on the base — that is exactly how a new bench lands (present on
# head, absent on base). `cargo bench --bench <name>` hard-errors on an unknown target, which would
# fail the gate on every PR that adds a bench. Skip those, and let bench_regression.py drop the
# unpaired side as it already does.
#
# The skip is announced, never silent: a bench that quietly stopped running would leave the job
# green and read as "gated and clean" when in fact nothing measured it. Same reason we probe the
# target list instead of `|| true`-ing the bench invocation — that would swallow real build and
# panic failures too, which is precisely what the gate exists to catch.
#
# Targets are selected by their workspace-unique bench name, NOT `-p <crate>`: the base checkout may
# predate a crate rename (e.g. rift-core -> rift-mock-core), so a hardcoded package name would not
# resolve there. Bench target names are stable across such renames.
set -euo pipefail

prefix="${1:?usage: run_benches.sh <baseline-prefix>}"
samples="${BENCH_SAMPLES:?BENCH_SAMPLES must be set}"
targets="${BENCH_TARGETS:?BENCH_TARGETS must be set}"

present="$(cargo metadata --no-deps --format-version 1 \
  | jq -r '.packages[].targets[] | select(.kind[] == "bench") | .name')"

for bench in $targets; do
  if ! grep -qxF "$bench" <<<"$present"; then
    echo "::notice::bench target '$bench' is absent from this checkout; skipping it for '$prefix' (expected when the PR adds a new bench)"
    continue
  fi
  for i in $(seq 1 "$samples"); do
    echo "::group::$bench ($prefix-$i)"
    cargo bench --bench "$bench" -- --save-baseline "$prefix-$i"
    echo "::endgroup::"
  done
done
