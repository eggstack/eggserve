# Plan 258 — Post-256 async suppressed-body lifetime qualification and closure

## Purpose

Close the narrow post-Plan-256 corrective introduced by Plan 257.

This plan does not reopen the completed Rust authority, Python typing, topology,
or broad async-lifecycle work from Plans 251–256. It qualifies only the
suppressed async-stream resource-lifetime defect discovered after Plan 256.

Planning baseline:

```text
cd6061a97f6538d013f0fac2adc1653a96097dd0
```

The Plan 256 implementation candidate
`4c145421c851fffa5e1f6762a7ef742c5db1e5d8` passed remote CI run
`35653232800`. Plan 258 must preserve that campaign's other closure claims and
supersede only the async suppressed-body permit/task-lifetime claim.

## Preconditions

Before entering closure:

- Plan 257 has a deterministic baseline reproducer for permit retention;
- the corrective is implemented with no public API change;
- HEAD/body-forbidden async stream bodies remain unconsumed;
- the permit/task owner terminates immediately when the body is dropped without
  a first pull;
- ordinary async streaming behavior remains green.

## Closure artifact

Create:

```text
release/plan-258-async-suppressed-body-lifetime-corrective-closure.md
```

Record:

- Plan 257 planning baseline SHA;
- Plan 257 implementation SHA;
- exact closure candidate SHA;
- the baseline reproducer and why it failed before the fix;
- the final lifetime-owner design;
- exactly-once permit/task cleanup evidence;
- focused Python test results;
- full installed-wheel result;
- structural/Rust results if Rust/PyO3 code changed;
- exact remote CI run ID and job conclusions;
- any later metadata-only documentation SHA separately.

Do not claim that a later metadata-only commit was the exact qualified
candidate unless CI actually ran on it.

## Track A — prove the original defect

The release record must contain a concise causal proof:

```text
stream marker returned
 -> async permit transferred
 -> producer task created
 -> producer waits for first_pull
 -> Rust suppresses HEAD/body-forbidden body without polling Python iterator
 -> never-entered Python generator cleanup is not executed
 -> producer remains until first-pull timeout
 -> permit remains owned until producer task completion
```

Record the exact regression test that failed against the Plan 257 baseline.

Do not close merely because source inspection looks correct.

## Track B — suppressed-body immediate reuse test

On the final candidate, with `max_async_tasks=1` and a deliberately long
`response_write_timeout_secs`:

1. send a streaming HEAD response;
2. verify the application iterable is not advanced;
3. immediately send an ordinary buffered request;
4. require ordinary success rather than 503;
5. verify bridge-owned task/admission state has returned to baseline.

Repeat for at least 204.

If 304 coverage was implemented under Plan 257, retain it here.

The test must finish without waiting for the configured response-write timeout.

## Track C — repeated resource-lifetime evidence

Run repeated sequences that would have exhausted the old bridge:

```text
HEAD stream -> normal GET
HEAD stream -> normal GET
...
```

and:

```text
204 stream -> normal GET
204 stream -> normal GET
...
```

Use a small async-task bound to make any retained permit immediately visible.

Required observations:

- zero application iterable pulls for suppressed streams;
- no 503 caused by historical suppressed requests;
- no growth in bridge-owned task count;
- no semaphore over-release;
- no dependence on server shutdown for cleanup.

This is a lifetime regression, not a throughput benchmark. A moderate
deterministic loop is sufficient.

## Track D — ordinary streaming parity

Run the complete Plan 254 streaming/lifecycle matrix and record that the
corrective did not alter:

- buffered response permit release;
- active stream permit hold;
- first-pull producer startup;
- bounded queue backpressure;
- empty-chunk handling;
- non-bytes/producer-error truncation;
- exact known length;
- under/overrun behavior;
- trailers;
- disconnect cancellation;
- producer no-progress timeout;
- server shutdown;
- tunnel/task tracking;
- sync iterable convenience.

Any regression in these semantics blocks closure.

## Track E — exactly-once cleanup mutation/white-box evidence

Add one or more focused assertions around the private lifetime owner proving
cleanup is idempotent.

Exercise combinations such as:

- explicit close then finalization;
- consumer drop then shutdown;
- producer EOF then cleanup;
- producer error then cleanup;
- timeout racing with cancellation.

The async semaphore count must never exceed its configured capacity and the
producer task must not remain registered after terminal cleanup.

No public introspection API should be added; tests may use private state.

## Track F — canonical suppression authority check

Review the final diff and prove that Python has not become the authority for
HTTP payload suppression.

The final architecture should still be:

```text
canonical Rust request/status normalization
    -> decides whether response body is consumed
        -> Python stream lifetime owner reacts correctly to pull or drop
```

The bridge may optimize around method/status only if correctness remains valid
when Rust drops the iterable without a pull.

If a duplicated Python status table became necessary, Plan 258 must not close
without a separate architecture review.

## Track G — sync-Python streaming non-regression

Because the same native `Response.stream` conversion may be touched by an
acceptable Plan 257 implementation, re-run synchronous Python callback stream
coverage:

- HEAD does not advance iterable;
- 204/body-forbidden does not advance iterable;
- ordinary stream chunks correctly;
- iterator error truncates;
- known-length behavior remains correct;
- trailer path remains correct.

If Plan 257 is Python-only and does not alter the native adapter, record that
fact and still retain the existing sync suite in the wheel run.

## Track H — public surface freeze

Re-run the Plan 252 strict typing/public API fixtures.

Confirm no changes to:

- `AsyncServer`, `AsyncRequest`, `AsyncResponse`, `AsyncBody`,
  `AsyncTunnel`, or `AsyncTunnelCapability` signatures;
- `RuntimeConfig` fields/defaults;
- Python import paths;
- native `Response.stream` public signature;
- Rust public items;
- H1-only Python support tier.

Any public API addition made only to solve this internal lifetime bug blocks
closure.

## Track I — local qualification

At minimum:

```sh
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-crate-topology.py
python3 scripts/check-python-release-metadata.py
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
bash scripts/test-python-wheel.sh
```

Also run all focused Plan 257 regressions and the full
`test_async_lifecycle.py` suite.

If Rust/PyO3 code changed:

```sh
cargo fmt --all -- --check
cargo +1.89 check --workspace --all-targets
cargo +1.89 check --workspace --all-targets --features http2,tls
cargo +1.89 check --workspace --all-targets --features http3,tls
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
```

Run supply-chain/package checks if Cargo manifests or Rust dependency closure
changed; otherwise record why they are unchanged from Plan 256 and still rely
on normal remote CI for the candidate.

## Track J — remote CI provenance

Push the exact evidence candidate and require successful normal GitHub Actions.

Record:

- exact candidate SHA;
- workflow run ID and URL;
- created/completed timestamps;
- `rust` conclusion;
- `supply-chain` conclusion;
- `python` conclusion.

Normal CI must pass on the exact candidate before closure.

## Track K — historical reconciliation

After successful exact-SHA CI:

- mark Plans 257–258 complete in `plans/ROADMAP.md`;
- add the Plan 258 release evidence link;
- update the Plan 256 release record only with a narrow supersession note:
  Plan 256 remains valid for typing, topology, connection-overlap, import
  cleanup, and the rest of the async parity matrix, but its suppressed-body
  permit/task-lifetime claim is corrected by Plans 257–258;
- update `AGENTS.md` only if the async bridge invariant summary needs to state
  the corrected lifetime owner.

Do not rewrite Plan 256 as though the defect was known at its original closure.

## Acceptance criteria

- [ ] the Plan 257 baseline reproducer fails before the fix and passes after it.
- [ ] streaming HEAD does not advance application state.
- [ ] streaming HEAD releases task/permit ownership immediately when suppressed.
- [ ] streaming 204/body-forbidden responses do the same.
- [ ] immediate subsequent requests are admitted with
      `max_async_tasks=1`.
- [ ] repeated suppressed responses do not accumulate tasks or permits.
- [ ] ordinary async streams retain correct bounded lifetime/backpressure.
- [ ] cleanup is exactly once across drop/error/timeout/shutdown races.
- [ ] canonical Rust remains the HTTP suppression authority.
- [ ] sync Python response streaming is not regressed.
- [ ] public Python/Rust API and support tiers are unchanged.
- [ ] full installed-wheel qualification is green.
- [ ] exact closure candidate passes remote rust/supply-chain/python CI.
- [ ] Plan 256 historical evidence is reconciled narrowly and truthfully.

## Closure rule

If the suppressed body can still retain a permit until
`response_write_timeout_secs`, this corrective is not closed even if all
ordinary stream tests pass.

If fixing that behavior requires a public API or duplicated HTTP policy table,
stop and open a separate architecture plan rather than weakening these
acceptance criteria.
