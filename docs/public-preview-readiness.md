# Open-source developer alpha readiness

Historical assessment for build 26.10.10.24. At that checkpoint, the repository was private.
The owner had selected AGPL-3.0-only. Publication still required explicit approval.
A public source alpha does not imply that the official network is ready for
untrusted Internet players.

See [Releases](https://github.com/PIndustries/arkline-engine/releases) for published packages.

## Update: 2026-10-11

Public early-access downloads and WSS official Production/Test connections now
exist for 26.10.11.5. Source-repository visibility remains a separate owner decision.
Current source bundles include vendored corresponding source and collected
third-party notices; the older table below records the earlier review, not the
current distribution state. See [licensing.md](licensing.md), [browser.md](browser.md)
and [../deploy/PUBLIC.md](../deploy/PUBLIC.md) for current boundaries.

A read-only redacted Gitleaks pass over 46 locally available commits through
`c7b268d` reported zero findings; historical object names contained no matches for
credential/private-key/database filename patterns. This does not certify deleted
remote refs, release attachments or undiscovered secrets. Re-scan the final commit.
Before making the repository public, reconcile older private-preview release
attachments with their historical licenses/source/notices, review operational
inventory for intended disclosure, and finish explicit owner acceptance.

[SECURITY.md](../SECURITY.md) now gives the existing private P.Industries contact
and distinguishes WSS from private raw TCP. Public contribution CI must not run
untrusted fork code on persistent privileged runners; current push/dispatch jobs
are not a substitute for an isolated contribution workflow.

## Recommended release target

Publish a developer alpha once a new contributor can build, run a local universe,
complete one dependable gameplay loop and understand the known limitations.
Complete graphics, planetary gameplay, every ship class, distributed hosting and
all economic systems are not prerequisites for inviting source contributors.

## Six publication gates

| Gate | Current evidence | Remaining work |
|---|---|---|
| License and rights | AGPL-3.0-only selected; LICENSE, five package manifests and contribution terms updated | Complete actual dependency-license, asset-provenance and redistribution-notice review. See licensing.md. |
| Repository hygiene | Gitleaks scanned 33 commits across locally available refs through ebfa4c9, with zero findings; tracked filenames contained no token/database/key matches | Review asset/import provenance and release bundle contents; automated secret scanning is not proof that every private artifact is absent. Re-scan final release history. |
| Reproducible onboarding | Locked Rust workspace, source commands, successful Linux/macOS CI for ebfa4c9 | Validate the documented local journey from a clean environment, document Linux native packages and supported-platform limits, and ensure no P.Industries credentials or Spark are required. |
| Stable core loop | Automated flight/mining/economy/restore and quarters journeys; dated native observations in validation.md | Complete one current-build hands-on journey: connect, undock, mine repeatedly, store, walk/board, reconnect/restart, preserve ownership and balances. Fix blocking freezes; record remaining reproducible defects openly. |
| Contributor and operator documentation | CONTRIBUTING.md and SECURITY.md exist, but describe private validation | Publish a working vulnerability-report route, contribution/review policy, supported-version and backup/restore instructions, a focused roadmap and starter issues. Keep direct Internet hosting explicitly out of scope until separately reviewed. |
| Owner release acceptance | Private artifacts and build-specific CI exist | Owner validates the intended alpha and authorizes publication. Then tag the exact passing revision and change visibility. |

## Evidence from this pass

- Current shipped reference: 26.10.10.23, ebfa4c9cc527faee33fce105b1262f7515a26370.
  Hosted Linux/macOS CI passed for that revision. Later source changes require
  their own checks; earlier green CI does not certify them.
- Cargo metadata resolved 456 third-party packages across platform targets. Every
  package declares license metadata. This is an inventory, not a compatibility or
  attribution sign-off. Bundled fonts have additional licenses; inspect actual
  license files and prepare redistribution notices for the chosen package scope.
- Icons have an in-repository generator; procedural graphics and synthesized audio
  also require an authorship/provenance record. Do not assume generated assets are
  automatically cleared merely because there are no downloaded model files.
- A walking defect used a full epoch timestamp cast to f32, losing useful animation
  precision. The new presentation gait uses bounded frame deltas and displacement;
  cycle, stop, idle-turn and seated behavior have dedicated regression coverage.

## Separate official-network launch

Public source can remain a local/trusted-tunnel developer preview. A publicly
reachable official server additionally needs encrypted transport, abuse and load
limits, account recovery/rotation, tested operational backups and recovery,
monitoring and a defined trust/administration policy. These are not implemented
merely by opening the repository. The economic ledger is an audit trail, not
independent consensus or proof of honest server operation.

## License decision

The selected AGPLv3 license supports the policy that operators of modified
networked versions offer their corresponding source to their users. GPLv3 has
copyleft obligations on distribution but lacks that additional network-use clause.
Neither choice requires a fork to join the official network or submit changes
upstream. Review the intended server/client/SDK boundaries and dependency notices
before public distribution. AGPL-3.0-only is now selected; asset/dependency review remains open.

Official reference: https://www.gnu.org/licenses/why-affero-gpl.html
