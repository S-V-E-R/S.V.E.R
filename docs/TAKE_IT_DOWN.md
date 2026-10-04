# Take It Down: removal of intimate images

Specified October 4, 2026. Required by the federal TAKE IT DOWN Act, whose platform rules took effect May 19, 2026 and are enforced by the FTC (civil penalties per violation). sver.tv is public and accepts uploads (avatars, banners, wall posts, fan art, emotes) and live video, so S.V.E.R is a covered platform. **This is built immediately**, ahead of the remaining Live streams work. A lawyer reviews the page text and the process before or right after it goes live.

## What it covers

Nonconsensual intimate images and videos of a real, identifiable person, including **digital forgeries** (made or altered with software or AI), anywhere on S.V.E.R: live streams, profile images, banners, wall posts, fan art, emotes, chat links, and later clips, VODs, Beacons and direct messages.

Intimate content of a **minor** is child sexual abuse material: S.V.E.R removes it immediately, preserves it as the law requires, and reports it to NCMEC's CyberTipline. That path applies on top of this one.

## The public request page

- **`sver.tv/take-it-down`**, linked as **"Take It Down requests"** in the footer of every page (including the homepage) and from the Guidelines, Terms, Privacy Policy and Help.
- Plain-language explanation of what can be reported, what happens next and how long it takes, in the design system's legal-page layout.
- **Works without an account.** Protected by Turnstile.
- A "Report → Intimate image (Take It Down)" option on every live stream, profile, wall post, fan art item and chat message opens the same form with the location filled in.

### What the form asks for (the four parts the law requires)

1. **Who is asking:** "I am the person shown" or "I am authorized to act for them" (with how).
2. **Where the content is:** one or more S.V.E.R links, plus anything that helps find it (username, time in a stream, description). **Never** asks for a copy of the image itself.
3. **Good-faith statement:** a checkbox and short text: "I believe in good faith that this content was shared without consent of the person shown."
4. **Contact details:** email (required), name.
5. **Signature:** typing their full name as an electronic signature, with the date.

Optional: anything else that helps (other places it appears).

### After submitting

- A **request number** (for example `TID-2026-000123`) is shown and emailed.
- A **status page** (request number plus email) shows: received, under review, removed (with time), or not removed with the reason.
- The requester is emailed at each step.

## What happens inside S.V.E.R

- **Immediate:** content S.V.E.R can identify from the link is **hidden right away** while the request is reviewed, and a live stream named in the request is stopped by staff on review (live streams can't be "hidden" without stopping). Requests jump to the top of the staff queue in `/admin` with a **48-hour countdown** from receipt.
- **Alerts:** every new request emails and pushes to all staff immediately; if it is still open at 24 hours, alerts repeat hourly. The 48 hours run on weekends and holidays.
- **Review:** staff check the request has the four required parts and that the content matches. A request is valid even if the requester has no account. Staff can contact the requester for more location details; the clock keeps running, so content stays hidden meanwhile.
- **Removal:** valid requests: the content is **removed** (not just hidden) and its stored files deleted, except copies the law requires to be preserved for minors. Done within 48 hours of receipt, and in practice as soon as possible.
- **Identical copies:** S.V.E.R stores a fingerprint (hash) of removed images and video frames, searches existing uploads for matches, removes known identical copies without a new request, and blocks future uploads that match. Later: share fingerprints with StopNCII.org (adults) and NCMEC's Take It Down service (minors).
- **Invalid or mistaken requests:** content hidden in error is restored, the requester is told why, and the uploader is not penalized. Knowingly false requests break the Terms.
- **The uploader:** told the content was removed under the Take It Down process. Posting nonconsensual intimate content is a **level-three strike** that opens an account-ban review (the Profiles and Live streams moderation rules). The uploader can appeal the strike, but valid removals are never reversed.

## Records

Every request keeps: request number, received time, requester contact, the four parts, content located, actions with times and staff, outcome, and notices sent. Kept for 3 years (confirmed October 4, 2026), access limited to staff, and used for a monthly compliance check (requests received, median and longest time to removal, any over 48 hours).

## Policy text to add

- Community Guidelines: nonconsensual intimate content and sexual deepfakes of real people are banned.
- Terms: the Take It Down process, false requests, strikes.
- Privacy Policy: what a request stores and for how long.
- Help: "How do I get an intimate image removed?"

## Done when

The footer link appears on every page; a signed-out visitor submits a request with the four required parts and gets a request number and email; the content is hidden immediately and staff are alerted; staff remove it with the 48-hour countdown visible; an identical copy uploaded elsewhere is found and removed, and a re-upload is blocked; the requester and uploader are notified; the status page shows the outcome; records are complete; an invalid request restores the content.

## Implementation status — October 4, 2026

The request workflow is implemented locally, pending deployment and live acceptance; playback authorization has shipped separately, as described below. Migration 0014 adds encrypted request records, action and email-delivery records, reversible media holds, image fingerprints and staff push jobs. Staff access uses the existing admin role and MFA rules; decisions and evidence viewing require a recent sign-in. Ordinary completed records expire three years after receipt. Valid cases involving a minor retain a legal hold and require a CyberTipline/preservation reference before closure. Reporting to CyberTipline is a staff action, not an automated submission.

Migration 0015 adds durable notification outcomes. Requester, staff and uploader emails take priority and retry for up to seven days; login emails keep their existing retry limit. Security resets retain these removal notices. Staff push attempts expire after two days, with hourly reminders for unresolved requests after the first 24 hours. The staff case record shows pending, retrying, accepted, failed and expired notices after queue cleanup. Provider acceptance does not establish receipt or reading. Missing email or push destinations are recorded, and staff follow up through an available channel. Saved preservation references and minor flags survive subsequent review actions.

The direct playback proxy now checks current stream authorization on every HLS playlist/segment and new WHEP request. Staff stopping a stream revokes its playback URLs immediately, including saved URLs for files still in SRS's rolling buffer. Responses use `no-store`. The private check requires the existing media-proxy secret and peer address, validates the media path, and refuses retired keys, ended streams and expired reconnect windows. It uses Nginx's [auth_request module](https://nginx.org/en/docs/http/ngx_http_auth_request_module.html); the API and playback proxy configuration must ship together, with the API activated first. Future CDN delivery must enforce equivalent revocation before it is enabled.

Playback authorization was deployed independently on October 4, without new migrations. Public checks confirmed denial of inactive HLS playlists, segment GET/HEAD requests and new WHEP requests, `no-store` responses, and rejection of public access to the private authorization endpoint. Health, the public roadmap and other services were preserved. A disposable Linux rehearsal verified both allowed live playback and denied revoked playback with the release image. These checks do not establish successful production browser playback or acceptance of the undeployed request workflow.

The isolated SRS/FFmpeg acceptance test now covers anonymous live-stream reporting, staff review, publisher disconnect, saved HLS URL denial, WHEP denial, final requester status and rejection of republishing. Its receipt is [stream-ingest-local.json](stream-ingest-local.json). This is direct local playback verification; it does not establish browser, CDN, capacity or production acceptance.

Image matching covers original upload bytes, decoded pixels and generated variants across the current avatar, banner, song, sponsor, setup and fan-art pipelines. Historical uploads can only be indexed from their surviving published files. Cropped or otherwise edited copies are not perceptual matches. Future video uploads and live video-frame matching still require media-pipeline work; live requests currently use the existing stream-stop process. Do not treat this as complete video fingerprint coverage.

Acceptance still needs a working production cache-purge credential, end-to-end staff email/push and browser checks, and review of the legal text/process. Internal service accounts keep their existing exemption from strikes; their content is removed and a staff-access review is recorded.
