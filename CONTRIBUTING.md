# Contributing to Breakform

## Development setup

```bash
cargo build --workspace
cargo test --workspace
```

Run benchmarks:

```bash
make bench
```

Regenerate the test corpus:

```bash
python3 scripts/gen_corpus.py
```

## DCO sign-off

All contributions must be signed off using the Developer Certificate of Origin (DCO). Add `-s` to your commit:

```bash
git commit -s -m "your message"
```

The sign-off certifies that you wrote the contribution or otherwise have the right to submit it under the Apache-2.0 license. The full DCO 1.1 text is available at https://developercertificate.org.

## Code style

This project uses `rustfmt` defaults. The only explicit setting is `edition = "2021"` in `rustfmt.toml`; all other options follow Rust edition 2021 defaults.

**No comments policy**: this codebase intentionally avoids code comments. Instead, write clear, self-documenting identifiers and keep functions small. Tests serve as executable documentation.

**Tests required for converters**: every format converter (`exl-fmt`, `exl-step`, `exl-gltf`) must include round-trip and fidelity-report tests before a PR is merged.

## Spec change policy

This project follows a **spec-with-implementation** governance model:

- No specification change is accepted without a shipped, working implementation in the reference library (`exl-core` and the associated converter crates).
- Prefer profiles over universality: spec additions should target a concrete profile (e.g. `mech`, `cfd`, `fea`) rather than attempting to cover every possible use case.
- Every spec change must be accompanied by a fidelity-report test demonstrating that the implementation correctly handles the new specification element.

## Fidelity-report honesty rule

Converters must **never silently drop data**. Every piece of information that cannot be represented in the target format must be recorded in the fidelity report. If a conversion loses semantics, the report must say so explicitly.

## PR checklist

Before opening a pull request, confirm:

- [ ] All tests pass (`cargo test --workspace`)
- [ ] Fidelity-report tests exist for any converter changes
- [ ] DCO sign-off on all commits (`git log --show-signature` or verify `Signed-off-by:` trailer)
- [ ] Spec updated if the format schema changed
- [ ] `cargo fmt` produces no changes
- [ ] `cargo clippy --workspace -- -D warnings` produces no new warnings
