# S.V.E.R

Live streaming for people who play, build, and make. Three factions, one seasonal war over every category.

Read `AGENTS.md` before working in this repo. Build status is in [docs/ROADMAP.md](docs/ROADMAP.md): Login is finished and staged at https://sver.tv/login; Profiles is closed (Joe accepted it on staging on October 3, 2026, 7:31 PM ET); Live streams has started. Module specifications: [Login](docs/LOGIN.md), [Profiles](docs/PROFILES.md), [Live streams](docs/LIVE_STREAMS.md) and its [legacy carryover review](docs/LIVE_STREAMS_LEGACY.md). The platform plan, operations notes and the work log are kept private; docs that mention `PLATFORM_PLAN.md`, `OPERATIONS.md` or `ChangeLog.md` refer to those.

## Layout

```
apps/api      Rust backend (Axum, Tokio, SQLx); crates under apps/api/crates
apps/web      Next.js frontend (TypeScript, React)
migrations    SQLx migrations, applied by the API on startup
infra         Dockerfiles, Nginx and backup units
scripts       Dev script, component/navigation/media checks, legacy export query
docs          Module specifications and decision notes
```

## Local development

Install Rust (pinned by `rust-toolchain.toml`), Node 24 (pnpm comes through `corepack`) and Docker Desktop. Keep secrets outside this repository. Local configuration lives at `%USERPROFILE%\SVER-dev\login.env`; `login.env.example` lists the names. Generate your own database password and 32-byte encryption key. Never point this service at a production database; it refuses a database containing the legacy Prisma/User tables.

```powershell
./scripts/dev.ps1 database
# In separate terminals:
./scripts/dev.ps1 api
./scripts/dev.ps1 web
```

Open http://localhost:3000. Postgres is bound only to `127.0.0.1:15432`. Development uses Cloudflare's official Turnstile test keys. With no Resend key, mail stays encrypted in the local `mail_jobs` table; `./scripts/dev.ps1 mail` writes verification/reset links to `%USERPROFILE%\SVER-dev\mail-preview.txt`, outside the workspace. Treat that file as sensitive and delete it after testing. Provider buttons stay disabled until credentials are configured.

## Checks

```powershell
./scripts/dev.ps1 test                      # reserved routes, login, profiles, streams, unit tests
cd apps/api; cargo fmt --check; cargo clippy --all-targets -- -D warnings
cd apps/web; corepack pnpm typecheck; corepack pnpm lint; corepack pnpm build
```

The same checks run on every push (`.github/workflows/ci.yml`), plus a scan for leaked secrets. Integration tests require a local database named `sver_rebuild`; each run creates a uniquely named schema and removes only that schema. They use synthetic users and controlled HTTP responses, so no real email is sent. For the explicit live Resend simulator check, set `SVER_TEST_RESEND=1` only in the test process.

Browser-level checks: `node scripts/check-navigation.mjs` (against a built frontend on port 13001 and the local API), `node scripts/check-signup.cjs <path-to-jsdom>` and `node scripts/check-stream-studio.cjs <path-to-jsdom>` for component checks with mocked network traffic, and `scripts/check-media.cjs` for the isolated real-media proof.

The anonymous removal form also has a component check: `node scripts/check-take-it-down.cjs <path-to-jsdom>`. Its backend acceptance cases run within the Profiles integration suite, using generated non-sensitive images and a fake cache-purge service.

The opt-in Rust/SRS ingest test also checks real callbacks, media decoding, reconnect, rotation and Stop against an isolated database and disposable media server. See [prerequisites and run command](docs/LIVE_STREAMS.md#integrated-rustsrs-ingest-proof--october-3); it is separate from normal CI and does not establish OBS/browser/CDN acceptance.

The single reserved-username list is `apps/api/crates/sver/src/reserved.rs`; `tests/reserved_routes.rs` fails if any `apps/web/app` or `apps/web/public` top-level entry or `apps/web/next.config.ts` redirect source is not reserved.

## Provider and production setup

OAuth callbacks default to `APP_ORIGIN/api/auth/oauth/{google,twitch,discord}/callback`. Optional `GOOGLE_REDIRECT_URI`, `TWITCH_REDIRECT_URI` and `DISCORD_REDIRECT_URI` preserve exact registered callbacks for both authorization and token exchange. Enable Google email/profile, Twitch `user:read:email`, Discord `identify email`. Google and Discord send S256 PKCE; Twitch uses its documented confidential-client flow.

Production requires `APP_ENV=production`, an HTTPS `APP_ORIGIN`, real Turnstile keys, `RESEND_API_KEY`, a verified `MAIL_FROM`, a durable encryption key and a dedicated clean database. Serve Next and `/api` on the same origin and bind the API privately. If using `TRUSTED_PROXY_IP`, the proxy must overwrite `X-Real-IP` with the client IP and block direct public access to the API; otherwise the API uses the socket address. Images are built from `infra/api.Dockerfile` and `infra/web.Dockerfile` (see `compose.stage.yaml`).

Passwords, stream keys and 2FA setup/recovery values never belong in logs. Back up the database and the encryption key separately. `sver-import-check` defaults to a disposable local rehearsal; its `--check-live` and `--apply-live` modes are restricted to the dedicated live database.

Take It Down deployment also needs `MEDIA_CLOUDFLARE_ZONE_ID` and `MEDIA_CLOUDFLARE_PURGE_TOKEN` with Cache Purge permission for the zone serving media. Verify a synthetic URL purge before activation; storage deletion alone does not remove cached copies. Configure the staff `VAPID_PUBLIC_KEY`, `VAPID_PRIVATE_KEY` and `VAPID_SUBJECT`, then enroll each staff browser from `/admin/take-it-down`. Mail and push use durable Postgres jobs. See [the removal spec and acceptance limits](docs/TAKE_IT_DOWN.md).

## Contributing

S.V.E.R is open source. Read `CONTRIBUTING.md` to get started; outside contributors sign the CLA (`CLA.md`) once, through a bot on their first pull request. Report security problems privately as described in `SECURITY.md`.

## License

GNU Affero General Public License v3.0. See `LICENSE`. If you run a modified version of S.V.E.R as a service, you must publish your changes under the same license.
