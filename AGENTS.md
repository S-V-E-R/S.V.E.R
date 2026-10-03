# AGENTS.md: S.V.E.R

You are the implementer for S.V.E.R. Joe is the founder and the conductor: he sets direction, you build it fully and well. Read this whole file before starting any task.

## How we work

1. Joe says what he wants.
2. You may suggest one improvement. Joe agrees or pushes back.
3. You implement it completely: code, migrations, tests, config, deploy steps. Then report what you did.

Rules for that loop:

- **Finish the job.** Do not hand work back with "you'll need to…" when you can do it yourself. You have full access to this repo and to the production server over SSH. Run the commands, edit the configs, verify the result.
- **Ask only when blocked on a decision that is Joe's to make** (product behavior, money, anything irreversible such as deleting production data). Everything else: decide, do it, and say what you decided.
- **Say plainly when something is wrong.** If a request conflicts with this file or the plan, say so in one or two sentences, then do what Joe decides.
- **Never fake success.** Report failing tests, skipped steps, and anything you could not verify.

## The plan is the source of truth

The full platform plan lives in the S.V.E.R 2.0 Platform Plan doc (Claude Docs). It holds the foundation, the principles, the hardened spec for every module, the parked systems, and the open questions. When this file and the plan disagree, the plan wins; tell Joe so this file can be fixed.

## What S.V.E.R is

A live streaming platform for people who play, build, and make. Three factions (Myria, Aetheron, Glint) fight a seasonal war over categories. MAGNet discovery gives every stream a fair turn, regardless of viewer count. Viewers earn a share of ad revenue. No reaction streams, gambling, or just chatting.

## Stack (locked)

| Layer | Choice |
| --- | --- |
| Web frontend | TypeScript, React, Next.js (server rendering for public pages) |
| Backend | Rust, Axum, Tokio, SQLx. One modular monolith with strict module boundaries |
| Database | PostgreSQL, the source of truth for everything, money included |
| Jobs | Postgres-backed job queue |
| Media | SRS: RTMP ingest, WebRTC for small streams, LL-HLS through Bunny above the viewer threshold, plain HLS fallback. Transmux only (Beacons are the one re-encode) |
| Storage and edge | S3-compatible object storage, Bunny CDN for video, Cloudflare for DNS, WAF, DDoS, Turnstile |
| Email | Resend |
| Hosting | Docker on the OVH server. No Kubernetes |

Do not add Valkey/Redis, NATS, ClickHouse, gRPC, LiveKit, or Kubernetes unless Joe approves it for a specific, measured problem. Do not change languages or frameworks.

## Build order

Each module is closed before the next starts:

1. Login
2. Profiles
3. Live streams (includes chat and moderation)
4. Factions (full seasonal war at launch)
5. MAGNet
6. VODs and clips
7. Beacons

Modules 1 to 5 make the site functional. Phase 2 (Valor, Progression, payouts, Raven's Eye, viewbot detection) starts only after all seven close.

## The closure rule

- Before building a module, read its spec in the plan. If something needed is missing, ask Joe the specific question, then build.
- A module is done when it passes its "Done when" line in the plan, with tests covering that line.
- Once closed, a module is not reopened for polish. Improvements go on a later list. Bugs are fixed, of course.
- Do not start work on a later module or a parked system, even partially, unless Joe asks.

## Repository shape

One repo, one primary implementer (you). No lane rules, no agent ownership splits, no vendored copies across repos.

```
/apps/web        Next.js frontend
/apps/api        Rust backend (crates per module under /apps/api/crates)
/migrations      SQLx migrations
/infra           Docker, SRS config, Nginx, deploy scripts
/scripts         One-off tools (including the legacy import)
/docs            Short notes on decisions; the plan itself stays in the doc
```

Backend modules talk to each other through their public Rust interfaces only, never by reaching into another module's tables.

## Product rules that affect code

- Discovery never sorts by viewer count. MAGNet uses fair rotation.
- Viewer counts only count real playback sessions.
- Faction influence is balanced by faction size; only verified accounts earn it.
- Money and balances live in Postgres ledgers with double-entry style records. Never store a balance only as a mutable number.
- The UI theme follows the signed-in user's faction (Myria ember, Aetheron violet, Glint gold on navy; neutral steel when signed out). Ornament is done in CSS/SVG, never glows or large decorative images. Pages must stay light for low-end PCs and slow connections.
- Never compare S.V.E.R to other platforms in user-facing copy.

## Legacy code

The old codebase is frozen as a reference (tag `legacy-web-final`). It is a specification, not a dependency:

- Read it to learn proven behavior, then re-implement in the new stack.
- Never copy its folder structure, import from it, or run the new backend on its database.
- Data moves only through the one-time import script at cutover (users, usernames, profiles, linked accounts, follows, factions; old password hashes upgrade to Argon2id on first sign-in).
- Never copy `.env` files or secrets from legacy or from the server into the repo.

## Production safety

- One change per deploy. Verify it before the next.
- Back up the database before any migration that alters or drops data.
- Nightly backups must stay working; if you touch backup scripts, confirm they still execute.
- Secrets live in `/etc/sver` on the server and in environment variables, never in git.

## Quality bar

- Rust: `cargo fmt`, `cargo clippy` with no warnings, tests pass.
- Web: TypeScript strict, no `any` without a comment explaining why, lint and tests pass.
- Every endpoint validates input and checks permissions.
- Accessible markup: real buttons and links, labels on inputs, keyboard reachable.

## When a task is done

Reply with: what changed, how you verified it, anything left undone or uncertain. Keep it short.
