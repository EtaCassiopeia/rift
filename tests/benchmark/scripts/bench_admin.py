#!/usr/bin/env python3
"""Admin-plane create/read benchmark: Rift vs Mountebank.

Where `bench_direct.py` measures request *serving* throughput, this measures the cost of the
admin control plane: **creating** an imposter with many stubs and **reading** it back. That is the
path Rift's stub-overlap analysis lives on (issue #423) — the analysis is a Rift extension that
Mountebank does not perform, so this is where the two engines' admin behaviour differs most.

For each engine and each (predicate shape, stub count) it launches a FRESH engine process (so the
RSS delta is isolated), POSTs one imposter, then repeatedly GETs it, recording:

  * create latency        — POST /imposters with N stubs
  * GET latency (x5)       — GET /imposters/:port (Rift now serves cached warnings; see #423)
  * process RSS delta      — engine memory growth from the create
  * response body size     — the create/GET payload
  * warnings               — entries under `_rift.warnings` (Mountebank: always 0)

Two shapes are exercised: `identical/overlap` (all stubs share one predicate — the O(n²)-prone
case Rift #423 fixed) and `distinct` (the cheap control). Engines run one at a time on disjoint
admin ports, mirroring `bench_direct.py`.

Usage:
  python3 bench_admin.py --run-all \
      --rift-bin ../../../target/release/rift-http-proxy \
      --mb-bin ~/bench-mb/node_modules/mountebank/bin/mb
"""
import argparse, json, os, shutil, signal, subprocess, time, urllib.request, urllib.error

HERE = os.path.dirname(os.path.abspath(__file__))
RESULTS_DIR = os.path.join(HERE, "..", "results")

GETS = 5
SIZES = [100, 1000]
RIFT_ADMIN = 2525
MB_ADMIN = 2625          # disjoint from Rift, matching bench_direct.py's +100 offset
DP_PORT = 4900           # data-plane port for the imposter under test (reused across sequential runs)


# ── stub shapes ─────────────────────────────────────────────────────────────
def identical_stubs(n):
    """All stubs share one predicate — every pair overlaps (the #423 pathology)."""
    return [{"predicates": [{"equals": {"path": "/data"}}],
             "responses": [{"is": {"statusCode": 200, "body": "x"}}]} for _ in range(n)]


def distinct_stubs(n):
    """Distinct predicates — the cheap control."""
    return [{"predicates": [{"equals": {"path": f"/p{i}"}}],
             "responses": [{"is": {"statusCode": 200, "body": "x"}}]} for i in range(n)]


SHAPES = [("identical/overlap", identical_stubs), ("distinct", distinct_stubs)]


# ── process / http helpers (self-contained; conventions mirror bench_direct.py) ──
def port_up(port, timeout=1):
    try:
        urllib.request.urlopen(f"http://127.0.0.1:{port}/", timeout=timeout)
        return True
    except urllib.error.HTTPError:
        return True  # any HTTP response means the admin plane is listening
    except Exception:
        return False


def wait_ready(port, tries=120):
    for _ in range(tries):
        if port_up(port):
            return True
        time.sleep(0.25)
    return False


def free_ports(ports):
    for p in ports:
        try:
            pids = subprocess.run(["lsof", "-ti", f"tcp:{p}"], capture_output=True, text=True).stdout.split()
        except Exception:
            pids = []
        for pid in pids:
            try:
                os.kill(int(pid), signal.SIGKILL)
            except Exception:
                pass


def launch(cmd, logpath):
    lf = open(logpath, "w")
    return subprocess.Popen(cmd, stdout=lf, stderr=subprocess.STDOUT, start_new_session=True)


def stop(proc, ports):
    if proc is not None:
        try:
            os.killpg(os.getpgid(proc.pid), signal.SIGTERM)
            proc.wait(timeout=5)
        except Exception:
            try:
                os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
            except Exception:
                pass
    free_ports(ports)
    for _ in range(40):
        if not any(port_up(p) for p in ports):
            return
        time.sleep(0.25)


def rss_mb(pid):
    out = subprocess.run(["ps", "-o", "rss=", "-p", str(pid)], capture_output=True, text=True)
    try:
        return int(out.stdout.strip()) / 1024.0
    except ValueError:
        return float("nan")


def post(url, obj):
    body = json.dumps(obj).encode()
    req = urllib.request.Request(url, data=body, method="POST",
                                 headers={"Content-Type": "application/json"})
    t = time.time()
    resp = urllib.request.urlopen(req, timeout=180)
    raw = resp.read()
    return resp.status, raw, (time.time() - t) * 1000


def get_ms(url):
    t = time.time()
    urllib.request.urlopen(url, timeout=180).read()
    return (time.time() - t) * 1000


def warning_count(raw):
    try:
        return len(json.loads(raw).get("_rift", {}).get("warnings", []))
    except Exception:
        return 0


# ── one measurement: fresh engine, create, read, teardown ───────────────────
def measure(cmd, admin_port, extra_ports, shape_build, n, logpath):
    ports = [admin_port, DP_PORT] + extra_ports
    free_ports(ports)
    proc = launch(cmd, logpath)
    try:
        if not wait_ready(admin_port):
            raise SystemExit(f"engine admin not ready on {admin_port}")
        time.sleep(0.3)
        base = rss_mb(proc.pid)
        status, body, create_ms = post(f"http://127.0.0.1:{admin_port}/imposters",
                                        {"port": DP_PORT, "protocol": "http", "stubs": shape_build(n)})
        if status not in (200, 201):
            raise SystemExit(f"create failed: HTTP {status}: {body[:200]!r}")
        peak = rss_mb(proc.pid)
        gets = sorted(get_ms(f"http://127.0.0.1:{admin_port}/imposters/{DP_PORT}") for _ in range(GETS))
        return {
            "create_ms": create_ms,
            "rss_delta_mb": peak - base,
            "body_mb": len(body) / 1024.0 / 1024.0,
            "warnings": warning_count(body),
            "get_min_ms": gets[0],
            "get_med_ms": gets[len(gets) // 2],
        }
    finally:
        stop(proc, ports)


def run_all(rift_bin, mb_bin):
    os.makedirs(RESULTS_DIR, exist_ok=True)
    node = shutil.which("node") or "node"
    engines = [
        ("rift", RIFT_ADMIN, [rift_bin, "--port", str(RIFT_ADMIN), "--loglevel", "warn"], [9090]),
        ("mb", MB_ADMIN, [node, mb_bin, "start", "--port", str(MB_ADMIN), "--loglevel", "warn"], []),
    ]
    results = {}  # (engine, shape, n) -> metrics
    for engine, admin, cmd, extra in engines:
        for shape, build in SHAPES:
            for n in SIZES:
                print(f"[{engine}] {shape}  N={n} ...", flush=True)
                m = measure(cmd, admin, extra, build, n,
                            os.path.join(RESULTS_DIR, f"admin-{engine}.log"))
                results[(engine, shape, n)] = m
                print(f"    create {m['create_ms']:.1f} ms | GET~{m['get_med_ms']:.1f} ms | "
                      f"RSS +{m['rss_delta_mb']:.1f} MB | warnings {m['warnings']}")
    write_report(results, rift_bin, mb_bin, node)


def write_report(results, rift_bin, mb_bin, node):
    def ver(cmd):
        try:
            return subprocess.run(cmd, capture_output=True, text=True).stdout.strip()
        except Exception:
            return "?"
    rift_ver = ver([rift_bin, "--version"]) or "local"
    mb_ver = ver([node, mb_bin, "--version"]) or "?"
    out = os.path.join(RESULTS_DIR, "ADMIN_BENCHMARK_REPORT.md")
    with open(out, "w") as f:
        f.write("# Rift vs Mountebank — Admin create/read benchmark\n\n")
        f.write(f"- **Date:** {time.strftime('%Y-%m-%d %H:%M:%S')}\n")
        f.write(f"- **Rift:** {rift_ver}\n- **Mountebank:** {mb_ver}\n")
        f.write("- **Method:** fresh engine process per (shape, N) on disjoint admin ports; "
                "create = `POST /imposters` with N stubs; GET = median of 5 `GET /imposters/:port`; "
                "RSS via `ps`.\n")
        f.write("- Stub-overlap analysis is a Rift extension (issue #423); Mountebank does none, "
                "so its `warnings` are always 0.\n\n")
        for shape, _ in SHAPES:
            f.write(f"## Shape: {shape}\n\n")
            f.write("| N | create (ms) — MB | create (ms) — Rift | GET med (ms) — MB | "
                    "GET med (ms) — Rift | RSS Δ (MB) — MB | RSS Δ (MB) — Rift | Rift warnings |\n")
            f.write("|--:|--:|--:|--:|--:|--:|--:|--:|\n")
            for n in SIZES:
                r = results[("rift", shape, n)]
                m = results[("mb", shape, n)]
                f.write(f"| {n} | {m['create_ms']:.1f} | {r['create_ms']:.1f} | "
                        f"{m['get_med_ms']:.1f} | {r['get_med_ms']:.1f} | "
                        f"{m['rss_delta_mb']:.1f} | {r['rss_delta_mb']:.1f} | {r['warnings']} |\n")
            f.write("\n")
    print(f"\nwrote {out}")
    return out


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--run-all", action="store_true")
    ap.add_argument("--rift-bin", default=os.path.join(HERE, "..", "..", "..", "target", "release", "rift-http-proxy"))
    ap.add_argument("--mb-bin", default=os.path.expanduser("~/bench-mb/node_modules/mountebank/bin/mb"))
    a = ap.parse_args()
    if a.run_all:
        run_all(os.path.abspath(a.rift_bin), os.path.expanduser(a.mb_bin))
    else:
        ap.print_help()
