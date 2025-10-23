use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use hyper::{HeaderMap, Method, Uri};
use rift_http_proxy::config::{FaultConfig, LatencyFault, MatchConfig, PathMatch, Rule};
use rift_http_proxy::matcher::{find_matching_rule, CompiledRule};
use rift_http_proxy::predicate::{PathMatcher, PredicateOptions, RequestPredicate, StringMatcher};
use rift_http_proxy::rule_index::RuleIndex;

fn create_test_rule(id: usize, path: &str, is_regex: bool) -> Rule {
    Rule {
        id: format!("rule-{id}"),
        match_config: MatchConfig {
            methods: vec!["GET".to_string(), "POST".to_string()],
            path: if is_regex {
                PathMatch::Regex {
                    regex: format!(r"^{path}$"),
                }
            } else {
                PathMatch::Prefix {
                    prefix: path.to_string(),
                }
            },
            headers: vec![],
            header_predicates: vec![],
            query: vec![],
            body: None,
            case_sensitive: true,
        },
        fault: FaultConfig {
            latency: Some(LatencyFault {
                probability: 0.5,
                min_ms: 100,
                max_ms: 200,
            }),
            error: None,
            tcp_fault: None,
        },
        upstream: None,
    }
}

fn compile_rules(count: usize) -> Vec<CompiledRule> {
    (0..count)
        .map(|i| {
            let rule = create_test_rule(i, &format!("/api/v1/endpoint{i}"), false);
            CompiledRule::compile(rule).unwrap()
        })
        .collect()
}

fn compile_rules_with_regex(count: usize) -> Vec<CompiledRule> {
    (0..count)
        .map(|i| {
            let rule = create_test_rule(i, &format!("/api/v\\d+/endpoint{i}"), true);
            CompiledRule::compile(rule).unwrap()
        })
        .collect()
}

fn bench_rule_matching(c: &mut Criterion) {
    let mut group = c.benchmark_group("rule_matching");

    // Test with different rule counts
    for rule_count in [10, 50, 100, 500, 1000].iter() {
        let rules = compile_rules(*rule_count);

        // Test matching first rule (best case)
        let uri_first: Uri = "http://localhost/api/v1/endpoint0".parse().unwrap();
        let method = Method::GET;
        let headers = HeaderMap::new();

        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::new("match_first", rule_count),
            rule_count,
            |b, _| {
                b.iter(|| {
                    find_matching_rule(
                        black_box(&rules),
                        black_box(&method),
                        black_box(&uri_first),
                        black_box(&headers),
                    )
                });
            },
        );

        // Test matching middle rule (average case)
        let middle = rule_count / 2;
        let uri_middle: Uri = format!("http://localhost/api/v1/endpoint{middle}")
            .parse()
            .unwrap();

        group.bench_with_input(
            BenchmarkId::new("match_middle", rule_count),
            rule_count,
            |b, _| {
                b.iter(|| {
                    find_matching_rule(
                        black_box(&rules),
                        black_box(&method),
                        black_box(&uri_middle),
                        black_box(&headers),
                    )
                });
            },
        );

        // Test matching last rule (worst case)
        let last = rule_count - 1;
        let uri_last: Uri = format!("http://localhost/api/v1/endpoint{last}")
            .parse()
            .unwrap();

        group.bench_with_input(
            BenchmarkId::new("match_last", rule_count),
            rule_count,
            |b, _| {
                b.iter(|| {
                    find_matching_rule(
                        black_box(&rules),
                        black_box(&method),
                        black_box(&uri_last),
                        black_box(&headers),
                    )
                });
            },
        );

        // Test no match (worst case - scans all rules)
        let uri_none: Uri = "http://localhost/not/found".parse().unwrap();

        group.bench_with_input(
            BenchmarkId::new("match_none", rule_count),
            rule_count,
            |b, _| {
                b.iter(|| {
                    find_matching_rule(
                        black_box(&rules),
                        black_box(&method),
                        black_box(&uri_none),
                        black_box(&headers),
                    )
                });
            },
        );
    }

    group.finish();
}

fn bench_regex_matching(c: &mut Criterion) {
    let mut group = c.benchmark_group("regex_matching");

    // Test regex performance with different rule counts
    for rule_count in [10, 50, 100].iter() {
        let rules = compile_rules_with_regex(*rule_count);
        let uri: Uri = "http://localhost/api/v1/endpoint50".parse().unwrap();
        let method = Method::GET;
        let headers = HeaderMap::new();

        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::new("regex_match", rule_count),
            rule_count,
            |b, _| {
                b.iter(|| {
                    find_matching_rule(
                        black_box(&rules),
                        black_box(&method),
                        black_box(&uri),
                        black_box(&headers),
                    )
                });
            },
        );
    }

    group.finish();
}

fn bench_single_rule_evaluation(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_rule_eval");

    let rule = create_test_rule(0, "/api/v1/test", false);
    let compiled = CompiledRule::compile(rule).unwrap();

    let uri: Uri = "http://localhost/api/v1/test".parse().unwrap();
    let method = Method::GET;
    let headers = HeaderMap::new();

    group.throughput(Throughput::Elements(1));
    group.bench_function("single_match", |b| {
        b.iter(|| compiled.matches(black_box(&method), black_box(&uri), black_box(&headers)));
    });

    group.finish();
}

// =============================================================================
// RuleIndex Benchmarks (Optimized matching with Radix Trie + Aho-Corasick)
// =============================================================================

fn create_predicate_exact(path: &str, method: Option<&str>) -> RequestPredicate {
    RequestPredicate {
        method: method.map(|m| StringMatcher::Equals(m.to_string())),
        path: Some(PathMatcher::Exact {
            exact: path.to_string(),
        }),
        headers: vec![],
        query: vec![],
        body: None,
        options: PredicateOptions::default(),
    }
}

fn create_predicate_prefix(path: &str, method: Option<&str>) -> RequestPredicate {
    RequestPredicate {
        method: method.map(|m| StringMatcher::Equals(m.to_string())),
        path: Some(PathMatcher::Prefix {
            prefix: path.to_string(),
        }),
        headers: vec![],
        query: vec![],
        body: None,
        options: PredicateOptions::default(),
    }
}

fn create_predicate_contains(pattern: &str) -> RequestPredicate {
    RequestPredicate {
        method: None,
        path: Some(PathMatcher::Contains {
            contains: pattern.to_string(),
        }),
        headers: vec![],
        query: vec![],
        body: None,
        options: PredicateOptions::default(),
    }
}

fn bench_rule_index_exact(c: &mut Criterion) {
    let mut group = c.benchmark_group("rule_index_exact");

    for rule_count in [10, 50, 100, 500, 1000].iter() {
        // Build index with exact path rules
        let predicates: Vec<_> = (0..*rule_count)
            .map(|i| {
                (
                    format!("rule-{i}"),
                    create_predicate_exact(&format!("/api/v1/endpoint{i}"), Some("GET")),
                    i as u32,
                )
            })
            .collect();

        let index = RuleIndex::build(predicates).unwrap();

        // Benchmark finding first rule
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::new("find_first", rule_count),
            rule_count,
            |b, _| {
                b.iter(|| {
                    index.find_candidates(black_box("/api/v1/endpoint0"), black_box(Some("GET")))
                });
            },
        );

        // Benchmark finding middle rule
        let middle = rule_count / 2;
        group.bench_with_input(
            BenchmarkId::new("find_middle", rule_count),
            rule_count,
            |b, _| {
                b.iter(|| {
                    index.find_candidates(
                        black_box(&format!("/api/v1/endpoint{middle}")),
                        black_box(Some("GET")),
                    )
                });
            },
        );

        // Benchmark no match
        group.bench_with_input(
            BenchmarkId::new("find_none", rule_count),
            rule_count,
            |b, _| {
                b.iter(|| index.find_candidates(black_box("/not/found"), black_box(Some("GET"))));
            },
        );
    }

    group.finish();
}

fn bench_rule_index_prefix(c: &mut Criterion) {
    let mut group = c.benchmark_group("rule_index_prefix");

    for rule_count in [10, 50, 100, 500].iter() {
        // Build index with prefix rules
        let predicates: Vec<_> = (0..*rule_count)
            .map(|i| {
                (
                    format!("rule-{i}"),
                    create_predicate_prefix(&format!("/api/v{i}"), None),
                    i as u32,
                )
            })
            .collect();

        let index = RuleIndex::build(predicates).unwrap();

        // Benchmark prefix matching
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::new("find_prefix", rule_count),
            rule_count,
            |b, _| {
                b.iter(|| index.find_candidates(black_box("/api/v50/users/123"), black_box(None)));
            },
        );
    }

    group.finish();
}

fn bench_rule_index_contains(c: &mut Criterion) {
    let mut group = c.benchmark_group("rule_index_contains");

    for rule_count in [10, 50, 100, 500].iter() {
        // Build index with contains rules (uses Aho-Corasick)
        let predicates: Vec<_> = (0..*rule_count)
            .map(|i| {
                (
                    format!("rule-{i}"),
                    create_predicate_contains(&format!("pattern{i}")),
                    i as u32,
                )
            })
            .collect();

        let index = RuleIndex::build(predicates).unwrap();

        // Benchmark Aho-Corasick multi-pattern matching
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::new("find_contains", rule_count),
            rule_count,
            |b, _| {
                b.iter(|| {
                    index.find_candidates(black_box("/api/v1/pattern50/data"), black_box(None))
                });
            },
        );

        // Benchmark no match (full Aho-Corasick scan)
        group.bench_with_input(
            BenchmarkId::new("find_contains_none", rule_count),
            rule_count,
            |b, _| {
                b.iter(|| {
                    index.find_candidates(black_box("/api/v1/nomatch/data"), black_box(None))
                });
            },
        );
    }

    group.finish();
}

fn bench_linear_vs_indexed(c: &mut Criterion) {
    let mut group = c.benchmark_group("linear_vs_indexed");

    // Compare linear scan vs indexed lookup for 1000 rules
    let rule_count = 1000;

    // Build linear rules (existing system)
    let linear_rules: Vec<CompiledRule> = (0..rule_count)
        .map(|i| {
            let rule = create_test_rule(i, &format!("/api/v1/endpoint{i}"), false);
            CompiledRule::compile(rule).unwrap()
        })
        .collect();

    // Build indexed rules (new system)
    let predicates: Vec<_> = (0..rule_count)
        .map(|i| {
            (
                format!("rule-{i}"),
                create_predicate_exact(&format!("/api/v1/endpoint{i}"), Some("GET")),
                i as u32,
            )
        })
        .collect();
    let index = RuleIndex::build(predicates).unwrap();

    let uri: Uri = "http://localhost/api/v1/endpoint500".parse().unwrap();
    let method = Method::GET;
    let headers = HeaderMap::new();

    group.throughput(Throughput::Elements(1));

    // Linear scan (existing)
    group.bench_function("linear_1000_middle", |b| {
        b.iter(|| {
            find_matching_rule(
                black_box(&linear_rules),
                black_box(&method),
                black_box(&uri),
                black_box(&headers),
            )
        });
    });

    // Indexed lookup (new)
    group.bench_function("indexed_1000_middle", |b| {
        b.iter(|| index.find_candidates(black_box("/api/v1/endpoint500"), black_box(Some("GET"))));
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_rule_matching,
    bench_regex_matching,
    bench_single_rule_evaluation,
    bench_rule_index_exact,
    bench_rule_index_prefix,
    bench_rule_index_contains,
    bench_linear_vs_indexed
);
criterion_main!(benches);
