# Contributing to Adriane

Thanks for your interest in Adriane. This guide covers how to set up the project,
the standards we hold code to, and the sign-off we require on every contribution.

## License

This repository is the **open Adriane framework** — the Rust engine (`crates/*`),
the TypeScript and Python SDKs and supporting packages (`packages/*`), and the
Claude Code plugin (`plugin/*`), all under Apache-2.0. Contributions are made under
Apache-2.0 (inbound = outbound). See [`LICENSE`](./LICENSE).

## Developer Certificate of Origin (DCO)

We use the [Developer Certificate of Origin](https://developercertificate.org/) —
not a CLA. By signing off on a commit you certify that you wrote the patch or
otherwise have the right to submit it under the project's license.

**Every commit must be signed off.** Add a `Signed-off-by` trailer with your real
name and email:

```bash
git commit -s -m "feat(runtime): add fan-out budget guard"
```

This appends:

```
Signed-off-by: Your Name <you@example.com>
```

PRs whose commits are not signed off cannot be merged. To sign off a branch you
already wrote: `git rebase --signoff main`.

<details>
<summary>The DCO text you are certifying</summary>

```
By making a contribution to this project, I certify that:

(a) The contribution was created in whole or in part by me and I have the right to
    submit it under the open source license indicated in the file; or
(b) The contribution is based upon previous work that, to the best of my knowledge,
    is covered under an appropriate open source license and I have the right under
    that license to submit that work with modifications; or
(c) The contribution was provided directly to me by some other person who certified
    (a), (b) or (c) and I have not modified it.
(d) I understand and agree that this project and the contribution are public and
    that a record of the contribution (including all personal information I submit
    with it, including my sign-off) is maintained indefinitely.
```
</details>

## Development setup

Prerequisites: **Node ≥ 22**, **pnpm 10**, and a **stable Rust toolchain** (pinned in
`rust-toolchain.toml`). The framework is storage-agnostic — no database is required
to build, test, or run it.

```bash
pnpm install

# TypeScript / JS gates (run across all workspaces via Turbo):
pnpm build
pnpm typecheck
pnpm test
pnpm lint

# Rust engine:
pnpm rust:fmt    # cargo fmt --check
pnpm rust:lint   # cargo clippy --all-targets -D warnings
pnpm rust:test   # cargo test

# Native bindings (optional — enables the Rust fast-path locally):
pnpm napi:build  # builds crates/bindings/adriane_napi.node
pnpm py:build    # builds the pyo3 extension for local `import adriane`
```

## Standards

- **TypeScript strict everywhere** — no `as any`, no `@ts-ignore`, no skipped
  tests. Validate external data with Zod. Errors are typed classes, not bare
  `throw new Error(...)`. Public API only through each package's `src/index.ts`.
- **No `eval` / `new Function` / dynamic `import()` of user strings** anywhere.
  Graph conditions are named registry predicates, never eval'd code.
- **Tests live next to source** (`foo.ts` → `foo.test.ts`), behavior-focused;
  mock only I/O boundaries (DB/HTTP/LLM).
- **Conventional Commits** for messages. Structural decisions get an ADR under
  `docs/adr/`.
- **Prettier**: double quotes, semicolons, no trailing commas, width 100.

The authoritative per-layer rules live in `.cursor/rules/*.mdc` — worth reading
before structural work.

## Pull requests

1. Branch off `main`.
2. Keep PRs focused; explain the *why* in the description.
3. Ensure `pnpm typecheck && pnpm test && pnpm lint` and the Rust gates pass.
4. Sign off every commit (`-s`).
5. CI (`.github/workflows/unit.yml` and `rust.yml`) must be green.

## Security

Do **not** open public issues for vulnerabilities — see [`SECURITY.md`](./SECURITY.md).
