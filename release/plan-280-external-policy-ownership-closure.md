# Plan 280 — External H1 policy ownership closure

Implemented explicit `PolicyOwner` values for handler/body/idle/write
deadlines and global body/target ceilings. Defaults remain EggServe-owned;
parser buffer/header-count/header-byte/framing limits remain mandatory.
External mode omits only the selected EggServe semantic timer/ceiling.

Direct-server integration coverage proves external handler/body/idle/write
ownership, an externally owned global body ceiling composed with service body
policy, external target ceiling with target validation retained, and the
independent connection-total ceiling configuration. Focused integration
command: `cargo test -p eggserve-server --test downstream_embedding`.

Registry artifact qualification is consolidated into Plan 286. Hosted CI
qualification is recorded by Plan 285.
