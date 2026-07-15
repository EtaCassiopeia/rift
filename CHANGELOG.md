# Changelog

All notable changes to Rift are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

This changelog was backfilled from git history and release tags; it summarizes user-facing
changes and omits internal refactors, CI, and test-only commits. See the git log for the full
record.

## [Unreleased]

### Fixed

- **Binary request bodies are no longer silently corrupted when recorded or handed to scripts.**
  Request bodies went through `String::from_utf8_lossy`, which replaces every invalid byte with
  U+FFFD — so a protobuf, gzip or image upload was recorded as something the client never sent,
  irreversibly and with no error. Rift records and replays real traffic, so reading a recording back
  or round-tripping it silently produced wrong bytes. A non-UTF-8 body is now base64-encoded and
  marked `"_mode": "binary"` on the recorded request, mirroring how binary *response* bodies have
  always been represented; scripts get the base64 body plus `isBinary` on `ctx.request`. This is
  additive: `_mode` is absent for text bodies, so existing recordings and all-text traffic are
  unchanged. The fault-injection proxy path had the same bug and is fixed too. Where rift genuinely
  cannot classify the body (the `decorate` and predicate-`inject` paths), `isBinary` is absent
  rather than `false` — a script can tell "text" from "unknown" instead of being told something
  untrue.

- **Intercept-rule body predicates no longer evaluate against corrupted binary payloads.** The
  TLS-intercept forward-proxy path ran the intercepted request body through
  `String::from_utf8_lossy` before rule matching, replacing every invalid byte with U+FFFD — so a
  body predicate on binary traffic (protobuf, gzip, an image upload) matched or failed to match
  against garbage the client never sent. A non-UTF-8 body is now matched against its standard
  base64 encoding, the same convention used for binary recorded requests and binary responses;
  write the predicate against the base64 string. Text/JSON bodies are matched as-is, unchanged.
  Forwarding was never affected — it always relayed the raw bytes. **Behavior change:** an
  intercept-rule body predicate that deliberately matched the U+FFFD-mangled form of a binary body
  must be rewritten against the base64 encoding.

- **Query-parameter *names* are now percent-decoded everywhere, so `?first%20name=bob` matches a
  predicate on `first name` on every path.** Rift has four query/form parsers, and two of them
  decoded only the value, leaving the key raw — so the same request got a different answer
  depending on which path evaluated it: the imposter's predicate matching saw the key `first name`
  while `deepEquals` predicates, rule matching (`_rift.match.query`), response templates, and the
  request context handed to behaviors all saw `first%20name` and failed to find it. Mountebank
  decodes both key and value (Node's `querystring.parse` unescapes keys), so the raw-key paths were
  also a compatibility divergence. An undecodable key (e.g. `%FF`) passes through raw, consistent
  with behavior before this fix. Closes #642.
