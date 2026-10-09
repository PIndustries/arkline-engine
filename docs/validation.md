# Private model validation

This document records evidence for the initial Rust model, not approval to publish.

## Automated local checks

On the development Apple Silicon Mac with Rust 1.89.0:

- Workspace compilation, formatting and Clippy with warnings denied.
- 17 tests: private client credential persistence/resume; server authorization,
  failed-operation rollback, concurrent fill, lifetime DB ownership, in-memory
  isolation, persisted travel/cooldown; TCP two-pilot journey; nine verifier rule
  and tamper checks; checkpoint CLI persistence on failure.
- The TCP journey checks mining and a partial trade, abruptly kills the process,
  resumes both saved identities after restart, checks balances/escrow, cancels the
  remainder, and verifies the exported history with the independent verifier.

This is not a stress test, distributed failure test, or simulated interruption
during an active SQLite commit. CI logs supply separate hosted-run evidence.

## Owner validation journey

Open the preview, create a pilot, open a second cockpit with another name, and
complete the flight/mining/trading steps in README.md. Check whether the interface
and shared world match the intended Arkline direction. Close/reopen the host and
resume the same identities. The ledger verifier can connect to the local address
shown in the cockpit.

Owner acceptance is pending. Neither a build nor an automated journey constitutes
that acceptance. Keep repository visibility private; selecting a license and
publishing require explicit owner approval.
