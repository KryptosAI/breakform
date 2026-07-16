# Governance

Breakform is stewarded under a BDFL + specification-first model through spec v1.0.

## BDFL editorship

The project lead serves as Benevolent Dictator For Life, holding final authority over spec direction, release scoping, and the Apache-2.0 licensing commitment. The BDFL delegates day-to-day review and maintenance to trusted maintainers.

## Spec change policy

No specification change is accepted without a shipped, working implementation in the reference library and a fidelity-report test demonstrating conformance. This applies to every schema addition, deprecation, and semver increment. The spec is not a design document — it is a machine-verifiable contract.

## Foundation transfer trigger

When three or more external organizations ship products on the Breakform spec (as measured by passing the conformance suite), governance transitions to an independent foundation with multi-stakeholder representation. The trigger date is determined by the BDFL based on public conformance submissions.

## Maintainer responsibilities

Maintainers retain authority over:

- **Trademark**: the name "Breakform" and the project logo, enforced to prevent confusing use.
- **Conformance mark**: defining and administering what qualifies as conformant usage of the exl format identifier.
- **Hosted services**: operating the reference hosted conversion API, model registry, and benchmark dashboard.

## Contributor agreement

All contributions are accepted under Apache-2.0 with DCO sign-off. There is no CLA, no copyright assignment, and no relicensing path. Every contributor retains full copyright to their contributions; the license grant is inbound=outbound.
