# ADR-0001: Brand and namespace

**Status**: accepted

## Context

The project was initially developed under the working name "exl" — matching the file extension (.exl/.exlb), the serialization magic (`EXLB`), and the Rust crate namespace (exl-core, exl-geo, exl-io, etc.). As the project matured into a public-facing identity, a separate brand name "Breakform" was chosen for the project, repository, and community. The question: should the format namespace also be renamed?

Renaming the exl namespace would require churn across: the wire format binary magic, every crate name and import, every file extension association, every existing document header (`#exl`), every downstream converter configuration, and every reference in the specification. The format identifier is a protocol-level constant; brand identity is a marketing and community surface.

## Decision

- **Project brand**: Breakform (repo, website, CLI binary `bf`, documentation, logos).
- **Format namespace**: `exl` retained — `.exl` / `.exlb` file extensions, `#exl` text header, `EXLB` binary magic bytes, `exl-*` Rust crate prefix, `import exl` Python module.
- The brand names the project; `exl` is the stable wire-format identifier.

### Precedents

- **Ruff**: the project is "Ruff"; the Python package is `ruff`; the Rust crates are `ruff_*`. No expectation that file extensions or crate names differ from the brand.
- **Pixar USD (OpenUSD)**: the project is "Universal Scene Description"; the file extension is `.usd`; the brand "OpenUSD" and the format ".usd" are distinct.
- **Protobuf / gRPC**: the protocol is "Protocol Buffers"; the file extension is `.proto`; the tool is `protoc`. No brand-format confusion results.

## Consequences

**Positive**: zero churn to the wire format, crate namespace, or any serialized data. Existing documents, libraries, and scripts that reference `exl` continue to work. The brand "Breakform" can evolve independently of the format identifier.

**Negative**: new users may briefly wonder why the CLI is `bf` but the files are `.exl`. This is addressed through consistent documentation and a short explanation in the README. The analogy (brand vs. wire format) is well understood in systems software.

**Mitigation**: the README and spec both include an explicit "Brand & namespace" note. Every CLI help message refers to "exl format" not "Breakform format." The GitHub org (KryptosAI/breakform) hosts the project; the crate registry (crates.io) hosts `exl-*` crates.
