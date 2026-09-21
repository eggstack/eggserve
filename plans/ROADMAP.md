# eggserve roadmap

## Purpose

eggserve is a hardened, auditable, Rust-backed replacement for the common `python -m http.server` use case and a reusable set of safe HTTP/static-serving primitives. Static serving remains the primary end-user product. EggServe is not itself an application server, ASGI/WSGI runtime, reverse proxy, framework, CDN, or Granian-style general server; its Rust core also exposes a hardened, transport-owning HTTP runtime and canonical service boundary that separate downstream application-server projects may embed. Its core value is a small, predictable, security-oriented substrate that gives Python users standard-library-like ergonomics with production-grade defaults.

The initial public surface should look familiar:

```bash
python -m eggserve
python -m eggserve 8000
python -m eggserve --directory public
python -m eggserve --bind 127.0.0.1 --port 8000
python -m eggserve --directory public --public
```

The long-term public surface should also expose conservative Python primitives:

```python
from eggserve import serve_directory, ServeConfig, StaticPolicy

serve_directory(
    "public",
    bind="127.0.0.1",
    port=8000,
    policy=StaticPolicy.safe_default(),
)
```

The Python compatibility API should remain narrow. Framework and application
semantics—routing, middleware ecosystems, templating, sessions, reverse
proxying, and application lifecycle—remain outside EggServe. The Rust core may
provide generic HTTP request/response streaming, lifecycle and cancellation
primitives, and service embedding; those capabilities are substrate support,
not an application-server implementation. A separate downstream project owns
ASGI/WSGI event models, Python event-loop integration, worker/process
management, framework loading, lifespan, and application concurrency policy.

## Product principles

1. Safety over exact `http.server` compatibility. Compatibility should be ergonomic and operational, not behavioral. Unsafe standard-library behaviors must not be preserved by default.
2. Explicit policy. Filesystem, path, symlink, dotfile, directory listing, MIME, caching, logging, and bind-address behavior should be visible and configurable through typed policy structures.
3. Controlled protocol scope. HTTP/1.1 remains the minimal/default compatibility baseline. Optional HTTP/2 and HTTP/3 runtime support is governed by Plans 183–190 for scope, implementation, deterministic qualification, and corrective closure; Plans 191–193 may promote those existing transports only through explicit independent-client, adversarial, dependency, platform, and release-evidence gates. None of those plans authorizes unrelated edge-server or framework features. Static/default services reject request bodies by default, while custom Rust services may opt into bounded request-body streaming through the experimental runtime seam.
4. Small dependency graph. Hyper is the HTTP/1/2 substrate. Avoid `reqwest`, full web frameworks, reverse-proxy stacks, templating engines, and broad middleware systems unless a specific milestone justifies them. HTTP/3/QUIC dependencies remain optional and isolated from the minimal build.
5. Auditable implementation. Security-critical behavior should live in small, independently tested modules with fuzz targets and regression corpora.
6. Stable foundation before features. Range requests, TLS, CORS, custom directory rendering, Python APIs, Rust library stabilization, and additional protocols should follow only after the path confinement and resource-limit model is proven.

## Architectural target

The repo converges on this layered workspace (Plan 226 corrective: the
earlier three-crate sketch below is superseded and must not be treated as
a target):

```text
crates/
  eggserve-primitives/  # canonical transport-neutral HTTP/security values
  eggserve-server/      # generic H1/runtime/service/tunnel authority
  eggserve-static/      # static service + filesystem confinement authority
  eggnet-tls/           # neutral TLS identity/trust/client-auth substrate
  eggserve-h3/          # optional experimental H3/QUIC adapter
  eggserve-core/        # compatibility/composition umbrella
  eggserve-bin/         # CLI
  eggserve-python/      # excluded PyO3 wheel crate
```

Superseded early sketch (retained for history only — not a target):

```text
crates/
  eggserve-core/       # policy, path confinement, static serving, canonical HTTP, runtime/service boundary
  eggserve-bin/        # Rust CLI binary
  eggserve-python/     # Python wheel packaging and python -m launcher
fuzz/
  fuzz_targets/
    path_target.rs
    percent_decode.rs
    request_target.rs
plans/
docs/
tests/
```

## Current downstream-substrate position

Plans 161 and 172–175 explicitly extended the reusable Rust boundary after the
original static-serving milestones. The qualified capability is a hardened
HTTP transport/runtime substrate plus the experimental generic tunnel handoff:
separate projects may build application servers against the public canonical
primitives and experimental `server` APIs, with bounded downstream coordination
and application-task admission owned there. Plan 199 implements the generic
tunnel successor to deferred Plan 176 (one-shot `TunnelCapability` +
bounded `TunnelIo`; denial stays ordinary HTTP; WebSocket framing stays
downstream).
This does not make EggServe an application server or promote experimental
runtime types to the stable 1.0 API. Plan 205 (application observability
hooks) is explicitly deferred by Plan 208: the Plan 181 per-runtime
`OpsContext` remains the observability boundary, and no `RequestObserver` /
request-ID / lifecycle-event / timing extension is promised.

The core crate should have no Python awareness. The binary should be a thin consumer of the core crate. The Python package should initially be a very thin launcher for the Rust binary, not a premature extension API. Once the core is stable, expose a Python API as a narrow wrapper around typed Rust configuration.

## Dependency/security authority convergence — Plans 217–225

Plans 217–225 are the post-216 ownership and dependency-hardening program. They do not broaden EggServe's product scope or promote protocol support tiers.

- **217 — direct service/request convergence:** finish the type/service-shape convergence left open by Plan 216, including compatibility H2 dispatch through the canonical direct service contract.
- **218 — supply-chain security remediation:** raise the rustls security floor, refresh both distributed lockfiles, and add scheduled advisory scanning independent of pushes/PRs.
- **219 — static/confinement authority collapse:** remove the duplicate path/filesystem/static implementation from `eggserve-core`; `eggserve-static` becomes the single static/confinement authority.
- **220 — HTTP/3 adapter extraction:** make `eggserve-h3` own the actual H3/QUIC adapter instead of only the coordinated dependency set; H3 remains experimental.
- **221 — first-party frontend migration:** move the Rust binary and Python extension onto the canonical leaf crates and reduce duplicated Python runtime validation.
- **222 — cross-repo server TLS consolidation:** use neutral `eggnet-tls` for common eggserve/eggress server TLS identity/trust/client-auth behavior; keep eggfetch client policy local.
- **223 — cross-repo HTTP CONNECT consolidation:** share only the neutral outbound H1 CONNECT wire primitive between eggfetch/eggress; eggserve does not acquire an eggfetch/eggress dependency.
- **224 — capability-filesystem crate evaluation:** post-219 GO/NO-GO gate for isolating platform capability/FFI code; closed NO-GO, no crate created (`release/plan-224-capability-filesystem-evaluation.md`).
- **225 — compatibility-core facade closure:** final proof that `eggserve-core` is a compatibility facade/adapter layer, not a security/protocol implementation authority. Closed: orphaned `primitives/canonical/` copy deleted, leftover `phf` dependency removed, module inventory + facade discipline enforced by the topology gate (see `release/plan-225-compatibility-facade-closure.md`).

The sequencing/index is `plans/217-225-dependency-security-architecture-program.md`. Plan 218 is immediate and parallel-safe; 217/219 establish canonical ownership; 220/221 consume that ownership; 222–224 are cross-repo/evaluation follow-ons; 225 is the closure gate.

## Post-225 release-readiness corrective — Plan 226

**Plan 226 — post-225 release-readiness and metadata corrective** is the narrow follow-up to the completed 217–225 authority-convergence campaign. It does not reopen crate ownership or add features. It aligns the repository with the release state already documented on `main`:

- formalize `eggserve-core` as the compatibility/composition umbrella rather than an implementation authority;
- update stale package descriptions and the obsolete early three-crate architectural target;
- raise the workspace MSRV from Rust 1.88 to the current eggstack Rust 1.89 baseline while keeping the exact release compiler pin separate;
- execute the required pre-1.0 `0.2.0` version transition rather than publishing the breaking `main` line as another `0.1.x` patch;
- require an actual remote GitHub Actions/check result for the exact closing SHA in addition to local validation.

Implementation plan: `plans/226-post-225-release-readiness-corrective.md`.

## Performance hot-path optimization campaign — Plans 227–231

**Plans 227–231** are the post-226 evidence-led performance campaign. They do
not reopen the crate-ownership work and do not authorize new product features.

```text
227  current-HEAD baseline + native-client/body profiling
 |\
 |  228  H1 response-activity + dispatch-state simplification
 |  229  static/file streaming + metadata allocation optimization
 |  230  Date/request-metadata/lazy-observability cleanup
 |/
231  same-machine A/B qualification, keep/revert decisions, closure
```

Plan 227 is mandatory before implementation because the latest comprehensive
performance capture is Plan 170 and predates the direct-crate restructuring.
Plans 228–230 may proceed independently only after their motivating cost is
confirmed or mechanically proven by Plan 227. Plan 231 is the final gate: it
retains only reproducible improvements or clear simplifications with no
correctness/resource/security regression.

The campaign explicitly preserves confinement, canonical framing, transport
write-stall semantics, admission limits, crate topology, and H2/H3 support
tiers. It does not authorize sendfile/splice/io_uring, mmap caches, custom
allocators, buffer pools, or a public Service API redesign.

Program index: `plans/227-231-performance-hotpath-optimization-program.md`.

Implementation plans:
- `plans/227-current-head-performance-baseline-and-profiling.md`
- `plans/228-h1-activity-and-dispatch-state-optimization.md`
- `plans/229-static-file-streaming-optimization.md`
- `plans/230-response-metadata-observability-hotpath-cleanup.md`
- `plans/231-performance-optimization-qualification-closure.md`

Status: COMPLETE. Plan 227 baseline artifacts, Plans 228–230 scoped
optimizations, Plan 231 qualification/closure artifacts, the Plan 232
corrective read-bound/evidence closure, and the Plan 233 per-trial
provenance polish are recorded under
`benchmarks/227-current-head/`, `benchmarks/231-optimization-closure/`,
`benchmarks/232-corrective/`, and `benchmarks/233-evidence-polish/`.

### Post-231 file-stream/evidence corrective — Plan 232

**Plan 232 — file-stream read-bound and performance-evidence corrective** is a
narrow follow-up to the completed optimization campaign. Review found that the
new `BytesMut` file-read helper incorrectly uses allocator-visible
`capacity()` as the logical read target even though
`BytesMut::with_capacity(n)` only guarantees capacity of at least `n`.
Plan 232 makes the representation/range `chunk_len` explicit, adds a forced
over-capacity regression, and proves full/range bodies cannot consume bytes
past their advertised boundary.

The plan also closes the remaining performance-evidence gap: compare 64 KiB
versus 128 KiB file chunks over the live native H1 harness using throughput,
tail latency, and RSS/resource evidence, select the final default from that
tradeoff, and capture the representative TLS established-connection and
handshake-churn measurements omitted by the Plan 231 `--skip-tls` run. It
does not reopen general optimization work or authorize sendfile, io_uring,
buffer pools, unsafe buffer manipulation, public API changes, or support-tier
changes.

Implementation plan:
`plans/232-file-stream-read-bound-and-performance-evidence-corrective.md`.

### Evidence provenance and closure polish — Plan 233

**Plan 233 — performance evidence provenance and closure polish** is an
evidence-only follow-up. It does not authorize runtime/default/API changes.
It records the successful CI run for the actual Plan 232 closure SHA, replaces
discarded `/tmp`-only benchmark provenance with compact retained per-trial
JSON, and fills the missing per-range throughput/latency/RSS fields.

The plan re-runs the 64 KiB vs 128 KiB native matrix, range probes, and
representative TLS cases only to make the existing Plan 232 conclusions
independently auditable from tracked repository evidence. If reproduction
materially contradicts the 128 KiB decision, Plan 233 must stop and open a
separate production corrective rather than changing code itself.

Implementation plan:
`plans/233-performance-evidence-provenance-and-closure-polish.md`.

Status: COMPLETE. Per-trial native/range/TLS evidence, the closure-SHA CI
record, and the mechanically derived aggregate are retained under
`benchmarks/233-evidence-polish/`; no production default changed and the
128 KiB decision stands.

## Fixed-cost performance optimization campaign — Plans 234–240

**Plans 234–240** are the post-233 follow-on performance campaign. The prior
227–233 work is closed; this program targets the smaller fixed costs left
behind rather than reopening file-stream chunk sizing or broad runtime design.

```text
234  current-HEAD fixed-cost allocation/syscall/resource baseline
 |\
 |  235  request-target/body common-path allocation cleanup
 |  236  static resolver root-FD + path fixed-cost cleanup
 |  237  H1 dispatch + connection metadata cleanup
 |  238  request-scoped shared-state consolidation (evidence-gated)
 |  239  Python request allocation + stream resource qualification
 |/
240  same-machine A/B qualification, keep/revert/defer closure
```

Plan 234 is mandatory before production changes. Plans 235–237 may proceed
independently after their targets are confirmed. Plan 238 follows Plan 235
because both touch request-scoped primitive allocation. Plan 239 is a separate
frontend track because Python object/GIL/thread costs have different
qualification requirements. Plan 240 is the final closure gate.

The campaign freezes the public Rust/Python surface and preserves confinement,
framing, lifecycle, timeout, admission, crate-topology, and protocol-tier
semantics. It does not authorize caches, sendfile/splice/io_uring, mmap,
custom allocators, global pools, a new executor, or a public Service redesign.

Program index:
`plans/234-240-fixed-cost-performance-optimization-program.md`.

Implementation plans:
- `plans/234-current-head-fixed-cost-baseline-and-profiling.md`
- `plans/235-request-target-and-body-common-path-allocation-optimization.md`
- `plans/236-static-resolver-and-path-fixed-cost-optimization.md`
- `plans/237-h1-dispatch-and-connection-metadata-optimization.md`
- `plans/238-request-scoped-shared-state-allocation-consolidation.md`
- `plans/239-python-bridge-allocation-and-stream-resource-optimization.md`
- `plans/240-fixed-cost-performance-qualification-closure.md`

Status: IMPLEMENTATION COMPLETE; EVIDENCE CLOSURE COMPLETED BY PLAN 241. Plan 234
records the implementation-start baseline under
`benchmarks/234-fixed-cost-baseline/`. Plans 235–237 and the Python
request-view portion of Plan 239 were retained; Plan 238 is NO-GO, and the
dedicated Python stream-producer redesign is DEFER. Plan 240 records the
initial same-machine static closure under
`benchmarks/240-fixed-cost-closure/`, but its retained matrix was narrower
than its written acceptance criteria. Plan 241 completed the evidence-only
corrective for custom/path-specific H1, TLS, installed-wheel Python callback,
slow-stream resource, Unix syscall, and exact-SHA CI provenance closure under
`benchmarks/241-fixed-cost-evidence-corrective/`. No production behavior was
changed; metadata sharing and the producer redesign remain DEFER, and Plan 238
remains NO-GO.

### Fixed-cost evidence/closure corrective — Plan 241

**Plan 241 — fixed-cost performance evidence and closure corrective** closes
the evidence gaps discovered after Plan 240. It does not reopen production
optimization work. It completes the missing same-machine baseline/candidate
matrix for custom H1, HEAD/304/range/path variants, established TLS, installed
Python callback views, and 10/100/N slow synchronous streams; retains direct
before/after Unix resolver syscall proof; records the already-successful CI run
`35538302042` for closing SHA
`5b048cbf66f57957625c9ad8b658635a56ac9593`; and requires remote CI for the
new evidence-content SHA.

If any new measurement materially contradicts a retained optimization, Plan
241 must stop and open a separate production corrective rather than changing
runtime code itself.

Implementation plan:
`plans/241-fixed-cost-performance-evidence-and-closure-corrective.md`.

Status: COMPLETE. Evidence-content SHA `9592d9b34d2d48ea8100537cc9cf8ea73ca19a96`
passed remote CI run `35543869576`; the final metadata-record commit is the
documentation commit containing this status update.

## API-preserving maintainability and authority convergence — Plans 242–248

**Plans 242–248** are the post-241 maintenance/convergence campaign. They do
not add product features or authorize public Rust/Python API regression. The
program follows a current-HEAD audit that found one direct-runtime shutdown
correctness defect plus remaining implementation duplication behind the
otherwise successful Plans 217–225 authority split.

```text
243  direct Server shutdown/lifecycle correctness
 |
244  H1 runtime authority convergence
 |\
 | 245 static-service authority convergence
 | 246 Python interop typing/internal maintainability
 | 247 leaf-crate surface, orphan-source, and qualification cleanup
 |/
248  API/capability-preserving qualification and closure
```

The campaign preserves every existing compatibility path and capability.
`eggserve-core` remains the compatibility/composition umbrella;
`eggserve-server` becomes the actual single H1 implementation authority for
shared connection behavior; `eggserve-static` becomes the single static
service implementation authority in addition to its existing
path/filesystem/planner ownership. The Python work is typing/internal
maintainability only and must not alter runtime semantics.

Plan 243 is immediate because the direct high-level server currently uses
`Notify::notify_waiters()` as the server shutdown signal while accepted
connection tasks are detached; the corrective makes shutdown durable and
makes `wait()` account for accepted runtime-owned tasks without changing
public signatures.

Plan 247 also removes the orphaned, uncompiled
`eggserve-primitives/src/primitives/runtime_limits.rs`, adds an orphan-source
gate, reconciles feature declarations with real direct-crate capabilities
without removing accepted feature names, and moves authority qualification
into the owning leaf crates while retaining compatibility/cross-protocol tests
in core.

Program index:
`plans/242-248-api-preserving-maintainability-convergence-program.md`.

Implementation plans:
- `plans/243-direct-server-shutdown-lifecycle-corrective.md`
- `plans/244-h1-runtime-authority-convergence.md`
- `plans/245-static-service-authority-convergence.md`
- `plans/246-python-interop-typing-maintainability.md`
- `plans/247-leaf-surface-orphan-source-qualification-cleanup.md`
- `plans/248-api-preserving-maintainability-closure.md`

Status: COMPLETE AS EXECUTED; POST-CLOSURE CORRECTIVE CLOSED BY PLANS 249–250
(`release/plan-250-h1-authority-lifetime-corrective-closure.md`, candidate
`e38d12d7d177888e7fc38fea42cc51f5a0ee5169`, remote CI run `35618901331`).
Baseline for the audit and handoff:
`673b6c60dab09d728b05d9e979be91bfc5417050`. Final candidate
`3fb59e4560b74407b7faed3a09aaae5974d3d36a` passed remote CI run
`35602644725`; the final metadata-record commit is the documentation commit
containing this status update. Post-closure source review at
`4b2af07991d20234d5167d08311ba6b18006025a` found that Auto-selected H1
could still execute the compatibility core's private Hyper H1 pipeline and
that accepted compatibility connections retained detached shutdown-forwarder
tasks until whole-server shutdown. Plans 249–250 supersede the H1-authority
and connection-lifetime closure claims only; the other Plan 243–248 results
remain closed.

## Post-248 H1 authority and connection-lifetime corrective — Plans 249–250

**Plans 249–250** are the narrow post-closure corrective for two residual
issues discovered after the Plans 242–248 implementation was marked complete.
They do not reopen the static/Python/orphan-source work and do not add
capability.

```text
249  eliminate Auto -> core-H1 execution + detached shutdown forwarder
 |
250  normal-accept-path/resource/API/full-CI corrective closure
```

Plan 249 resolves cleartext protocol selection before constructing the core
Hyper service. Auto-selected H1, explicit H1, TLS ALPN H1, PROXY-prefixed H1,
and Unix H1 must all enter the single `eggserve-server` H1 authority; core
retains only H2-specific execution and protocol-selection/composition glue.
The same plan replaces the detached per-connection broadcast forwarder with a
connection-scoped structured shutdown future so completed connections cannot
leave sleeping tasks/receivers until whole-server shutdown.

Plan 249 also strengthens the topology gate to reject a core HTTP/1 Hyper
builder/connection driver and detached shutdown-forwarder spawning rather than
merely checking that some direct delegation call exists.

Plan 250 re-runs normal compatibility accept-path H1/H2/TLS/PROXY/Unix
qualification, deterministically proves shutdown-forwarder lifetime cleanup,
re-checks API/feature compatibility, runs the full repository/package/wheel/
supply-chain matrix, and records exact-SHA remote CI before reconciling the
Plan 244/248 historical closure notes.

Implementation plans:
- `plans/249-core-auto-h1-authority-and-shutdown-forwarder-corrective.md`
- `plans/250-post-248-h1-authority-lifetime-corrective-closure.md`

Status: COMPLETE. Corrective baseline:
`4b2af07991d20234d5167d08311ba6b18006025a`. Implementation/evidence
candidate `e38d12d7d177888e7fc38fea42cc51f5a0ee5169` passed exact-SHA remote
CI run `35618901331` (rust / supply-chain / python all success, 2026-09-21);
evidence record
`release/plan-250-h1-authority-lifetime-corrective-closure.md`.

## Post-convergence maintenance and interop fidelity — Plans 251–256

**Plans 251–256** are the post-250 API-preserving maintenance campaign from
the current-tree review at
`0ee02acd69f1c63d32134f8265283fff04e4630c` (normal CI run
`35620987177` green). The campaign does not add product features, change
protocol support tiers, or authorize Rust/Python public API regression.

The review found four remaining maintenance classes after the successful
single-H1/static authority convergence:

1. concrete drift in shipped Python stubs, including low-level async request
   header/address/proxy property types and missing supported `http.server`
   subclass hooks;
2. substantial source-level overlap between core's H2/multiprotocol connection
   machinery and the direct H1 server even though executable H1 authority is
   now correctly single-owned by `eggserve-server`;
3. the Python `AsyncServer` bridge has strong bounds but independently
   orchestrates asyncio admission, timeout, streaming, cancellation, and task
   lifetime and therefore needs stronger deterministic parity evidence;
4. post-extraction import/comment/module residue remains, while
   `scripts/check-crate-topology.py` has become a large executable
   architecture specification that should be internally easier to maintain
   without weakening any rule.

```text
252  Python typing/public-surface fidelity corrective
 |
253  core/server connection-overlap classification + safe convergence
 |\
 | 254 async-Python lifecycle/stream parity hardening
 | 255 migration-residue + topology-checker maintainability cleanup
 |/
256  API/capability-preserving qualification and closure
```

Plan 253 is intentionally conservative: overlap must be classified at the
symbol/responsibility level, but deduplication is DEFER when the only clean
route would expose new public Hyper/Tokio internals, activate direct H2/TLS
capability, move H2 ownership, or create a new crate solely for source sharing.
The target is bounded drift risk rather than maximum line deletion.

Plan 255 also treats large-file decomposition as evidence-led: security-local
filesystem modules are not split merely for size, and CLI/H3/Python modules are
decomposed only where a natural private responsibility boundary improves
reviewability without changing behavior.

Program index:
`plans/251-256-post-convergence-maintenance-interop-fidelity-program.md`.

Implementation plans:

- `plans/252-python-typing-public-surface-fidelity-corrective.md`
- `plans/253-core-server-connection-overlap-safe-convergence.md`
- `plans/254-async-python-lifecycle-streaming-parity-hardening.md`
- `plans/255-migration-residue-module-topology-maintainability-cleanup.md`
- `plans/256-post-convergence-maintenance-interop-fidelity-closure.md`

Status: **COMPLETE**. Landing SHAs `ee1724b` (252) / `a27c330` (253) /
`0745635` (254) / `e359cf1` (255); evidence record
`release/plan-256-post-convergence-maintenance-interop-closure.md`.
Planning/audit baseline:
`0ee02acd69f1c63d32134f8265283fff04e4630c`. Plans 252–255 preserve the
existing capability/API surface. Closure candidate `4c14542` passed
exact-SHA remote CI run `35653232800` (rust / supply-chain / python all
success, 2026-09-21); the final metadata-record commit is the
documentation commit containing this status update.

## Post-256 async suppressed-body lifetime corrective — Plans 257–258

**Plans 257–258** are a narrow post-closure corrective for one resource-lifetime
defect discovered after Plans 251–256 were marked complete. They do not reopen
the Python typing, Rust authority, topology, import-cleanup, or broader
async-parity results from that campaign.

Current corrective baseline:
`cd6061a97f6538d013f0fac2adc1653a96097dd0`.

Plan 254 correctly changed async streamed responses so the application iterable
is not advanced until the native response body is first pulled. That fixed
HEAD/body-forbidden application-state consumption, but the first-pull wait is
owned by a producer task that also owns the `AsyncServer` application permit.
For HEAD/204-style suppression, Rust correctly drops the Python iterable without
polling it. The current cancellation path lives inside the synchronous Python
generator's `finally`, and a generator that has never been entered does not
execute that `finally` when closed/dropped. The producer can therefore remain
parked until `response_write_timeout_secs` and retain its async permit.

With a small `max_async_tasks` bound, repeated suppressed stream responses can
temporarily consume all permits and make an otherwise-valid next request fail
fast with 503 even though no application stream work is active.

```text
257  explicit suppressed-stream lifetime / permit corrective
 |
258  focused resource-lifetime + full-wheel + exact-SHA CI closure
```

Plan 257 requires a deterministic `max_async_tasks=1` reproducer before the
fix and an explicit lifetime owner/drop path that works even when the stream
iterator is never entered. It must not duplicate the canonical HTTP
body-suppression status table in Python; canonical Rust remains the authority
for whether a body is consumed.

Plan 258 proves immediate permit reuse for HEAD/body-forbidden streams,
repeated suppressed-response task closure, exactly-once cleanup across
drop/error/timeout/shutdown races, ordinary stream non-regression, public API
preservation, installed-wheel qualification, and exact-SHA remote CI.

Implementation plans:

- `plans/257-async-python-suppressed-body-permit-lifetime-corrective.md`
- `plans/258-post-256-async-suppressed-body-lifetime-corrective-closure.md`

Status: **COMPLETE**. Implementation/evidence candidate
`c22a2d20dc5dec31da17f8cc1b97c0378b022c88` passed exact-SHA remote CI run
`35658950539` (rust / supply-chain / python all success, 2026-09-21);
evidence record
`release/plan-258-async-suppressed-body-lifetime-corrective-closure.md`.

Plans 251–256 remain historically complete on candidate
`4c145421c851fffa5e1f6762a7ef742c5db1e5d8` / CI run `35653232800`;
Plans 257–258 supersede only the async suppressed-body permit/task-lifetime
closure claim. The other Plan 256 results remain closed unless new evidence
shows otherwise.

## Protocol expansion, corrective closure, and support promotion — Plans 183–194

Plan 183's product/scope gate has been implemented and the live product contract in `docs/non-goals.md` now authorizes only the narrow native H2/H3 transport work described by this program. Plans 184–188 implemented and qualified the first protocol adapters, leaving H2 and H3 experimental. Plans 189–190 closed deterministic semantic gaps discovered by the post-188 review without changing those support tiers. Plans 191–193 are evidence-led promotion gates: they may promote the already-implemented protocol transports, but they do not add another protocol family or broaden the product surface. Plan 194 is a narrow H3 producer-timeout + promotion-trace correction with no promotion authority. Plan 213 isolates the direct H3/QUIC dependency set in `eggserve-h3` and records a dedicated qualification inventory without changing the experimental tier.

```text
183  HTTP/2 and HTTP/3 protocol expansion roadmap / product gate
 |
184  Protocol-neutral runtime preparation and overlap cleanup
 |
185  HTTP/2 runtime, TLS/ALPN, multiplexing, and hardened limits
 |
186  HTTP/2 conformance, interoperability, and release closure
 |
187  HTTP/3 QUIC transport and canonical adapter
 |
188  HTTP/3 interoperability and multi-protocol release closure
 |
189  Multiprotocol request-body, error, and lifecycle correctness
 |
190  Multiprotocol corrective qualification and release closure
 |
191  HTTP/2 supported-tier promotion qualification

  192  HTTP/3 dependency readiness and conformance hardening
  |
  193  HTTP/3 supported-tier promotion qualification
  |
 194  HTTP/3 response-producer timeout and promotion-trace correction
  |
  213  HTTP/3 and QUIC isolation, qualification, and promotion gates
```

Plan 183 updates the product/non-goal and pre-1.0 API contract. Plan 184 removes the remaining duplicated request-target parser, repeated service-invocation logic, HTTP/1-specific lifecycle decisions in shared code, lossy version conversion, latent upgradeable-connection machinery, and ambiguous protocol-specific configuration ownership while proving HTTP/1 behavior unchanged.

Plan 185 adds HTTP/2 through the existing Hyper/Hyper-Util family, with explicit H2 stream/header/flow-control limits, TLS ALPN, stream-scoped body/error handling, stream-aware response activity, and GOAWAY/drain semantics. Plan 186 provides the initial independent-client, adversarial/resource, shutdown, platform, footprint, and documentation evidence; its executed result keeps H2 experimental because meaningful second-implementation/browser/platform evidence and per-stream reset/progress limitations remain.

Plan 187 treats HTTP/3 correctly as a separate QUIC/UDP transport implementation sharing the same canonical service layer. Its H3/Quinn dependencies remain optional/internal; it owns dual-listener lifecycle, TLS 1.3/`h3` ALPN, handshake/stream/QPACK budgets, canonical H3 adaptation, stream-specific backpressure/cancellation, GOAWAY/drain, and runtime-owned Alt-Svc. Plan 188 closed the feature at the experimental tier after deterministic checks; external H3 interoperability, network-impairment/resource evidence, and cross-platform runtime qualification remained explicit follow-up gates.

Plan 189 corrects the narrow post-188 findings: H2/H3 Reject-body handling detects DATA without relying on `Content-Length`, H3 runtime errors share the canonical representation authority, H3 provides connection/stream `RequestLifecycle` cancellation parity, and H2 response-progress wording/behavior matches what the public Hyper stack can actually observe. Plan 190 directly reproduces those bug classes, re-runs available protocol qualification, synchronizes plan/release documentation, and closes the corrective pass while retaining H2/H3 as experimental.

Plan 191 is a qualification-led HTTP/2 promotion attempt. It requires at least two independent H2 implementation families rather than two libnghttp2 frontends, at least one current browser, current-RFC conformance classification, multiplexing/reset/header/flow-control/GOAWAY/resource tests, and real Linux/macOS/Windows runtime evidence. H2 may become **supported, opt-in** without becoming default-enabled and without falsely promising a Hyper stream-local wire-progress/reset capability.

Plan 192 was the mandatory HTTP/3 dependency-readiness gate before promotion, executed 2026-09-10 with a `BLOCKED` outcome. It froze the latest released stack (`h3` 0.0.8 / `h3-quinn` 0.0.10 / Quinn 0.11.11 — no upgrade candidate), found `hyperium/h3#338` open with no released fix, fixed three `hyperium/h3#262` early-error paths while recording three residual ones, and left H3 experimental with concrete blockers (see `release/plan-192-http3-dependency-readiness.md`). Plan 193 closed at preflight the same day without entering promotion qualification: the Plan 192 prerequisite was still `BLOCKED`, so the pass inventoried the unchanged candidate, re-checked `#338`/`#262` as still open, and recorded two-family interop, browser Alt-Svc, adversarial-frame, network-impairment, and cross-platform H3 runtime evidence as unavailable (see `release/plan-193-http3-supported-tier-qualification.md`). Plan 194 (same day) bounds the H3 `ResponseStream` producer poll with an absolute `response_write_timeout` no-progress deadline (empty chunks are not progress) plus `WriteStallTimeout` observability and corrects the H2-vs-H3 timeout wording across the live docs, without changing the experimental tier or the Plan 192/193 blockers (see `release/plan-194-http3-producer-timeout-correction.md`). Plan 195 (2026-09-11) correctively qualifies that bound with reproducible evidence — stalled, progress-then-stall, slow-progress, empty-chunk, and sibling isolation plus new shutdown-race drain and write-stall observability/permit-release regressions (H3 suite 14 → 16) — with no source change and no tier change (see `release/plan-195-http3-response-timeout-corrective-qualification.md`). Plan 213 (2026-09-12) isolates the direct H3/QUIC dependency set in `eggserve-h3` and adds a dedicated qualification inventory; the compatibility adapter remains in core for 0.1 source compatibility and the experimental tier is unchanged (see `release/plan-213-http3-quic-isolation-qualification.md`). A future H3 promotion requires a new scoped plan closing those blockers; Plans 193–195 and 213 are no longer open promotion authorities.

The program explicitly does **not** authorize WebSockets, WebTransport, datagrams, CONNECT tunnels, server push, reverse proxying, ACME, DNS HTTPS/SVCB automation, routing, middleware, uploads, application workers, or in-tree ASGI/WSGI semantics. The six-class Python `http.server` compatibility facade remains HTTP/1.1-shaped unless a later separate product decision changes it.

## Default security posture

The safe default should be deliberately conservative:

```text
bind address: 127.0.0.1
methods: GET, HEAD
request bodies: rejected
HTTP version: HTTP/1.1 compatibility baseline; H2/H3 remain opt-in and may become supported only through Plans 191–193 without becoming default-enabled
directory listing: disabled unless explicitly enabled
index files: enabled for index.html by default
symlinks: denied by default
dotfiles: denied by default
unknown MIME: application/octet-stream
public bind: requires explicit opt-in or loud warning
logging: sanitized text logs by default
TLS: optional feature, not required for minimal build
```

Path handling is the critical security boundary. eggserve must not rely on a naive `canonicalize(root.join(path)).starts_with(root)` model as the final design. The path layer should be treated as an independently auditable subsystem with platform-specific behavior. Unix should move toward descriptor-relative traversal where practical. Windows should explicitly handle drive prefixes, UNC-like paths, reserved names, alternate data streams, reparse points, and separator ambiguity.

## Milestones

### M0: repository foundation and security contract

Create the repo skeleton, threat model, non-goals, dependency policy, initial architecture notes, and release criteria. This milestone establishes what eggserve is and is not. It should produce documentation that future contributors can use to reject scope creep.

Exit criteria: the repo contains docs for threat model, security policy, non-goals, dependency policy, initial architecture, and compatibility boundaries. CI can run formatting and basic checks even before full implementation.

### M1: Rust core skeleton and HTTP substrate

Create the Cargo workspace and initial crates. Add the Hyper/Tokio HTTP/1.1 accept loop, service entry point, typed configuration, error taxonomy, and basic `GET`/`HEAD` placeholders. No serious static serving should ship before the policy modules exist.

Exit criteria: `cargo test` and `cargo check --workspace` pass; a minimal server can return a static placeholder response; unsupported methods return deterministic errors; connection limits and graceful shutdown have initial scaffolding.

### M2: path confinement and filesystem policy

Implement the security-critical path pipeline: request-target handling, percent decoding, component validation, dotfile policy, symlink policy, root confinement, and platform-specific denial cases. Add unit tests, fixture tests, and fuzz targets.

Exit criteria: no accepted path can escape the configured root under the safe default policy; traversal, double-encoding, absolute-path, Windows-prefix, NUL, dotfile, and symlink regression tests exist; the path module is independently testable without starting the server.

### M3: static file serving MVP

Serve regular files using `GET` and `HEAD` with correct `Content-Length`, conservative `Content-Type`, `Last-Modified`, optional ETag support, index handling, and directory denial/listing behavior. Do not add Range or compression yet.

Exit criteria: a real directory can be served safely; `HEAD` mirrors `GET` headers without a body; directories without an index are denied unless listing is explicitly enabled; generated listing output is HTML-escaped and protected by conservative headers.

### M4: resource limits and operational hardening

Add header/request-target limits, connection concurrency limits, file-serving permits, read/write/idle timeouts, slow-client resistance, sanitized logging, and graceful shutdown behavior. Establish load and adversarial behavior tests.

Exit criteria: slowloris-style clients cannot hold resources indefinitely; high concurrency fails predictably; logs cannot be trivially injection-poisoned; large-file serving is bounded by explicit permits; all defaults are documented.

### M5: CLI parity and Python wheel launcher

Implement `eggserve` CLI and `python -m eggserve` packaging. Keep the Python layer thin at first. Provide the familiar `http.server`-like workflow while making unsafe behavior explicit.

Exit criteria: wheels build for the first supported platforms; `python -m eggserve --directory public 8000` works; CLI prints effective policy; public bind and unsafe flags are visible; package metadata and README accurately describe scope.

### M6: fuzzing, CI matrix, and security validation

Expand fuzz targets, add cargo-audit/cargo-deny/cargo-vet where appropriate, run platform CI, and add regression fixtures for path and HTTP behavior.

Exit criteria: Linux, macOS, and Windows checks pass; fuzz targets are documented; dependency policy is enforced; security regression tests are part of normal CI.

### M7: optional TLS and deployment guidance

Add optional `rustls` support under a feature flag. Document native TLS and reverse-proxy deployment patterns. Do not implement ACME in eggserve.

Exit criteria: TLS cert/key serving works when the feature is enabled; minimal builds do not pull TLS dependencies; deployment docs explain Caddy/nginx/Traefik/load-balancer fronting.

### M8: minimal Python API

Expose stable Python functions and configuration classes after the core behavior is proven. Keep the API synchronous and static-serving-oriented.

Exit criteria: Python users can call `serve_directory(...)` and configure safe policies without interacting with Rust details; API docs clearly state non-goals; no dynamic request callback API is introduced.

### M9: library stabilization and 1.0 preparation

Stabilize Rust primitives, document compatibility guarantees, finalize default policies, run a security review, and prepare crates.io/PyPI release workflows.

Exit criteria: public APIs are documented; unsafe choices are opt-in; release checklist is repeatable; project has a clear 1.0 security posture.

## Initial dependency policy

The initial dependency set should be small and justified:

```text
tokio: async runtime
hyper: HTTP/1/2 protocol substrate
hyper-util: Hyper 1.x server/runtime utilities
http-body-util: response body helpers
bytes: efficient byte buffers
percent-encoding or equivalent: path decoding, if selected after review
pico-args or minimal parser: CLI argument handling
tracing/tracing-subscriber: optional structured logging
rustls/tokio-rustls: optional TLS feature only
QUIC/H3 stack: optional only under Plans 187–194; never required by the minimal build
```

Avoid `reqwest`, Axum, Tower, Tera, Askama, libmagic bindings, compression stacks, ACME clients, database crates, and app-framework dependencies in the initial milestones.

## Release gates

An alpha can ship after M0-M5 if the docs clearly mark it as early and the unsafe areas are not exposed. A beta should require M6. A production-ready 1.0 should require M7-M9, a platform test matrix, dependency audit, fuzz corpus, and a written security review.

Optional HTTP/2/HTTP/3 support does not become part of the release promise merely because code exists. Plans 186/188 and corrective Plans 189–190 leave both transports experimental after deterministic qualification. Plan 191 executed the H2 promotion attempt and retained the experimental tier: two-family interop, h2spec classification, and flow-control/load evidence were collected, but browser evidence, macOS/Windows runtime evidence, trailer-scope determinism, and the stream-local reset hook remain open (see `release/plan-191-http2-supported-tier-qualification.md`). A future H2 promotion requires a new scoped plan closing those blockers; Plan 191 is no longer an open promotion authority. H3 additionally closed Plan 192 dependency readiness as `BLOCKED` (latest released `h3` 0.0.8 / `h3-quinn` 0.0.10 / Quinn 0.11.11; upstream `h3#338` unfixed, `#262` remainder open; see `release/plan-192-http3-dependency-readiness.md`), Plan 193 closed at preflight on 2026-09-10 without entering promotion qualification (unmet Plan 192 prerequisite; unchanged candidate; `#338`/`#262` still open; two-family, browser, adversarial, impairment, and platform evidence inventoried as unavailable; see `release/plan-193-http3-supported-tier-qualification.md`), and Plan 194 bounds the H3 producer poll with an absolute no-progress deadline (empty chunks are not progress) without changing the tier (see `release/plan-194-http3-producer-timeout-correction.md`), and Plan 195 correctively qualifies that bound (shutdown-race and observability regressions, H3 suite 14 → 16) without changing the tier (see `release/plan-195-http3-response-timeout-corrective-qualification.md`). A future H3 promotion requires a new scoped plan closing those blockers; Plans 193–195 are no longer open promotion authorities. Either protocol may remain experimental independently while HTTP/1/static serving continues to ship.

Support-tier promotion never implies default enablement. The minimal/default product remains HTTP/1.1-shaped, and the Python compatibility facade remains HTTP/1.1-shaped unless a separate future product plan explicitly changes it.

The current stable-Rust API line also contains documented pre-1.0 breaking changes and must not be published as a `0.1.x` patch release. Release preparation should use the synchronized metadata ownership established by Plan 182 and publish that line as `0.2.0` or later, with the migration guide/release notes updated in the same release change.

The 1.0 promise should remain bounded: static serving is the primary product;
stable hardened HTTP primitives and policies are the core library promise; and
the transport-owning service/runtime seam is a documented downstream embedding
path according to its stability classification. EggServe does not promise to
be an ASGI/WSGI server, framework, process manager, reverse proxy, or WebSocket
implementation.
