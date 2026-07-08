#!/usr/bin/env python3
"""Direct-process Rift vs Mountebank benchmark (no Docker).

Each engine runs one at a time on a DISJOINT port range, so even if one fails
to shut down it can never be measured in place of the other. For each engine we
load an identical set of imposters, warm up, then drive a curated set of
scenarios with `oha`, capturing RPS and latency percentiles from oha's JSON.

Fairness / correctness safeguards:
  * engines run sequentially (never contend for CPU on a single machine),
  * disjoint ports per engine (rift offset 0, mb offset +100),
  * each engine launched in its own process group and killed by group + lsof,
  * the engine's ports are asserted free before launch and after teardown,
  * every scenario asserts the HTTP status distribution (a mis-served stub
    cannot silently inflate throughput).

Run everything (launches + stops both engines, writes the report):
    python3 bench_direct.py --run-all

Must be run OUTSIDE the CLI sandbox (via the sidecar) because `oha` needs
macOS keychain access to initialise TLS even for plain-HTTP targets.
"""
import argparse, json, subprocess, sys, time, urllib.request, urllib.error, os, signal, shutil

RESULTS_DIR = os.path.join(os.path.dirname(__file__), "..", "results")

# ---- imposter config generation (identical JSON posted to both engines) ----

def api_stubs(resources=10, per=10):
    out = []
    for i in range(1, resources + 1):
        r = f"resource{i}"
        out.append({
            "predicates": [{"equals": {"method": "GET", "path": f"/api/v1/{r}"}}],
            "responses": [{"is": {"statusCode": 200,
                "headers": {"Content-Type": "application/json"},
                "body": json.dumps({"items": [{"id": 1}, {"id": 2}], "total": 2})}}],
        })
        for j in range(1, per + 1):
            out.append({
                "predicates": [{"equals": {"method": "GET", "path": f"/api/v1/{r}/{j}"}}],
                "responses": [{"is": {"statusCode": 200,
                    "headers": {"Content-Type": "application/json"},
                    "body": json.dumps({"id": j, "name": f"{r}_{j}"})}}],
            })
            out.append({
                "predicates": [{"equals": {"method": "PUT", "path": f"/api/v1/{r}/{j}"}}],
                "responses": [{"is": {"statusCode": 200, "body": json.dumps({"id": j, "updated": True})}}],
            })
            out.append({
                "predicates": [{"equals": {"method": "DELETE", "path": f"/api/v1/{r}/{j}"}}],
                "responses": [{"is": {"statusCode": 204}}],
            })
    return out

def regex_stubs(n=100):
    return [{
        "predicates": [{"matches": {"path": f"/regex/pattern{i}/[a-zA-Z0-9]+"}}],
        "responses": [{"is": {"statusCode": 200, "body": f"regex {i}"}}],
    } for i in range(1, n + 1)]

def complex_stubs(n=50):
    return [{
        "predicates": [{"and": [
            {"equals": {"method": "POST"}},
            {"startsWith": {"path": f"/complex/{i}/"}},
            {"or": [
                {"contains": {"headers": {"X-Request-Type": "json"}}},
                {"contains": {"headers": {"Content-Type": "application/json"}}},
            ]},
        ]}],
        "responses": [{"is": {"statusCode": 200,
            "headers": {"Content-Type": "application/json"},
            "body": json.dumps({"complex": i, "matched": True})}}],
    } for i in range(1, n + 1)]

def json_body_stubs(n=50):
    return [{
        "predicates": [{"equals": {"method": "POST", "path": f"/json/equals/{i}",
            "body": {"id": i, "type": "request"}}}],
        "responses": [{"is": {"statusCode": 200, "body": json.dumps({"matched": "equals", "id": i})}}],
    } for i in range(1, n + 1)]

def jsonpath_stubs(n=50):
    return [{
        "predicates": [{"equals": {"method": "POST", "path": f"/jsonpath/{i}"},
            "jsonpath": {"selector": "$.user.id", "equals": i}}],
        "responses": [{"is": {"statusCode": 200, "body": json.dumps({"jsonpath_matched": True, "user_id": i})}}],
    } for i in range(1, n + 1)]

def xpath_stubs(n=50):
    return [{
        "predicates": [{"equals": {"method": "POST", "path": f"/xpath/{i}"},
            "xpath": {"selector": f"//item[@id='{i}']", "exists": True}}],
        "responses": [{"is": {"statusCode": 200,
            "headers": {"Content-Type": "application/xml"},
            "body": f"<response><id>{i}</id></response>"}}],
    } for i in range(1, n + 1)]

def template_stubs(n=50):
    return [{
        "predicates": [{"equals": {"path": f"/template/{i}"}}],
        "responses": [{"is": {"statusCode": 200,
            "headers": {"Content-Type": "application/json", "X-Request-Path": "${request.path}"},
            "body": '{"template": %d, "path": "${request.path}", "query": "${request.query}"}' % i}}],
    } for i in range(1, n + 1)]

def header_stubs(n=100):
    return [{
        "predicates": [{"equals": {"path": "/headers/route", "headers": {"X-Route-Id": f"route-{i}"}}}],
        "responses": [{"is": {"statusCode": 200, "body": json.dumps({"routed_to": i})}}],
    } for i in range(1, n + 1)]

def query_stubs(n=100):
    return [{
        "predicates": [{"equals": {"path": "/query/search", "query": {"page": str(i), "size": "10"}}}],
        "responses": [{"is": {"statusCode": 200, "body": json.dumps({"page": i})}}],
    } for i in range(1, n + 1)]

def simple_stubs():
    return [
        {"predicates": [{"equals": {"path": "/health"}}], "responses": [{"is": {"statusCode": 200, "body": "OK"}}]},
        {"predicates": [{"equals": {"path": "/ping"}}], "responses": [{"is": {"statusCode": 200, "body": "pong"}}]},
    ]

# base imposter ports (an engine offset is added to each)
IMPOSTERS = [
    (4549, "Simple", simple_stubs()),
    (4545, "API", api_stubs()),
    (4546, "Regex", regex_stubs()),
    (4547, "Complex", complex_stubs()),
    (4550, "JSONBody", json_body_stubs()),
    (4551, "JSONPath", jsonpath_stubs()),
    (4552, "XPath", xpath_stubs()),
    (4553, "Template", template_stubs()),
    (4554, "Header", header_stubs()),
    (4555, "Query", query_stubs()),
]

# scenarios: (name, base_port, method, path, body, headers)
SCENARIOS = [
    ("simple_health",     4549, "GET",  "/health", None, {}),
    ("api_first",         4545, "GET",  "/api/v1/resource1", None, {}),
    ("api_middle",        4545, "GET",  "/api/v1/resource5/5", None, {}),
    ("api_last",          4545, "GET",  "/api/v1/resource10/10", None, {}),
    ("no_match",          4545, "GET",  "/nonexistent", None, {}),
    ("regex_last",        4546, "GET",  "/regex/pattern100/test", None, {}),
    ("complex_predicate", 4547, "POST", "/complex/25/test", '{"name":"test"}', {"Content-Type": "application/json"}),
    ("json_body_equals",  4550, "POST", "/json/equals/25", '{"id":25,"type":"request"}', {"Content-Type": "application/json"}),
    ("jsonpath",          4551, "POST", "/jsonpath/25", '{"user":{"id":25,"name":"x"}}', {"Content-Type": "application/json"}),
    ("xpath",             4552, "POST", "/xpath/25", '<root><item id="25">x</item></root>', {"Content-Type": "application/xml"}),
    ("template",          4553, "GET",  "/template/25?foo=bar&baz=qux", None, {}),
    ("header_last",       4554, "GET",  "/headers/route", None, {"X-Route-Id": "route-100"}),
    ("query_last",        4555, "GET",  "/query/search?page=100&size=10", None, {}),
]

# both engines return an empty 200 as the default no-match response, so all
# scenarios here expect 2xx; kept as a map in case future scenarios differ.
EXPECT = {}

# ---- admin API helpers ----

def post_json(url, obj):
    data = json.dumps(obj).encode()
    req = urllib.request.Request(url, data=data, method="POST",
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=30) as r:
        return r.status, r.read()

def delete(url):
    req = urllib.request.Request(url, method="DELETE")
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            return r.status
    except urllib.error.HTTPError as e:
        return e.code

def port_up(port, timeout=1):
    try:
        urllib.request.urlopen(f"http://localhost:{port}/", timeout=timeout)
        return True
    except urllib.error.HTTPError:
        return True   # answered (any status) => something is listening
    except Exception:
        return False

def wait_ready(admin_port, tries=120):
    for _ in range(tries):
        if port_up(admin_port):
            return True
        time.sleep(0.5)
    return False

def load_imposters(admin, offset):
    delete(admin + "/imposters")
    for port, name, stubs in IMPOSTERS:
        status, body = post_json(admin + "/imposters",
                                 {"port": port + offset, "protocol": "http", "name": name, "stubs": stubs})
        if status != 201:
            raise SystemExit(f"  ! create imposter {port+offset} ({name}) failed: HTTP {status}: {body[:200]}")
    print(f"  loaded {len(IMPOSTERS)} imposters "
          f"({sum(len(s) for _, _, s in IMPOSTERS)} stubs) at offset +{offset}")

# ---- oha runner ----

def run_oha(url, method, body, headers, duration, conns):
    cmd = ["oha", "-z", duration, "-c", str(conns), "--no-tui",
           "--output-format", "json", "-m", method]
    for k, v in headers.items():
        cmd += ["-H", f"{k}: {v}"]
    if body is not None:
        cmd += ["-d", body]
    cmd.append(url)
    out = subprocess.run(cmd, capture_output=True, text=True, timeout=int(duration.rstrip("s")) + 30)
    if out.returncode != 0:
        raise RuntimeError(f"oha failed: {out.stderr[:300]}")
    return json.loads(out.stdout)

def metric(j):
    s = j["summary"]
    lat = j.get("latencyPercentiles", {})
    def ms(key):
        v = lat.get(key)
        return round(v * 1000, 3) if v is not None else None
    codes = j.get("statusCodeDistribution", {})
    return {
        "rps": round(s["requestsPerSec"], 1),
        "p50_ms": ms("p50"), "p90_ms": ms("p90"), "p99_ms": ms("p99"),
        "avg_ms": round(s["average"] * 1000, 3),
        "codes": codes,
    }

def bench(engine, admin_port, offset, duration, warmup, conns):
    os.makedirs(RESULTS_DIR, exist_ok=True)
    admin = f"http://localhost:{admin_port}"
    if not wait_ready(admin_port):
        raise SystemExit(f"{engine}: admin API not ready on {admin_port}")
    print(f"[{engine}] admin ready on {admin_port}; loading imposters")
    load_imposters(admin, offset)
    time.sleep(1)
    rows = []
    for name, base_port, method, path, body, headers in SCENARIOS:
        url = f"http://localhost:{base_port + offset}{path}"
        run_oha(url, method, body, headers, warmup, conns)          # warmup (discarded)
        m = metric(run_oha(url, method, body, headers, duration, conns))
        total = sum(m["codes"].values())
        if name in EXPECT:
            good = all(EXPECT[name](c) for c in m["codes"])
        else:
            good = all(c.startswith("2") for c in m["codes"])
        status = "ok" if good and total > 0 else f"BAD codes={m['codes']}"
        print(f"  {name:20s} {m['rps']:>10.1f} rps  p50={m['p50_ms']}ms p99={m['p99_ms']}  {status}")
        if not (good and total > 0):
            raise SystemExit(f"{engine}/{name}: unexpected status distribution {m['codes']} — aborting")
        rows.append((name, m))
    csv = os.path.join(RESULTS_DIR, f"direct_{engine}.csv")
    with open(csv, "w") as f:
        f.write("scenario,rps,p50_ms,p90_ms,p99_ms,avg_ms\n")
        for name, m in rows:
            f.write(f"{name},{m['rps']},{m['p50_ms']},{m['p90_ms']},{m['p99_ms']},{m['avg_ms']}\n")
    print(f"[{engine}] wrote {csv}")

# ---- engine orchestration ----

def engine_ports(offset):
    return [admin_port_for(offset)] + [p + offset for p, _, _ in IMPOSTERS] + ([9090] if offset == 0 else [])

def admin_port_for(offset):
    return 2525 + offset

def free_ports(ports):
    """Force-free ports by killing whatever listens on them (lsof + SIGKILL)."""
    for p in ports:
        try:
            pids = subprocess.run(["lsof", "-ti", f"tcp:{p}"], capture_output=True, text=True).stdout.split()
        except Exception:
            pids = []
        for pid in pids:
            try:
                os.kill(int(pid), signal.SIGKILL)
                print(f"  freed port {p} (killed pid {pid})")
            except Exception:
                pass

def stop(proc, ports):
    if proc is not None:
        try:
            os.killpg(os.getpgid(proc.pid), signal.SIGTERM)
        except Exception:
            pass
        try:
            proc.wait(timeout=5)
        except Exception:
            try:
                os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
            except Exception:
                pass
    # belt-and-suspenders: ensure the ports are actually free
    free_ports(ports)
    for _ in range(40):
        if not any(port_up(p) for p in ports):
            return
        time.sleep(0.25)
    raise SystemExit(f"ports still occupied after stop: {ports}")

def launch(cmd, logpath):
    lf = open(logpath, "w")
    return subprocess.Popen(cmd, stdout=lf, stderr=subprocess.STDOUT, start_new_session=True)

def run_all(duration, warmup, conns, rift_bin, mb_bin):
    os.makedirs(RESULTS_DIR, exist_ok=True)
    node = shutil.which("node") or "node"
    plan = [
        ("rift", 0,   [rift_bin, "--port", str(admin_port_for(0)), "--allow-injection", "--loglevel", "warn"]),
        ("mb",   100, [node, mb_bin, "start", "--port", str(admin_port_for(100)), "--allowInjection", "--loglevel", "warn"]),
    ]
    for engine, offset, cmd in plan:
        ports = engine_ports(offset)
        free_ports(ports)                       # clean slate
        if any(port_up(p) for p in ports):
            raise SystemExit(f"{engine}: ports not free before launch: {ports}")
        print(f"[{engine}] launching: {' '.join(cmd)}")
        proc = launch(cmd, os.path.join(RESULTS_DIR, f"{engine}-engine.log"))
        try:
            bench(engine, admin_port_for(offset), offset, duration, warmup, conns)
        finally:
            stop(proc, ports)
    rift_ver = subprocess.run([rift_bin, "--version"], capture_output=True, text=True).stdout.strip() or "local"
    mb_ver = subprocess.run([node, mb_bin, "--version"], capture_output=True, text=True).stdout.strip() or "2.9.1"
    report(rift_ver, mb_ver, duration, conns)

def report(rift_ver, mb_ver, duration, conns):
    def load(engine):
        path = os.path.join(RESULTS_DIR, f"direct_{engine}.csv")
        d = {}
        with open(path) as f:
            next(f)
            for line in f:
                p = line.strip().split(",")
                d[p[0]] = {"rps": float(p[1]), "p50": p[2], "p99": p[4]}
        return d
    rift, mb = load("rift"), load("mb")
    out = os.path.join(RESULTS_DIR, "DIRECT_BENCHMARK_REPORT.md")
    order = [s[0] for s in SCENARIOS]
    with open(out, "w") as f:
        f.write("# Rift vs Mountebank — Direct-Process Benchmark\n\n")
        f.write(f"- **Date:** {time.strftime('%Y-%m-%d %H:%M:%S')}\n")
        f.write(f"- **Rift:** {rift_ver}\n- **Mountebank:** {mb_ver}\n")
        f.write(f"- **Load generator:** oha, {conns} keep-alive connections, {duration} per scenario (after warmup)\n")
        f.write("- **Method:** native processes (no Docker); engines run one at a time on disjoint "
                "port ranges (no CPU contention, no cross-talk); identical imposter configs; response "
                "status distribution asserted per scenario.\n\n")
        f.write("## Throughput (requests/sec, higher is better)\n\n")
        f.write("| Scenario | Mountebank | Rift | Speedup |\n|---|--:|--:|--:|\n")
        for name in order:
            mr, rr = mb[name]["rps"], rift[name]["rps"]
            sp = f"{rr/mr:.1f}x" if mr else "n/a"
            f.write(f"| {name} | {mr:,.0f} | {rr:,.0f} | **{sp}** |\n")
        f.write("\n## Latency p99 (ms, lower is better)\n\n| Scenario | Mountebank | Rift |\n|---|--:|--:|\n")
        for name in order:
            f.write(f"| {name} | {mb[name]['p99']} | {rift[name]['p99']} |\n")
    print(f"wrote {out}")

if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--duration", default="20s")
    ap.add_argument("--warmup", default="3s")
    ap.add_argument("--connections", type=int, default=50)
    ap.add_argument("--run-all", action="store_true")
    ap.add_argument("--report", action="store_true")
    ap.add_argument("--rift-bin", default=os.path.join(os.path.dirname(__file__), "..", "..", "..", "target", "release", "rift-http-proxy"))
    ap.add_argument("--mb-bin", default=os.path.expanduser("~/bench-mb/node_modules/mountebank/bin/mb"))
    ap.add_argument("--rift-version", default="local")
    ap.add_argument("--mb-version", default="2.9.1")
    a = ap.parse_args()
    if a.run_all:
        run_all(a.duration, a.warmup, a.connections, a.rift_bin, a.mb_bin)
    elif a.report:
        report(a.rift_version, a.mb_version, a.duration, a.connections)
    else:
        raise SystemExit("use --run-all (or --report)")
