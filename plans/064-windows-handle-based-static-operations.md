# Phase 64 — Windows Handle-Based Static Operations

## Goal

Complete the Windows static-serving pipeline so metadata, conditional and range planning, file streaming, index lookup, and optional directory listing all operate from handles validated by Plan 063. Eliminate any remaining validate-by-handle/reopen-by-path gap.

## Preconditions

- Plan 063 provides pinned-root, component-relative no-reparse resolution.
- Final files and directories are represented as owned validated resources.
- Windows remains a candidate platform until Plan 065 qualification completes.

## Non-goals

Do not add:

- reparse following to the hardened profile;
- filesystem writes;
- file watching or cache invalidation;
- virtual hosting or multiple roots;
- application routing;
- directory listing enabled by default;
- SMB or non-NTFS hardened support;
- mmap or sendfile-style optimization unless required for correctness and independently reviewed.

## Central invariant

> Every Windows static response is planned and emitted from the exact opened object returned by the hardened resolver. No response-critical operation reopens the object by pathname.

## Track A — File object integration

Convert the validated Windows handle into the canonical internal file object used by the response pipeline.

Requirements:

- ownership transfers exactly once;
- no double close;
- no leaked raw handle;
- async reads and seeks work;
- cancellation and response drop close the object;
- full-file and range bodies share the same opened object semantics;
- file replacement after resolution cannot change the object served;
- Python server and CLI paths use the same conversion.

Review whether the existing file body stores:

- `tokio::fs::File`;
- `std::fs::File` converted into Tokio;
- or another owned abstraction.

Document conversion and ownership in safety comments.

## Track B — Metadata from handle

Obtain all response-critical metadata from the validated file handle:

- file type;
- file length;
- last-modified time;
- stable identity inputs used for ETag;
- attributes needed for policy decisions.

Do not stat the original or reconstructed pathname.

Define behavior when metadata changes after the handle is opened:

- response planning uses one coherent metadata snapshot;
- range bounds use that snapshot;
- short reads due to concurrent truncation terminate safely;
- growth does not cause bytes beyond the planned representation to be sent unless the existing body contract explicitly permits it;
- ETag construction is documented and deterministic.

Add tests for concurrent replacement, truncation, and growth.

## Track C — Range and conditional response behavior

Run the existing canonical planner against handle-derived metadata.

Verify Windows parity for:

- full GET;
- HEAD;
- `If-Modified-Since`;
- `If-Unmodified-Since` if supported;
- `If-None-Match`;
- `If-Match` if supported;
- `Range`;
- `If-Range`;
- unsatisfiable range;
- zero-length file;
- large file;
- file replacement after planning;
- cancellation during a range body.

Raw-wire outputs should match the platform-independent contract except for intentionally platform-derived timestamp precision that must be normalized.

## Track D — Handle-relative index lookup

When a validated directory is requested:

- probe configured index names relative to that directory handle;
- validate index component names using the same component policy;
- open without following reparse points;
- reject reparse index candidates;
- require regular-file type;
- retain the opened index file;
- never join the directory pathname with an index name;
- preserve index precedence order;
- distinguish missing candidate from denied/error candidate according to policy.

Tests must include:

- ordinary index file;
- multiple configured index names;
- index symlink;
- index junction/other reparse object;
- index replaced during probe;
- directory replaced at original pathname;
- dotfile index when dotfiles are denied;
- missing and inaccessible index.

## Track E — Handle-based directory enumeration

Directory listing remains opt-in. When enabled, enumerate from the validated directory handle.

Requirements:

- no absolute directory reopen;
- bounded enumeration;
- deterministic sorting policy;
- names decoded/encoded safely for HTML and URLs;
- no local absolute path leakage;
- dotfiles hidden unless explicitly allowed;
- reparse entries hidden or rejected under hardened mode;
- unsupported object types omitted or represented conservatively;
- cancellation stops enumeration;
- listing generation has entry and byte bounds compatible with Plan 067;
- directory mutation cannot redirect entry inspection outside the root.

If entry metadata requires a child open, perform it relative to the validated directory handle and apply the same no-reparse policy.

Do not infer safety from enumeration attributes alone where a child is later opened.

## Track F — MIME and display-name handling

MIME lookup may use the safe validated relative display name or final component extension; it must not require reopening or canonicalization.

Verify:

- case-insensitive extension handling where intended;
- unknown extension fallback;
- trailing-dot/space names are rejected before this layer;
- directory-listing HTML escapes names;
- URL encoding is single-pass and reversible under the accepted name policy;
- no response header is constructed from unsanitized filesystem text.

## Track G — Static-service integration

Ensure all Windows static paths use hardened operations:

- CLI static server;
- Rust `StaticService`;
- Rust `SecureRoot` planning;
- Python `ServerSecureRoot`;
- Python native `SecureRoot`;
- installed wheel subprocess binary.

Add an assertion or test-only instrumentation proving no fallback pathname-open function is called under a hardened NTFS profile.

If the functional fallback remains, it must be separately selectable and separately tested.

## Track H — Error and connection behavior

Map failures consistently:

- missing file: 404;
- denied reparse/dotfile/policy: 403;
- malformed target: 400;
- unsupported root/profile: startup/configuration error or explicit functional classification;
- file changed/truncated during response: terminate response/connection safely according to current body semantics;
- metadata/read internal failure: sanitized 500 if response not started, otherwise close;
- file-stream admission exhaustion: bounded 503 behavior as currently defined.

Never include NT paths, device prefixes, volume names, or configured absolute roots in client responses.

## Required tests

### File-serving parity

- GET and HEAD across representative MIME types;
- empty, small, and large files;
- full and partial ranges;
- conditional requests;
- concurrent readers;
- stream cancellation;
- write timeout;
- replacement/truncation/growth during response;
- handle count returns to baseline.

### Index tests

- ordinary index;
- precedence;
- reparse candidate denial;
- index replacement races;
- directory pathname replacement;
- dotfile policy.

### Listing tests

- listing disabled by default;
- ordinary bounded listing;
- dotfile filtering;
- reparse filtering;
- HTML and URL escaping;
- mutation during enumeration;
- entry and byte limits;
- cancellation;
- no absolute path leakage.

### Cross-language and packaging

- Rust primitive planning;
- Python native planning;
- Python live server;
- subprocess server;
- installed wheel on Windows;
- standalone binary on Windows.

## Performance and resource checks

This phase does not optimize for maximum throughput, but it must detect pathological regressions:

- no handle opened per body chunk;
- no pathname canonicalization per body chunk;
- no unbounded directory metadata opens;
- no leaked handles on 404/403/error paths;
- no entire-file buffering;
- range responses seek and stream rather than copy whole files.

Add microbenchmarks only where they protect an architectural invariant. Do not create performance targets that encourage weakening confinement.

## Release criteria

Add candidate gates for:

- Windows handle-based full-file serving;
- Windows range/conditional parity;
- handle-relative index lookup;
- handle-based listing policy;
- no-path-reopen instrumentation;
- installed wheel and binary tests;
- handle leak stress.

Invalidate on changes to:

- Windows platform filesystem code;
- response body/file streaming;
- planner metadata/ETag behavior;
- index/listing code;
- Python Windows bindings or packaging.

## Documentation

Update:

- Windows filesystem architecture;
- secure-root contract;
- directory-listing policy;
- deployment support matrix;
- known functional-only filesystem list;
- file mutation semantics;
- ETag/metadata platform normalization where relevant.

## Acceptance criteria

- Windows static responses use validated handles end to end.
- File metadata, range planning, and streaming do not reopen paths.
- Index lookup is relative to the validated directory handle.
- Directory listing is handle-based, policy-aware, bounded, and opt-in.
- Reparse objects are never served or traversed in the hardened profile.
- CLI, Rust, Python, wheel, and standalone binary paths share the same behavior.
- Error responses are sanitized and resource cleanup is complete.

## Stop conditions

Stop before Plan 065 if:

- any response path still reopens by absolute or reconstructed pathname;
- directory enumeration cannot be made handle-relative;
- range streaming requires loss of validated handle identity;
- a fallback silently replaces hardened behavior;
- resource cleanup cannot be demonstrated under cancellation.

## Handoff

Plan 065 owns adversarial Windows qualification, filesystem support classification, and promotion gates. This phase must leave Windows as candidate until those gates pass on real Windows environments.
