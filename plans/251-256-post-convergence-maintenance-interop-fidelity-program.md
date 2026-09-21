# Plans 251–256 — Post-convergence API-preserving maintenance and interop fidelity program

## Objective

Follow the completed Plans 242–250 with a narrow maintenance campaign based on
the current-tree review at:

```text
0ee02acd69f1c63d32134f8265283fff04e4630c
docs: close plans 249-250 with CI evidence
```

The baseline passed normal CI run `35620987177` on 2026-09-21.

This is not a feature campaign. It must preserve the existing Rust and Python
public API surface, capability set, security defaults, protocol support tiers,
wire semantics, and package topology unless a subplan explicitly closes with a
documented DEFER because the only clean change would violate those constraints.

The review found four actionable classes of work plus one mandatory closure
gate:

1. shipped Python type stubs have concrete signature/type drift from the
   implemented low-level and `http.server`-compatibility surfaces;
2. `eggserve-core` and `eggserve-server` still retain substantial
   connection-pipeline source overlap after H1 execution authority was
   successfully converged in Plans 249–250;
3. `eggserve.lowlevel.AsyncServer` is a correct but independently orchestrated
   Python asyncio bridge whose admission, timeout, cancellation, streaming, and
   shutdown contracts need stronger parity evidence against the Rust runtime;
4. migration residue remains in decomposed modules and the crate-topology
   checker has grown into a large executable architecture description that
   should be internally simplified without weakening any gate;
5. the resulting candidate must be requalified against the frozen public
   Rust/Python surface and exact-SHA remote CI.

## Sequence

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

Plan 252 is first because it corrects a known shipped developer-facing defect
without changing runtime behavior and strengthens the typing fixtures used by
all later work.

Plans 253–255 may proceed independently after Plan 252. Plan 256 is mandatory
and closes the campaign only after exact-SHA local and remote evidence.

## Global invariants

- No existing public Rust item may be removed, renamed, moved without an
  identity-preserving compatibility re-export, or have its signature narrowed.
- No existing public Python import path, class, method, property, constructor
  parameter, or successful runtime behavior may be removed or renamed.
- Type stubs must describe the runtime surface; runtime behavior must not be
  changed merely to match an incorrect stub.
- `eggserve-primitives` remains transport/runtime neutral.
- `eggserve-server` remains the generic H1 runtime/service authority.
- `eggserve-core` remains the compatibility/composition umbrella and the
  current owner of H2 plus extended TLS/listener/proxy composition.
- `eggserve-static` remains the sole static/path/filesystem authority.
- `eggnet-tls` remains the neutral TLS identity/trust authority.
- `eggserve-h3` remains the H3/QUIC adapter authority.
- H2 and H3 remain experimental; this campaign cannot promote either tier.
- Python remains H1-only by product contract.
- No routing, middleware, ASGI/WSGI product runtime, WebSocket framing, reverse
  proxying, worker model, ACME, or new protocol family is authorized.
- No broad production dependency may be added solely to reduce source overlap.
- No new crate may be introduced solely as an internal-sharing vehicle unless
  a separate architecture plan first proves that the package/API cost is
  justified. Plan 253 must DEFER such moves rather than smuggle them into this
  campaign.
- No public raw Hyper, Tokio transport, rustls-session, socket, or filesystem
  handle is added.
- Existing timeout, admission, cancellation, framing, proxy-trust, tunnel,
  confinement, privacy, and shutdown semantics remain fail closed.

## Plan 252 — Python typing/public-surface fidelity corrective

Correct `lowlevel.pyi` and `server.pyi` so the installed typed package
matches the existing runtime surface. Expand strict typing fixtures to exercise
actual property access and subclass hooks, not only object construction.

Known review findings include:

- `AsyncRequest.headers` is implemented as the native compatibility
  `dict[str, str]` view but is declared as `HeaderBlock`;
- `AsyncRequest.remote_addr`, `local_addr`, and `effective_addr` forward
  string-valued native `*_addr` fields while their corresponding
  `*_address` properties are the tuple-valued forms; the stub currently
  conflates them;
- `proxy_source` / `proxy_destination` are string-valued native fields but
  are typed as tuples;
- query optionality/text compatibility must be reconciled with the actual
  implementation;
- public compatibility hooks such as `BaseHTTPRequestHandler.log_request`,
  `log_error`, `log_message`, and `HTTPServer.server_bind` /
  `server_activate` are implemented but incompletely represented in the stub.

The plan must inventory all public stub/runtime properties rather than fixing
only these examples.

## Plan 253 — Remaining core/server connection-overlap classification and safe convergence

Inventory the still-parallel connection modules in `eggserve-core` and
`eggserve-server` after Plan 249, classify every duplicated responsibility,
and converge only the pieces that can be shared without changing the public
surface, capability graph, or protocol ownership.

This is explicitly not authorization to move H2 into `eggserve-server`, make
its inert `http2` compatibility feature active, expose private transport
internals publicly, or create a new shared runtime crate.

A valid Plan 253 result may be a mixture of:

- deletion of residual H1-only/dead compatibility machinery;
- thin delegation/re-export where type identity permits it;
- shared private helpers within an existing authority when no new public
  boundary is required;
- documented intentional duplication for H2 compatibility where cross-crate
  sharing would require a worse API/package design;
- stronger parity/topology gates that make accepted duplication mechanically
  bounded.

The objective is lower drift risk, not maximum line deletion.

## Plan 254 — Async-Python lifecycle and streaming parity hardening

Strengthen deterministic evidence around the Python asyncio bridge without
redesigning it or adding a new native async API.

Cover application-task admission, pre/post-response permit ownership, handler
timeouts, request cancellation, response-producer backpressure/no-progress
timeouts, disconnect races, producer failure after commitment, tunnel/SSE task
tracking, and shutdown cancellation.

The bridge may receive internal bug fixes if tests reproduce a real defect, but
public async classes/signatures and the H1-only product boundary remain fixed.

## Plan 255 — Migration residue and topology-checker maintainability cleanup

Remove post-extraction residue such as broad `allow(unused_imports)` and
copied binding import preambles where they are no longer needed. Reduce
historical plan-number narration in production code where an invariant-focused
comment can replace it without losing rationale.

Refactor `scripts/check-crate-topology.py` internally so dependency rules,
module inventory, authority rules, and Python/package rules are easier to
review and test. Every existing rejection must remain effective.

Also tighten the current Rust-consumption documentation so direct H1 consumers
and richer compatibility/multiprotocol consumers have explicit, truthful
entry-point guidance. This is documentation clarification, not an API
promotion.

## Plan 256 — Qualification and closure

Freeze the final candidate and prove:

- Python runtime/stub fidelity through a built wheel and strict static typing;
- no Rust/Python public API regression;
- H1 remains single-authority;
- no accidental H2/H3/TLS/proxy/static capability movement occurred;
- async-Python lifecycle/stream behavior remains bounded;
- topology checks are at least as strict as baseline;
- package/dependency graphs remain within policy;
- full routine Rust, Python, supply-chain, and package qualification is green;
- exact candidate SHA passes normal remote CI.

Create a release evidence record and distinguish the CI-verified candidate SHA
from any later metadata-only documentation commit.

## Compatibility method

For every implementation plan:

1. inventory the public paths and behavior it touches before changing code;
2. add a regression/typing fixture that fails on the baseline defect or drift
   class when practical;
3. make the smallest internal change that resolves the finding;
4. preserve existing public paths and type identity;
5. run direct-leaf and compatibility-path tests together;
6. update topology/documentation only after behavior is proven;
7. stop and record DEFER when the only clean deduplication requires a public
   API, feature-capability, package-topology, or support-tier change.

## Completion definition

The program is complete only when:

- the shipped Python stubs match the supported runtime surface exercised by
  representative strict typing fixtures;
- remaining core/server connection overlap is classified and either safely
  converged or explicitly bounded/accepted with structural parity guards;
- async-Python lifecycle/admission/streaming/shutdown parity has deterministic
  regression coverage;
- stale migration suppressions/import residue are removed where possible;
- the topology checker is easier to maintain without any lost rejection rule;
- direct-H1 versus compatibility-multiprotocol Rust usage is documented
  unambiguously;
- all existing public Rust/Python paths and capabilities remain intact; and
- the exact closure candidate passes local qualification and remote CI.

## Non-goals

This program does not authorize a new application-server product, native
Python async runtime redesign, H2/H3 promotion, new listener type, new TLS
capability, new proxy behavior, sendfile/io_uring work, executor replacement,
`async-trait`, public Hyper exposure, or a new shared crate introduced only
to reduce file similarity.
