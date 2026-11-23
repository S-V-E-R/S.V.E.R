## What this changes

<!-- One or two sentences. Link the issue: Closes #123 -->

## How I tested it

<!-- Commands you ran, what you checked by hand. -->

## Checklist

- [ ] One change per pull request
- [ ] `cargo fmt --check`, `cargo clippy`, `./scripts/dev.ps1 test` pass (API changes)
- [ ] `pnpm typecheck`, `pnpm test`, `pnpm lint`, `pnpm build` pass in `apps/web` (web changes)
- [ ] `pnpm typecheck`, `pnpm test`, `pnpm lint`, `pnpm bundle` pass in `apps/mobile` (mobile changes)
- [ ] Tests added or updated for changed behavior
- [ ] No secrets, real user data, or private tuning values
