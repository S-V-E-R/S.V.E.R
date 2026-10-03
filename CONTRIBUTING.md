# Contributing to S.V.E.R

Thanks for helping build S.V.E.R, a live streaming platform for people who play, build, and make. This guide covers what we accept right now and how to get a change merged.

## What we accept right now

S.V.E.R is built one module at a time, in this order: Login, Profiles, Live streams, Factions, MAGNet, VODs and clips, Beacons. See `docs/ROADMAP.md` for where things stand.

- **Bug fixes:** always welcome.
- **Work on the current module:** welcome. Comment on the matching issue first so two people don't build the same thing.
- **Later modules and new features:** open an issue to discuss before writing code. Work that jumps ahead of the build order will usually wait.

## Set up

Follow "Local development" in `README.md`. You need Docker, Rust (pinned by `rust-toolchain.toml`) and Node 24 (pnpm through `corepack`).

## Before you open a pull request

Run the same checks CI runs:

```bash
cd apps/api && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
cd apps/web && pnpm typecheck && pnpm lint && pnpm build
```

Then:

- Keep each pull request to one change, and say how you tested it.
- Add or update tests for behavior you change.
- Follow the conventions in `AGENTS.md`. It is written for AI coding tools but applies to everyone: the stack, the module boundaries, and the product rules (fair discovery, real viewer counts, money in ledgers).
- AI-assisted contributions are welcome. You are responsible for every line you submit.

## Contributor License Agreement

The first time you open a pull request, a bot asks you to sign the CLA (`CLA.md`) by posting a comment. It takes a few seconds and covers all your future contributions. We can't merge a pull request until it's signed.

## Never commit

- Secrets of any kind: keys, tokens, passwords, `.env` files.
- Real user data.
- Anti-abuse tuning values (thresholds, weights, detection limits). The code that uses them is public; the live values are private server config. Use the example values in the repo for tests.

CI scans every push for leaked secrets.

## Security issues

Do not open a public issue. Follow `SECURITY.md`.

## Conduct

Everyone in this project follows the `CODE_OF_CONDUCT.md`.

## License

S.V.E.R is licensed under the GNU Affero General Public License v3.0 (`LICENSE`). By contributing, you agree your contributions are licensed under it and under the terms of the CLA.
