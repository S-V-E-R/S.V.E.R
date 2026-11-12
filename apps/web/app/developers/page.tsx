import type { Metadata } from "next";
import Link from "next/link";
import SitePage from "../../components/SitePage";

export const metadata: Metadata = {
  title: "Developers | S.V.E.R",
  description: "Build apps, bots, overlays and tools on S.V.E.R: sign-in, scopes, the API, live events and webhooks.",
  alternates: { canonical: "https://sver.tv/developers" },
};

// Mirrors devapps.rs SCOPES and events.rs PUBLIC/PRIVATE (docs/DEVELOPER_PLATFORM.md); update both together.
const scopes = [
  ["user:read", "See the person's username, display name and faction"],
  ["channel:read", "See their channel's stream settings"],
  ["chat:read", "Read chat as them"],
  ["chat:write", "Send chat messages as them"],
  ["channel:moderate", "Moderate their channel's chat"],
  ["channel:edit", "Change their stream's title and category"],
  ["channel:run", "Run raids, polls, predictions, highlight markers and the board's pause"],
  ["events:private", "Receive their channel's private events, such as subscriptions and tributes"],
  ["board:control", "Control their CrowdSync board like a connected game (setting the whole board also needs their “May set my board”)"],
  ["whispers:read", "Read their direct messages as they arrive"],
] as const;

const publicTopics = [
  ["channel:{name}:live", "Went live or offline"],
  ["channel:{name}:update", "Title, category and profile changes"],
  ["channel:{name}:follows", "The follower count, never who followed"],
  ["channel:{name}:raids", "Raids coming in"],
  ["channel:{name}:costream", "Co-stream members"],
  ["channel:{name}:board", "CrowdSync presses, results and board changes"],
  ["channel:{name}:poll", "A poll's start, votes and close"],
  ["channel:{name}:prediction", "A prediction's start, votes and close"],
  ["channel:{name}:surge", "Surge levels"],
  ["faction:{slug}:war", "Weekly checkpoint results"],
  ["chat:{name}", "Chat messages and deletions"],
] as const;

const privateTopics = [
  ["channel:{name}:follows:detail", "Who followed"],
  ["channel:{name}:subs", "New, renewed and gifted subscriptions"],
  ["channel:{name}:tributes", "Tributes"],
  ["channel:{name}:skills", "Skills used"],
  ["channel:{name}:moderation", "The moderation log"],
  ["channel:{name}:raid:incoming", "A raid's countdown starting"],
] as const;

const calls = [
  ["GET /api/v1/me", "The signed-in person: username, display name and faction (user:read)"],
  ["GET /api/v1/channels/{name}", "A channel: live, title, category, faction and followers (client ID only)"],
  ["PATCH /api/me/stream", "Change the title and category (channel:edit)"],
  ["POST /api/me/raids", "Start a raid (channel:run)"],
  ["POST /api/channels/{name}/polls", "Start a poll or prediction (channel:run)"],
  ["POST /api/channels/{name}/polls/{id}/close", "Close one (channel:run)"],
  ["POST /api/channels/{name}/marker", "Add a highlight marker (channel:run)"],
  ["PUT /api/channels/{name}/board/disabled", "Pause or resume the CrowdSync board (channel:run)"],
] as const;

const sections = [
  ["start", "Get started"], ["sign-in", "Sign-in"], ["scopes", "Scopes"], ["api", "API"],
  ["events", "Live events"], ["webhooks", "Webhooks"], ["catalog", "CrowdSync catalog"], ["rules", "Rules"],
] as const;

function Table({ head, rows }: { head: [string, string]; rows: readonly (readonly [string, string])[] }) {
  return <div className="dev-table"><table>
    <thead><tr><th scope="col">{head[0]}</th><th scope="col">{head[1]}</th></tr></thead>
    <tbody>{rows.map(([a, b]) => <tr key={a}><td><code>{a}</code></td><td>{b}</td></tr>)}</tbody>
  </table></div>;
}

/** Public developer docs (docs/DEVELOPER_PLATFORM.md, as built). /dev redirects here. */
export default function DevelopersPage() {
  return <SitePage path="/developers" title="Developers" intro="Build apps, bots, overlays and tools for S.V.E.R. Sign people in, read channels, run their stream's actions and react to live events as they happen." wide>
    <div className="policy-layout">
      <nav className="policy-contents" aria-label="On this page">
        <h2>On this page</h2>
        <ol>{sections.map(([id, title]) => <li key={id}><a href={`#${id}`}>{title}</a></li>)}</ol>
      </nav>
      <div className="policy-text">
        <section id="start">
          <h2>Get started</h2>
          <ol>
            <li>Verify your email and turn on two-factor sign-in.</li>
            <li>Register an app in <Link href="/settings/developer">Settings → Developer</Link>. You can have up to 10. Each one gets a public client ID.</li>
            <li>Add redirect URIs: <code>https://</code>, or <code>http://127.0.0.1</code> on any port for desktop apps.</li>
            <li>Server apps can create a client secret. It&apos;s shown once, so store it safely. You can rotate it at any time.</li>
          </ol>
          <p>Every API call sends your client ID in an <code>SVER-Client-Id</code> header, including public reads.</p>
        </section>

        <section id="sign-in">
          <h2>Sign-in</h2>
          <p>S.V.E.R uses OAuth 2.1: authorization code with PKCE (<code>S256</code>) for every app, and no implicit flow. Send people to:</p>
          <pre><code>{`https://sver.tv/oauth/authorize?response_type=code
  &client_id=YOUR_CLIENT_ID
  &redirect_uri=https://example.com/callback
  &scope=user:read chat:read
  &state=RANDOM
  &code_challenge=BASE64URL_SHA256_OF_VERIFIER
  &code_challenge_method=S256`}</code></pre>
          <p>They see your app&apos;s name, icon and owner, and each scope in plain words. If they allow it, you get a <code>code</code> that works once and lasts 10 minutes. Exchange it at <code>POST https://sver.tv/api/oauth/token</code> (form-encoded):</p>
          <pre><code>{`grant_type=authorization_code&code=…&redirect_uri=…&client_id=…&code_verifier=…
(server apps also send client_secret=…)

→ {"access_token":"…","token_type":"Bearer","expires_in":3600,
   "refresh_token":"…","scope":"user:read chat:read"}`}</code></pre>
          <p>Access tokens last 1 hour. Refresh tokens last 60 days and work once: <code>grant_type=refresh_token</code> returns a new pair. Reusing an old refresh token revokes the whole grant. <code>POST /api/oauth/revoke</code> ends a grant, and people can disconnect your app at any time in Settings → Connected apps.</p>
          <h3>Devices without a browser</h3>
          <p>For desktop, TV and console apps, use device sign-in (RFC 8628). <code>POST /api/oauth/device</code> with <code>client_id</code> and <code>scope</code> returns a 6-character code to show, like <code>BCD-FGH</code>. The person enters it at <Link href="/go">sver.tv/go</Link>. Poll the token endpoint with <code>grant_type=urn:ietf:params:oauth:grant-type:device_code</code> no faster than every 5 seconds. Codes expire after 10 minutes.</p>
          <h3>CrowdSync games</h3>
          <p>A game connects to a streamer&apos;s board the same way: device sign-in with the <code>board:control</code> scope, then the board gateway at <code>wss://sver.tv/api/integrations/ws</code> with the access token and your client ID. Register the game without a client secret; the client ID is public and is all a game needs. The board SDKs for JavaScript, Unity and Unreal do this for you: give them the client ID, show the code they hand you, and they keep and renew the sign-in. On the approval screen the streamer can tick &ldquo;May set my board&rdquo; to let the game bring its own board. A browser game can call the device, token and revoke endpoints from its own site.</p>
        </section>

        <section id="scopes">
          <h2>Scopes</h2>
          <p>Ask only for what your app needs. Scopes are separated by spaces.</p>
          <Table head={["Scope", "Lets your app"]} rows={scopes} />
        </section>

        <section id="api">
          <h2>API</h2>
          <p>The base URL is <code>https://sver.tv/api</code>. Send <code>SVER-Client-Id</code> on every call, and <code>Authorization: Bearer ACCESS_TOKEN</code> when acting as someone.</p>
          <Table head={["Call", "What it does"]} rows={calls} />
          <p>Each app and person can make 600 calls a minute. Every response includes <code>RateLimit-Limit</code>, <code>RateLimit-Remaining</code> and <code>RateLimit-Reset</code>. Over the limit, you get <code>429</code> with <code>Retry-After</code>.</p>
        </section>

        <section id="events">
          <h2>Live events</h2>
          <p>Use one WebSocket instead of polling:</p>
          <pre><code>{`wss://sver.tv/api/events?client_id=YOUR_CLIENT_ID&access_token=OPTIONAL

→ {"type":"method","id":1,"method":"subscribe","params":{"topics":["channel:joe:live"]}}
← {"type":"reply","id":1,…}
← {"type":"event","id":42,"topic":"channel:joe:live","data":{"live":true},"at":"…"}`}</code></pre>
          <p>Other methods are <code>unsubscribe</code>, <code>ping</code>, <code>history</code> (the last 50 chat messages) and <code>send</code> (chat, with <code>chat:write</code>). After a reconnect, subscribe again with <code>since</code> set to your last event ID to replay the past 5 minutes. A connection can have up to 200 topics, and each app and person up to 10 connections.</p>
          <h3>Public topics (client ID only)</h3>
          <Table head={["Topic", "Events"]} rows={publicTopics} />
          <h3>Private topics (owner or moderator, with events:private)</h3>
          <Table head={["Topic", "Events"]} rows={privateTopics} />
          <p>With a person&apos;s own token, you can also subscribe to <code>{"user:{name}:notifications"}</code> (<code>user:read</code>) and <code>{"user:{name}:whispers"}</code> (<code>whispers:read</code>).</p>
        </section>

        <section id="webhooks">
          <h2>Webhooks</h2>
          <p>Get the same events posted to your server. <code>POST /api/hooks</code> with <code>{`{"topics":[…],"url":"https://…"}`}</code> returns a signing secret once. <code>GET /api/hooks</code> lists your hooks and <code>{"DELETE /api/hooks/{id}"}</code> removes one. You can also manage them in Settings → Developer. Each hook takes up to 50 topics, and each person can have 20 hooks. Apps register hooks with the person&apos;s bearer token.</p>
          <p>Each delivery is the event JSON with an <code>SVER-Signature: t=TIMESTAMP,v1=HEX</code> header, where <code>v1</code> is the HMAC-SHA256 of <code>TIMESTAMP.BODY</code> with your secret. Check it, and reject old timestamps. Failed deliveries are retried 8 times with growing gaps. After 50 failures in a row the hook turns off and you get a notification. Private and local addresses are refused.</p>
        </section>

        <section id="catalog">
          <h2>CrowdSync catalog</h2>
          <p>The <Link href="/crowdsync">catalog</Link> is where streamers find CrowdSync games and ready-made boards. Each app can have one listing. Send it in from <Link href="/settings/developer">Settings → Developer</Link>, under the app.</p>
          <ul>
            <li><strong>What a listing has:</strong> a name (3 to 40 characters), a one-line summary (up to 120), a description (up to 2,000), an <code>https://</code> link to the game&apos;s store page or site, your app&apos;s icon and up to 4 screenshots.</li>
            <li><strong>The board:</strong> either a board streamers can add to their library (your draft, a board from your library or a <code>.sverboard.json</code> file), or &ldquo;this game sets its own board&rdquo; for a game that defines it when connected. Effects that use your own Skills become &ldquo;none&rdquo;, because other channels don&apos;t have them.</li>
            <li><strong>Review:</strong> staff review every new listing and every change before it appears. While a change waits, the listing stays up as it was. If staff reject or remove a listing, you see the reason and can send it in again.</li>
            <li><strong>What&apos;s allowed:</strong> nothing banned by the <Link href="/guidelines">Community Guidelines</Link>; no gambling or betting; nothing that lets a viewer pay for an advantage over other viewers; no one else&apos;s name, art or trademarks without permission; a link that leads to the game or its maker; screenshots of the actual game or board.</li>
            <li><strong>Order:</strong> listings are shown newest first and searched by name. There is no ranking by spending and no paid placement.</li>
            <li><strong>Counts:</strong> a listing shows how many channels added its board (or, for a game with its own board, how many connected it). It never shows who.</li>
            <li><strong>Your name:</strong> the listing shows your display name and links to your channel. Under-18 developers aren&apos;t named.</li>
            <li>Images go through the same checks as every upload, and a <Link href="/take-it-down">Take It Down request</Link> removes them. Suspending an app hides its listing.</li>
          </ul>
        </section>

        <section id="rules">
          <h2>Rules</h2>
          <ul>
            <li>Apps can never move money: no Valor spending, tributes, Skills, payouts or purchases.</li>
            <li>Events and responses never include email, IP address or internal IDs. Under-18 accounts never appear by name in public events.</li>
            <li>Follow the <Link href="/terms">Terms</Link> and <Link href="/guidelines">Community Guidelines</Link>. Staff can suspend an app that breaks them.</li>
            <li>SDKs for JavaScript, Unity and Unreal, the OBS bridge and the Stream Deck plugin are open source on <a href="https://github.com/S-V-E-R/S.V.E.R/tree/main/integrations">GitHub</a>. Questions? <Link href="/contact">Get in touch</Link>.</li>
          </ul>
        </section>
      </div>
    </div>
  </SitePage>;
}
