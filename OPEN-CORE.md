# Open-core boundary

Breakform is an open-core project. What ships here is free forever.

## In this repo (Apache-2.0, free forever)

| component | scope |
|-----------|-------|
| Format specification | complete exl schema and wire-format definition |
| Reference library | exl-core, exl-geom, exl-io, exl-fmt, exl-step, exl-gltf, exl-diff, exl-validate |
| CLI | `bf` binary — convert, validate, diff, inspect |
| Python bindings | exl-py via PyO3/maturin |
| Test corpus | 50-model regression suite under `corpus/` |
| Benchmarks | benchmark runner and HTML dashboard |

## Commercial (outside this repo)

| service | description |
|---------|-------------|
| Hosted conversion API | high-throughput REST/gRPC conversion with SLA |
| Hosted validation API | cloud validation against configurable profiles |
| Team model registry | private model storage with versioning and access control |
| Proprietary-kernel bridges | Parasolid, ACIS, and other commercial kernel integrations |
| Enterprise features | SSO, audit logging, compliance reports, air-gapped deployment |

## The rule

**Never paywall a format.** The Breakform spec, all converters, and all schema definitions remain fully open-source and free. Commercial tiers sell throughput, assurance, and access to proprietary geometry kernels — never the ability to read, write, or convert exl data.

## Contributor guarantee

Every contribution to this repository is accepted under Apache-2.0 with DCO sign-off. There is no CLA, no copyright assignment, and no relicensing path. If you contribute to this repo, your code is Apache-2.0 forever.
