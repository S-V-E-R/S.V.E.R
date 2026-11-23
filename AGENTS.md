# AGENTS.md: S.V.E.R

You are the implementer for S.V.E.R. Joe is the founder and the conductor: he sets direction, you build it fully and well. Read this whole file before starting any task.

## How we work

1. Joe says what he wants.
2. You may suggest one improvement. Joe agrees or pushes back.
3. You implement it completely: code, migrations, tests, docs, config, and a pull request with green CI. Commit, push and open the PR without asking first. Merge and deploy only when Joe or this file says to; otherwise say the PR is ready. Then report what you did.

Rules for that loop:

- **Finish the job.** Do not hand work back with "you'll need to…" when you can do it yourself. Run the commands, edit the configs, verify the result.
- **Ask only when blocked on a decision that is Joe's to make.** Everything else: decide, do it, and say what you decided. Joe's calls, plus any this file names elsewhere:
  - product behavior the specs don't settle (for a small call, build a sensible default and flag it for Joe), and words in S.V.E.R's voice (lore, bot lines);
  - real money: live Stripe charges, refunds and payouts, opening a sale, paid services;
  - deleting production data or accounts, or server files you didn't create;
  - the S.V.E.R Plays runtime, and restarting or reconfiguring the media server;
  - provider dashboards and accounts (Stripe, Discord, app stores, Cloudflare, GitHub settings), and access to production or its credentials for anyone, other agents included;
  - anything published in S.V.E.R's name: announcements, social posts, app store builds, the desktop update feed;
  - installing system software on the server or Joe's PC;
  - deliberate differences from a design mockup;
  - anything else you can't undo.
- **Say plainly when something is wrong.** If a request conflicts with this file or a spec, say so in one or two sentences, then do what Joe decides.
- **Never fake success.** Report failing tests, skipped steps, and anything you could not verify.
- **Know your access.** A local session on Joe's PC has this repo, the private files and the production server over SSH. A session without them (a cloud session) branches from current origin/main, opens the PR, and says what it could not check or deploy.

## What S.V.E.R is

A live streaming platform for people who play, build and make: gaming, art, crafting and making, music, education and coding. No reaction streams, gambling or just chatting. Built by a streamer for community-minded streamers, especially smaller ones, so they can grow and earn without becoming a caricature.

S.V.E.R succeeds when, any time you open the site, at least one person is live: measured as hours per week with someone live, and distinct streamers per week. Every feature serves the core loop: open S.V.E.R, see live content at once, find someone interesting, watch, chat, follow, come back.

Three factions (Myria, Aetheron, Glint) fight a seasonal war over genres. MAGNet gives every live stream a fair turn, whatever its size. Viewers back creators with subscriptions, Valor and tributes; Ad Valor, their share of ad revenue, starts when ad sales open.

## Where things stand

All nine modules in `docs/ROADMAP.md` are closed, and several later systems shipped early at Joe's direction. The roadmap has the status; each spec's status line and "Done when" say what is left in it. Before launch, the docs still list a legal review of the site pages, a full live Take It Down run, and each paid feature's sales checklist.

S.V.E.R is tightening up before launch and outreach: audits, fixes, phone layouts and the mobile app. Over-building is the risk now. Make what exists work well, keep it fast and simple, and promote nothing the live site can't deliver.

## Sources of truth

1. Joe's latest instruction wins. When it changes how something works, update the spec in the same change.
2. The specs in `docs/` say how each part works as built. A pull request that changes behavior updates its spec in the same PR, written as built, with anything not built marked so. Date Joe's decisions in it.
3. `docs/ROADMAP.md` is the status of record and the public roadmap feed. Update it in the same PR when a module's status or the shipped-early list changes.
4. This file says how to work. Where it summarizes a spec and they disagree, the spec wins; tell Joe so this file gets fixed.
5. Joe's platform plan (the S.V.E.R 2.0 Platform Plan in Claude Docs, private copy at `docs/PLATFORM_PLAN.md`) holds the foundation and the principles; test new ideas against them. The specs replace the rest.

Private files stay on Joe's machine and are never committed: `docs/PLATFORM_PLAN.md`, `docs/OPERATIONS.md`, `ChangeLog.md`. They exist only in the main checkout (the first entry in `git worktree list`), so read them there.

## Product rules that affect code

The specs hold the detail. Every change keeps these:

1. **Watching comes first.** A signed-out visitor reaches a live stream in one click, with no S.V.E.R terms to learn. A feature earns top-level navigation only by strengthening an existing loop.
2. **Discovery never uses viewer count, follower count or money.** MAGNet says why each stream is showing (`docs/MAGNET.md`). S.V.E.R Plays stays listed, after every live creator.
3. **Only real people count.** The public viewer count is Counted and Trusted playback sessions; tiers, payouts, Ad Valor and faction influence use Trusted sessions only (`docs/LIVE_STREAMS.md`, "Session levels").
4. **Influence is earned and balanced, never a gate.** Only verified accounts earn faction influence, through Trusted sessions or capped, linked Discord activity (`docs/DISCORD_BOT.md`). Support counts supporters, never dollars, and a faction's score is divided by its active members. Factions are identity and discovery hints, never access restrictions.
5. **Money buys only what it says.** Nothing paid changes discovery, XP or chat priority, and paid looks never match earned ones.
6. **Money lives in Postgres double-entry ledgers:** append-only, every transaction sums to zero, one unique reference per posting, refunds as reversing entries. Never store a balance only as a mutable number.
7. **The server decides.** XP, achievements, orders and ad impressions come from server records and checks, never from what a browser claims.
8. **Interaction never blocks watching:** no pre-rolls, and ad breaks shrink the stream, never cover or stop it. Complex creator tools live in Creator Studio. Streamers keep their own software and are encouraged to multistream; S.V.E.R Desktop is optional.
9. **Notify less.** Celebrate members by name without pinging them; ping or DM only for what they asked for or must act on.
10. **Show only what is real:** no invented numbers, zero counts, "Soon" items or disabled links. A section waits until its data exists. With nothing live, show recently live channels and the war map, never a blank grid.
11. **Joe opens sales and closed features.** Wall and ad sales wait on their checklists, Beacon video uploads on known-abuse matching. Never turn a sales switch on or open uploads yourself.
12. **Take It Down stays reachable.** Every page keeps the footer's "Take It Down requests" link and every content surface its report link; valid removals finish within 48 hours (`docs/TAKE_IT_DOWN.md`).
13. **Ages:** under-13s can't join, and an account with no date of birth counts as under 18. Buyers under 18 need guardian confirmation and have a $50 monthly cap (`docs/SUPPORT.md`). Ad Valor cash-out and Wall seats are 18+.
14. **The look follows `docs/DESIGN.md` and `docs/design/README.md` with its mockups; read them before changing a page.** It is a game client, not a generic streaming site, and it wears the viewer's faction colors (the owner's inside a channel's area). Use the theme tokens; raw faction colors only for faction identity. Ornament is CSS and inline SVG: no glows, gradient washes, emoji icons or large decorative images. Discovery previews are still images, never extra players. Pages stay light for low-end PCs and slow connections.
15. **Copy is short, direct and specific:** say what a thing does, in plain words (`docs/DESIGN.md`, "Copy"). Never compare S.V.E.R to other platforms, tell a personal hardship story, or make the site look busier than it is. Empty states are one plain line and one next step.

## Stack (locked)

| Layer | Choice |
| --- | --- |
| Web | TypeScript, React, Next.js App Router, hls.js. Motion only for the moments in `docs/MOTION.md`; everything else animates in CSS |
| Mobile | Expo (React Native, TypeScript) with expo-router: a viewer app on the same API (`docs/MOBILE.md`) |
| Backend | Rust, Axum, Tokio, SQLx: one crate, with the Discord bot and all background work inside it |
| Database | PostgreSQL 18, the source of truth for everything, money included |
| Jobs | Per-feature Postgres job tables claimed with `FOR UPDATE SKIP LOCKED` by loops in the API (`jobs::tick`). No broker, no separate worker |
| Media | SRS: RTMP, WHIP and SRT ingest; WebRTC for small audiences, HLS through Bunny above the measured limit. Transmux only, except Beacons and advertiser videos |
| Payments | Stripe Checkout and Connect payouts with signed webhooks. Card data never reaches S.V.E.R |
| Messaging | Resend email, Web Push with our own VAPID keys, Expo push |
| Storage and edge | S3-compatible storage (Cloudflare R2), Bunny CDN for video, Cloudflare for DNS, WAF, DDoS and Turnstile |
| Hosting | Docker Compose on the OVH server behind Nginx. No Kubernetes |

Do not add Valkey/Redis, NATS, ClickHouse, gRPC, LiveKit, or Kubernetes unless Joe approves it for a specific, measured problem. Do not change languages or frameworks, and ask Joe before re-encoding any other video.

Run one API instance: chat, DMs and live events fan out in memory (`chat::Hub`). A second instance needs a tested cross-instance design first.

Next.js, React and Expo here may be newer than your training data. Before writing Next.js code, read the matching guide in `apps/web/node_modules/next/dist/docs/` (install first in a new worktree).

## Repository shape

`README.md` has the layout. The API is one crate, `apps/api/crates/sver`: `lib.rs` holds config, the router and request checks, `main.rs` starts the background loops, and integration tests live in its `tests/`.

This repo is the platform, all public: the API (Discord bot included), web, mobile app and integrations. S.V.E.R Desktop and the S.V.E.R Plays runner are separate products in their own repositories. They reach the platform only through its APIs (`docs/DESKTOP.md`, `docs/PLAYS.md`), and the API pieces they need are built here. Never vendor code between these repositories.

Rust is pinned by `rust-toolchain.toml`; CI and the web image use Node 24. Web and mobile each pin pnpm in `packageManager` and keep their own lockfile; run pnpm inside the app folder. In this file `pnpm` means `corepack pnpm`, or `npx -y pnpm@10.34.6` where corepack is missing (Node 26 doesn't bundle it).

## Reuse before you write

Backend paths are under `apps/api/crates/sver/src/`, web paths under `apps/web/`. Use these before writing your own:

- **Errors:** `profiles::Fail` and `Res<T>` in most modules; `Error` and `Result<T>` in `lib.rs` convert into them.
- **Who is calling:** `profiles::viewer` (optional) or `profiles::signed_in`; `auth::recent` for the five-minute step-up; `devapps::actor` where apps call with a bearer token.
- **Staff:** `safety::staff` for reads, `safety::staff_write` for changes, `safety::audit` to log every staff action.
- **Abuse limits:** `security::reserve`, `security::client_ip`, `security::turnstile`.
- **Secrets at rest:** `security::seal` and `unseal` (bound to a purpose); `security::token`, `security::digest`.
- **Text:** `text::clean`, `filter`, `plain`, `markdown`; chat rules in `moderation::check_send`.
- **Money:** `ledger::post` (idempotent by reference), `ledger::lock`, `ledger::balance`; Stripe calls go through `stripe.rs`.
- **Notifications:** `alerts::community` applies blocks and settings, then queues browser and phone push.
- **Risky features:** `switches::guard` refuses while staff have the switch off.
- **Background work:** a step in `jobs::tick`, or a loop in `main.rs` when it waits on an outside service.
- **Mail:** `safety::queue_notice` for an account's verified address, `jobs::queue_address` for any other address.
- **Uploads and storage:** `media::read_upload`, `store`, `queue_delete`, and `media::S3`.
- **Private tuning:** follow `magnet::Tuning`: safe defaults in code, a `<NAME>_TUNING_FILE` read once into `Config`.
- **SQL:** runtime `sqlx::query*` with `.bind`; there are no compile-time `query!` macros. SQL built with `format!` goes inside `sqlx::AssertSqlSafe`, with a comment saying where each piece comes from.
- **Module boundaries:** modules share one database, and reading another module's tables is fine. Write through the owning module's function when one exists, and add one rather than copy its SQL: money only through `ledger::post`, staff actions only through `safety::audit`.
- **Web data:** `lib/client-api.ts` (`send`, `useLoad`) in the browser; `lib/server-api.ts` (`apiGet`) and `app/session.ts` (`currentAccount`) on the server.
- **Web look:** tokens in `app/globals.css` switch with `data-theme` (`lib/theme.ts`); page shells are `components/SiteShell.tsx` and `SitePage.tsx`; shared parts such as `Form.tsx`, `UserChip.tsx`, `Avatar.tsx` and `Turnstile.tsx` live in `components/`.

## Shared lists to update

Tests fail when you forget these:

- A new top-level web route, `public/` entry or `next.config.ts` redirect source: add the name to `RESERVED` in `src/reserved.rs` (`tests/reserved_routes.rs`).
- A new WebSocket route: add it to the upgrade `location` in `infra/nginx-sver-stage.conf` (`tests/websocket_routes.rs`). Register it on one line in the handler's own file, `.route("/api/...", get(handler))`, so the test sees it.
- A new or changed Rust crate or web production package, Dependabot bumps included: run `node scripts/credits.mjs` and commit `apps/web/app/credits/credits.json`. CI runs it with `--check`; where `corepack pnpm` fails, set `PNPM` to the npx command above.

No test checks these:

- Writes that arrive without the site's `Origin` (webhooks, server-to-server calls) are refused unless `boundaries` in `lib.rs` lists the path. Endpoints apps call with a bearer token go in `app_callable` and use `devapps::actor`.
- Request bodies are capped at 16 KB; a route that needs more adds its own `DefaultBodyLimit`.
- A new notification kind needs a migration that re-creates `notifications_kind_check`. A community kind also goes in `alerts::COMMUNITY_TYPES`, `notification_type_settings_kind_check` and the settings labels in `apps/web/components/Notifications.tsx` and `apps/mobile/app/alerts.tsx`. Other CHECK lists (`reports_target_type_check`, `checkout_sessions_kind_check`) work the same way.
- A new emergency switch goes in `switches::SWITCHES`, or staff can never turn it off.
- Every foreign key to `users(id)` says `ON DELETE CASCADE` or `ON DELETE SET NULL`, so erasure can delete the account.
- The data export reads only tables with a column named in `SUBJECT_COLUMNS` (`account.rs`): give a new table's person column one of those names, or add its name there. Put tables holding secrets, abuse signals, staff notes or other people's details in `EXCLUDED_TABLES`.
- A new top-level page goes in `STATIC` in `apps/web/proxy.ts`, so loading it skips the channel lookup.
- A new file in `tests/` also goes in `scripts/dev.ps1 test`, or it runs only in CI. A file under `tests/streams/` or `tests/profiles/` runs only once declared with `#[path]` and called from `streams.rs` or `profiles.rs`.
- Every new setting the API reads goes in `login.env.example` with a safe example value.

## Migrations

- Add `migrations/NNNN_short_name.sql`. Before taking a number, check origin/main, every open PR branch and what production has applied, then take the next number above all of them; a session that can't see production says so in the PR. Gaps are fine; name any skipped number in the PR.
- The API runs migrations at startup (`sqlx::migrate!` in `lib.rs`) and refuses to start if an applied migration was edited (checksum) or is missing from the build.
- Renumber freely until production applies a migration. After that never edit, rename, renumber or delete it: fix forward with a new one.
- Deploy only a build that contains every migration production has applied.

## Working alongside other sessions

Several AI sessions land PRs here minutes apart, and anyone fixes anything:

- Work in your own worktree (`git worktree add`) on a branch cut from origin/main. Never switch branches, stash, reset, clean or commit in a checkout that holds someone else's uncommitted work, the main checkout included. All worktrees share one stash.
- Merge origin/main into your branch before opening or updating a PR, and resolve conflicts there. Never rebase or force-push a pushed branch; others may have pushed to it.
- Parallel audits find the same bugs. Check open PRs and recent main before fixing; when two fixes meet, keep one and say so in the PR.
- A red check may not be yours: check main's CI before chasing it.
- A new worktree has no `node_modules`: run `pnpm install --frozen-lockfile` in each app you touch, never a link to another checkout's.
- When another session is compiling, set `CARGO_BUILD_JOBS=2` before you build or test: parallel full builds have exhausted Windows' page file (os error 1455).

## Branches, commits and PRs

- Branch names are short kebab-case topics (`chat-audit`, `mobile-nav`) unless your tool assigns one; integration and release branches carry a date (`release/audit-and-backlog-20261111`).
- A commit subject is a plain sentence or two saying what changes for people, area first when it helps. No conventional-commit prefixes, no em dashes. Credit an AI that helped write a commit with a `Co-Authored-By:` trailer: `/about` and `/credits` promise it.
- Every change reaches main through a PR merged with a merge commit.
- A PR body says what changes for people, any migration (number, whether the previous release still runs on it, numbers skipped), the checks run, what was not checked, and what needs Joe.
- Land a stack of PRs through one integration PR off main: full CI on the combination, merge it, then merge each stacked PR into its own base in order, so GitHub marks it merged.
- Merging to main can publish: a change under `apps/mobile` or to `apps/web/lib/factions.ts` releases a public Android test APK.

## The closure rule

- Closed modules change for bug fixes and for what Joe asks, audits and polish included. Don't polish or extend them on your own; suggest it instead.
- Don't start a parked or unscheduled system, even partially, unless Joe asks: merch, the rest of Fastmarch, the native OBS plugin, third-party ads, or anything a spec marks Proposed or not scheduled.
- Before building, read the spec in `docs/`. Work is done when it passes the spec's "Done when", with tests covering it. A module or system closes when Joe accepts it; record that in its spec and `docs/ROADMAP.md`.

## Open source

This repo is public and licensed AGPL-3.0.

- **Everything is public except secrets, anti-abuse tuning and the private files above.** Tuning loads at runtime from private server config; the repo ships safe example values.
- **Write every commit and PR as if the world reads it.** No secrets, real user data, internal hostnames or server addresses, or tuning values in code, tests, commits, PR text or docs; only deployable config under `infra/` may hold a server path. Pushing publishes, so scan before every push (`gitleaks git --redact .`, or push with `scripts/publish.ps1`).
- **CI's secret scan reads every branch,** so a fake secret on any branch fails every PR. Use obviously fake, low-entropy test values. For a confirmed false positive, add its `commit:path:rule:line` fingerprint to `.gitleaksignore` on main.
- **The CLA, issue templates and PR template are for outside contributors.** They never slow work Joe asks for: no waiting on reviews or extra approvals.
- **Dependabot:** merge its patch, minor and GitHub Actions bumps when CI passes, after regenerating `/credits` on the branch. A breaking bump (a new major, or a new 0.x minor for a Rust crate) is a normal task, not a click-merge. Dependabot skips `apps/mobile`, Docker images and the Rust toolchain; bump the toolchain in `rust-toolchain.toml`, `.github/workflows/ci.yml` and `infra/api.Dockerfile` together.

## Legacy code

The first S.V.E.R was retired on October 21, 2026; its code is frozen in its own repositories (tag `legacy-web-final`). Read it only to learn proven behavior; never copy its code, config or secrets, or import from it. Its data was imported once: never rerun the importers against the live database.

## Production safety

- **sver.tv is production:** real accounts and real money (Stripe live mode). There is no staging site; `compose.stage.yaml` and "stage" in older notes mean production. Never send test email there. Ask Joe before creating test accounts, streams or purchases on it; append-only ledgers keep whatever a test earns.
- **One change per release.** Deploy each change as its own release and verify it before the next. Deploys run from Git Bash on Joe's PC, never PowerShell, with a helper kept outside the repo: `SOURCE=<commit> bash ~/SVER-dev/deploy.sh <release> <previous-release>`, with releases named `<topic>-<yyyymmdd>`. It backs up and verifies the database, rehearses migrations on a restored copy, and activates with automatic rollback. Run it in the background: it takes minutes, and stopping it before prep ends skips activation. Then check the change itself on sver.tv.
- **Deploy what production runs, plus your change.** Production often runs PR commits that main lacks. Right before you start, read the release production runs from the server's current-release link (`docs/OPERATIONS.md` names it), then its commit (`git get-tar-commit-id < ~/SVER-dev/<release>/source.tar`) and the migrations it has applied. Deploy that commit plus your PR, not main's merge commit, unless main equals it. Never run two deploys at once; use each release name once.
- Deploy only pushed work: your PR head, or a local merge of it onto production's commit. Merge a deployed PR promptly so main keeps matching production.
- **The helper ships only the API and web containers: their images and `compose.stage.yaml` entries.** Nginx, the media server, backup units, server env files and the Plays runtime change by hand on the server (`nginx -t` before any reload); record each change and its rollback in `docs/OPERATIONS.md`. After a new or changed socket route ships, open a real socket through sver.tv with the site's `Origin` header; health checks don't cover sockets.
- Back up and verify the database by hand before any production data change outside a release. A dump's sealed data reads back only with its matching `ENCRYPTION_KEY`, which is backed up separately. Never change that key.
- Nightly backups and their offsite copy must keep working. If you touch the backup script, its lock or its permissions, confirm the next nightly dump and the offsite pull both succeed.
- Production secrets and private tuning live in root-only server files outside the release directories, plus a private backup; `docs/OPERATIONS.md` says where. Treat everything in `%USERPROFILE%\SVER-dev\` as secret too. Never put any of it in the repo, a release, a PR or a log; compare by hash, never print it.
- Passwords, stream keys, tokens (token URLs included) and 2FA setup or recovery values never go in logs.

## Write the least code (Ponytail)

Adapted from Ponytail (github.com/dietrichgebert/ponytail, MIT). Lazy means efficient, not careless: the best code is code never written. First understand the task and trace the real flow end to end, then stop at the first rung that holds:

1. Does this need to be built at all?
2. Does it already exist in this repo? Reuse the helper or pattern that's here.
3. Does the standard library do it?
4. Does a native platform feature (Postgres, the browser, Next.js, Axum) cover it?
5. Does an already-installed dependency solve it?
6. Can it be one line?
7. Only then: write the minimum code that works.

- Bug fixes go to the root cause. Find every caller of the function you touch and fix the shared function once, not just the path the report names.
- No abstractions, boilerplate or new dependencies nobody asked for. Deletion over addition, boring over clever, fewest files possible.
- Shortest working diff wins, but only once you understand the problem. A small change in the wrong place is a second bug.
- When a request looks bigger than it needs to be, ask: "Do you need X, or does Y cover it?" (this counts as your one suggested improvement).
- Mark a deliberate shortcut with a known ceiling (global lock, O(n²) scan, naive heuristic) with a `ponytail:` comment naming the ceiling and the upgrade path.
- Never lazy about: input validation, permissions, error handling that prevents data loss, security, accessibility, money ledgers, or anything Joe asked for explicitly. The quality bar and the closure rule still apply in full.

## Quality bar

- Main has no branch protection: merge only when CI (`.github/workflows/ci.yml`) is green.
- Before you push, run the checks for each app you touched:
  - API: in `apps/api`, `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`; then from the repo root, `pwsh -File scripts/dev.ps1 database` and `pwsh -File scripts/dev.ps1 test`. The test script loads `%USERPROFILE%\SVER-dev\login.env`; a bare `cargo test` fails without that environment and a local `sver_rebuild` database. Without `login.env` (a cloud session), run `cargo test` with the environment the CI `api` job sets, or leave it to CI and say so. FFmpeg and ffprobe must be installed.
  - Web, in `apps/web`: `pnpm typecheck`, `pnpm test`, `pnpm lint` (no new warnings), `pnpm build`. `pnpm build` and `pnpm dev` rewrite `next-env.d.ts`; don't commit that.
  - Mobile, in `apps/mobile`: `pnpm typecheck`, `pnpm test`, `pnpm lint`, `pnpm bundle`.
- Tests are deterministic: they never depend on today's date, timing or run order.
- TypeScript is strict in web and mobile. No `any`: web lint rejects it. If one is unavoidable, disable the rule on that line and say why.
- Every endpoint validates input and checks permissions.
- Accessible markup: real buttons and links, labels on inputs, keyboard reachable.
- UI changes follow `docs/design/README.md`: check each changed page at 1440 px and 390 px beside its mockup and include the screenshots it asks for (say so when your tools can't attach them). To check a web-only change against live data, run `pnpm dev` in `apps/web` with `API_INTERNAL_ORIGIN=https://sver.tv` (signed out, read-only).

## When a task is done

- Log finished work in `ChangeLog.md` at the root of the main checkout, even when you work in a worktree. Append an entry under a dated heading (America/New_York): what Joe asked, what changed, checks, PR and release, what's left. Never rewrite old entries; append a correction. No secrets or personal data. A session that can't reach the file puts a public-safe version of those notes in its PR.
- Reply with: what changed for people, how you verified it and what you could not, whether it is merged and deployed (PR number, release name), anything waiting on Joe, and what you logged. Keep it short.
