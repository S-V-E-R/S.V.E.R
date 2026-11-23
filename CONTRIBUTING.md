# Contributing to S.V.E.R

Thanks for helping build S.V.E.R, a live streaming platform for people who play, build, and make. This guide covers what we accept right now and how to get a change merged.

## What we accept right now

S.V.E.R was built one module at a time: Login, Profiles, Live streams, Factions, MAGNet, Support, CrowdSync, VODs and clips, Beacons. All nine are done. `docs/ROADMAP.md` lists them and what has shipped since, and the specs in `docs/` describe each part as built.

- **Bug fixes:** always welcome.
- **New features and changes to how something works:** open an issue to discuss before writing code, so two people don't build the same thing. Finished modules aren't reopened for polish, so an improvement may wait.

## Set up

Follow "Local development" in `README.md`. You need Docker, Rust (pinned by `rust-toolchain.toml`), Node 24 (pnpm through `corepack`), and FFmpeg with ffprobe for the API tests.

## Before you open a pull request

Run the same checks CI runs, each line from the repository root. The API tests need the local configuration from `README.md` (`%USERPROFILE%\SVER-dev\login.env`) and its `sver_rebuild` database, so a bare `cargo test` fails. The dev script loads that file and starts the database:

```powershell
./scripts/dev.ps1 database   # start the local database
./scripts/dev.ps1 test       # API integration and unit tests
cd apps/api; cargo fmt --check; cargo clippy --all-targets -- -D warnings
cd apps/web; corepack pnpm typecheck; corepack pnpm test; corepack pnpm lint; corepack pnpm build
cd apps/mobile; corepack pnpm typecheck; corepack pnpm test; corepack pnpm lint; corepack pnpm bundle
```

CI also runs the integration kit tests, the `/credits` license check (after changing dependencies, regenerate it with `node scripts/credits.mjs`) and a secret scan; see `.github/workflows/ci.yml`.

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

CI scans every pull request and every push to main for leaked secrets.

## Security issues

Do not open a public issue. Follow `SECURITY.md`.

## Conduct

Everyone in this project follows the `CODE_OF_CONDUCT.md`.

## License

S.V.E.R is licensed under the GNU Affero General Public License v3.0 (`LICENSE`). By contributing, you agree your contributions are licensed under it and under the terms of the CLA.
