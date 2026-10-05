"use client";
import { FormEvent, useEffect, useRef, useState } from "react";
import Link from "next/link";
import { Turnstile } from "../components/Turnstile";
import { isFaction } from "../lib/factions";
import { Avatar } from "../components/Avatar";
import type { Sizes } from "../lib/types";

type Config = { providers: string[]; turnstile_site_key: string; development: boolean };
type Me = { username: string; email: string; email_verified: boolean; mfa_enabled: boolean; has_password: boolean; providers: string[]; session_id: string; reauthenticated: boolean; recovery_codes_remaining: number; deletion_due: string | null };
type Session = { id: string; user_agent: string; last_seen_at: string; created_at: string };
type Reply = { error?: string; requires_mfa?: boolean; url?: string; secret?: string; uri?: string; recovery_codes?: string[] };
declare global { interface Window { turnstile?: { render(el: HTMLElement, options: Record<string, unknown>): string; remove(id: string): void } } }

async function request<T = Reply>(path: string, data?: unknown, method?: string): Promise<T> {
  const response = await fetch(`/api/auth/${path}`, { method: method || (data === undefined ? "GET" : "POST"), headers: data === undefined ? {} : { "Content-Type": "application/json" }, body: data === undefined ? undefined : JSON.stringify(data), credentials: "same-origin", cache: "no-store" });
  const result = await response.json().catch(() => ({ error: "The service could not be reached. Please try again." }));
  if (!response.ok) throw new Error(result.error || "The request could not be completed.");
  return result as T;
}
function Field({ label, name, type = "text", autoComplete, minLength, maxLength, hint, required = true, pattern }: { label: string; name: string; type?: string; autoComplete?: string; minLength?: number; maxLength?: number; hint?: string; required?: boolean; pattern?: string }) {
  return <label className="field" htmlFor={name}><span>{label}</span><input id={name} name={name} type={type} autoComplete={autoComplete} required={required} minLength={minLength} maxLength={maxLength} pattern={pattern} aria-describedby={hint ? `${name}-hint` : undefined} />{hint && <small id={`${name}-hint`}>{hint}</small>}</label>;
}
function UsernameField({ initialUsername = "" }: { initialUsername?: string }) {
  const [username, setUsername] = useState(initialUsername);
  const [feedback, setFeedback] = useState<{ available?: boolean; message: string }>();
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (!username) return;
    if (!/^[A-Za-z0-9_]{3,25}$/.test(username)) {
      // eslint-disable-next-line react-hooks/set-state-in-effect -- format feedback shares the debounced availability state
      setFeedback({ available: false, message: "Use 3–25 letters, numbers or underscores." });
      return;
    }
    const controller = new AbortController();
    const timer = setTimeout(async () => {
      try {
        const response = await fetch(`/api/auth/username-availability?username=${encodeURIComponent(username)}`, { signal: controller.signal, cache: "no-store" });
        const result = await response.json();
        if (!response.ok) throw new Error(result.error || "Could not check availability. Please try again.");
        if (!controller.signal.aborted && input.current?.value === username) {
          setFeedback(result);
          input.current.setCustomValidity(result.available ? "" : result.message);
        }
      } catch (error) {
        if (!controller.signal.aborted) setFeedback({ message: error instanceof Error ? error.message : "Could not check availability. Please try again." });
      }
    }, 350);
    return () => { clearTimeout(timer); controller.abort(); };
  }, [username]);
  return <label className="field" htmlFor="username"><span>Username</span><input ref={input} id="username" name="username" autoComplete="username" required minLength={3} maxLength={25} pattern="[A-Za-z0-9_]{3,25}" value={username} aria-describedby="username-hint username-status" aria-invalid={feedback?.available === false || undefined} onChange={event => { setUsername(event.target.value); setFeedback(event.target.value ? { message: "Checking availability…" } : undefined); event.target.setCustomValidity(""); }} /><small id="username-hint">3–25 letters, numbers or underscores. Availability is confirmed when your account is created.</small><small id="username-status" className={`username-status ${feedback?.available === true ? "available" : feedback?.available === false ? "unavailable" : ""}`} role="status" aria-live="polite" aria-atomic="true">{feedback?.message}</small></label>;
}
const providerNames: Record<string, string> = { google: "Google", twitch: "Twitch", discord: "Discord" };
function ProviderIcon({ name }: { name: string }) {
  return <span className={`provider-icon ${name}`} aria-hidden="true">{name === "google" ? "G" : name === "twitch" ? "T" : "D"}</span>;
}
export default function AuthScreen({ screen }: { screen: string }) {
  const [config, setConfig] = useState<Config>();
  const [me, setMe] = useState<Me>();
  const [profile, setProfile] = useState<{ display_name: string; avatar: Sizes }>();
  const [sessions, setSessions] = useState<Session[]>([]);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [botToken, setBotToken] = useState("");
  const [botKey, setBotKey] = useState(0);
  const [linkToken, setLinkToken] = useState("");
  const [setup, setSetup] = useState<Reply>();
  const [recovery, setRecovery] = useState<string[]>();
  const [primaryPassword, setPrimaryPassword] = useState("");
  const [mfaCode, setMfaCode] = useState("");
  const [deleteConfirmed, setDeleteConfirmed] = useState(false);
  const [pendingSignup, setPendingSignup] = useState<{ provider: string; username: string }>();
  const form = useRef<HTMLFormElement>(null);

  async function loadAccount() {
    const user = await request<Me>("me"); setMe(user);
    // The channel avatar (Module 2). Best effort: without it the monogram stays.
    fetch("/api/me/profile", { credentials: "same-origin", cache: "no-store" }).then(r => r.ok ? r.json() : undefined).then(p => p && setProfile({ display_name: p.display_name, avatar: p.avatar ?? null })).catch(() => {});
    if (!user.deletion_due) setSessions((await request<{ sessions: Session[] }>("sessions")).sessions);
  }
  useEffect(() => {
    request<Config>("config").then(setConfig).catch(e => setError(e.message));
    const params = new URLSearchParams(window.location.hash.slice(1));
    // eslint-disable-next-line react-hooks/set-state-in-effect -- the link token is read from the URL hash, which only exists on the client
    setLinkToken(params.get("token") || "");
    const preferred = new URLSearchParams(window.location.search).get("faction");
    if (isFaction(preferred)) sessionStorage.setItem("preferred-faction", preferred);
    const providerError = new URLSearchParams(window.location.search).get("error");
    if (providerError) setError(providerError);
    if (window.location.hash || providerError) history.replaceState(null, "", window.location.pathname);
    if (screen === "account") loadAccount().catch(e => setError(e.message));
    if (screen === "oauth-signup") request<{ provider: string; username: string }>("oauth/signup").then(setPendingSignup).catch(e => setError(e.message));
  }, [screen]);

  async function run(action: () => Promise<void>) {
    setBusy(true); setError(""); setMessage("");
    try { await action(); } catch (e) { setError(e instanceof Error ? e.message : "Please try again."); }
    finally { setBusy(false); setBotToken(""); setBotKey(k => k + 1); }
  }
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const values = Object.fromEntries(new FormData(event.currentTarget));
    await run(async () => {
      let result: Reply;
      switch (screen) {
        case "signup": result = await request("signup", { ...values, turnstile_token: botToken }); window.location.assign("/choose-side"); break;
        case "oauth-signup": await request("oauth/signup", { ...values, turnstile_token: botToken }); window.location.assign("/choose-side"); break;
        case "login": result = await request("login", values); window.location.assign(result.requires_mfa ? "/mfa" : "/"); break;
        case "mfa": await request("mfa/login", values); window.location.assign("/"); break;
        case "forgot": await request("password/forgot", { ...values, turnstile_token: botToken }); setMessage("If that account exists, a recovery email has been queued. Check your inbox and spam folder."); break;
        case "reset": await request("password/reset", { ...values, token: linkToken }); setLinkToken(""); setMessage("Your password has been changed. Sign in again to continue."); break;
        case "verify": await request("email/verify", { token: linkToken }); setLinkToken(""); setMessage("Your email is verified. You’re ready for your next chapter."); break;
      }
    });
  }
  async function oauth(name: string, intent: string) {
    await run(async () => {
      if (intent === "link") await confirmPrimary();
      const result = await request(`oauth/${name}/start`, { intent, code: mfaCode });
      if (result.url) window.location.assign(result.url);
    });
  }
  async function confirmPrimary() {
    if (primaryPassword) { await request("reauth", { password: primaryPassword }); setPrimaryPassword(""); }
  }
  async function securityAction(path: string) {
    await run(async () => {
      await confirmPrimary();
      const result = await request(path, { code: mfaCode }); setMfaCode("");
      if (path === "mfa/setup") setSetup(result);
      if (result.recovery_codes) { setRecovery(result.recovery_codes); setSetup(undefined); }
      if (path === "mfa/disable") { setSetup(undefined); setRecovery(undefined); }
      if (path === "account/delete") { window.location.assign("/login"); return; }
      await loadAccount();
      if (path !== "mfa/setup") setMessage("Account security updated.");
    });
  }
  const titles: Record<string, [string, string]> = {
    login: ["Log in", "Welcome back."],
    signup: ["Enlist", "Create your account. Watching is free and needs no account."],
    "oauth-signup": ["Finish enlisting", "Finish creating your S.V.E.R account."],
    forgot: ["Reset your password", "We’ll email you a link to set a new password."],
    reset: ["Set a new password", "Choose a new password for your account."],
    verify: ["Confirm your email", "Confirm the email address for your S.V.E.R account."],
    mfa: ["Two-factor check", "Enter an authenticator code or an unused recovery code."],
    account: ["Account security", "Your sign-in methods, two-factor setup and devices."]
  };
  const notice = <div className="notices" aria-live="polite" aria-atomic="true">{error && <p className="notice error" role="alert">{error}</p>}{message && <p className="notice success">{message}</p>}</div>;

  if (screen === "account") return <div className="account-page"><div className="eyebrow">Account</div><h1>{titles.account[0]}</h1><p className="intro">{titles.account[1]}</p>{notice}
    {!me ? <p className="loading">{error ? "Account details are unavailable. Please retry or sign in again." : "Opening your account…"}</p> : me.deletion_due ? <section className="panel"><span className="eyebrow">DELETION GRACE PERIOD</span><h2>Your account is scheduled for deletion.</h2><p>It will be erased after {new Date(me.deletion_due).toLocaleString()}. Your account is restricted during this period.</p><button disabled={busy} onClick={() => run(async () => { await request("account/restore", {}); await loadAccount(); setMessage("Deletion cancelled. Welcome back."); })}>Keep my account</button><button className="quiet" disabled={busy} onClick={() => run(async () => { await request("logout", {}); window.location.assign("/login"); })}>Sign out</button></section> : <>
      <section className="identity-panel panel">{profile?.avatar ? <Avatar sizes={profile.avatar} name={profile.display_name || me.username} size={66} /> : <div className="avatar" aria-hidden="true">{me.username.slice(0, 1).toUpperCase()}</div>}<div><span className="eyebrow">YOUR S.V.E.R IDENTITY</span><h2>@{me.username}</h2><p>{me.email}</p></div><span className={`badge ${me.email_verified ? "verified" : ""}`}>{me.email_verified ? "EMAIL VERIFIED" : "EMAIL UNVERIFIED"}</span>{!me.email_verified && <button className="quiet" disabled={busy} onClick={() => run(async () => { await request("email/resend", {}); setMessage("Verification email queued. Check your inbox."); })}>Resend verification</button>}</section>
      <div className="account-grid"><section className="panel"><span className="eyebrow">01 / IDENTITY CHECK</span><h2>Confirm it’s you</h2><p>Security changes need a sign-in confirmation from the last five minutes{me.mfa_enabled ? " and a fresh authenticator or recovery code" : ""}.</p>
        {me.has_password ? <form onSubmit={e => { e.preventDefault(); run(async () => { await confirmPrimary(); await loadAccount(); setMessage("Identity confirmed for five minutes."); }); }}><label className="field" htmlFor="primary-password"><span>Current password</span><input id="primary-password" type="password" autoComplete="current-password" value={primaryPassword} onChange={e => setPrimaryPassword(e.target.value)} required /></label><button className="quiet" disabled={busy || !primaryPassword}>Confirm password</button></form> : <div className="stack">{me.providers.map(p => <button className="quiet" disabled={busy} key={p} onClick={() => oauth(p, "reauth")}>Confirm with {providerNames[p]}</button>)}</div>}
        {me.reauthenticated && <p className="small-text">Primary sign-in recently confirmed.</p>}
        {me.mfa_enabled && <label className="field" htmlFor="security-code"><span>Authenticator or recovery code</span><input id="security-code" autoComplete="one-time-code" value={mfaCode} onChange={e => setMfaCode(e.target.value)} maxLength={64} /><small>Use a fresh code for each security change.</small></label>}
      </section><section className="panel"><span className="eyebrow">02 / SECOND FACTOR</span><h2>Authenticator security <span className={`badge ${me.mfa_enabled ? "verified" : ""}`}>{me.mfa_enabled ? "ON" : "OFF"}</span></h2><p>Add an authenticator app to protect your account. Required before you can access streaming credentials.</p>
        {!me.mfa_enabled && !setup && <button disabled={busy} onClick={() => securityAction("mfa/setup")}>Set up authenticator</button>}
        {setup?.secret && <div className="setup"><p>In your authenticator app, add a time-based account named S.V.E.R and enter this setup key:</p><code className="secret">{setup.secret}</code><a href={setup.uri}>Open authenticator app</a><form onSubmit={e => { e.preventDefault(); const code = new FormData(e.currentTarget).get("code"); run(async () => { const r = await request("mfa/enable", { code }); setRecovery(r.recovery_codes); setSetup(undefined); await loadAccount(); }); }}><Field label="Six-digit authenticator code" name="code" autoComplete="one-time-code" pattern="[0-9]{6}" maxLength={6} /><button disabled={busy}>Enable authenticator</button></form></div>}
        {me.mfa_enabled && <><p>{me.recovery_codes_remaining} recovery codes remaining.</p><div className="stack"><button className="quiet" disabled={busy} onClick={() => securityAction("mfa/recovery")}>Generate new recovery codes</button><button className="quiet danger-text" disabled={busy} onClick={() => securityAction("mfa/disable")}>Disable authenticator</button></div></>}
        {recovery && <div className="recovery"><h3>Save these recovery codes</h3><p>Each code works once. They will not be shown again. Store them somewhere safe before leaving.</p><textarea aria-label="Recovery codes to save" readOnly value={recovery.join("\n")} rows={10} /><button className="quiet" onClick={() => setRecovery(undefined)}>I’ve saved my codes</button></div>}
      </section><section className="panel"><span className="eyebrow">03 / CONNECTIONS</span><h2>Your sign-in methods</h2><p>{me.has_password ? "Email and password is enabled." : "You use a provider to sign in. Password recovery can also add a password after you prove mailbox ownership."}</p><div className="connections">{Object.entries(providerNames).map(([p, name]) => <div className="connection" key={p}><ProviderIcon name={p} /><span>{name}<small>{me.providers.includes(p) ? "Connected" : config?.providers.includes(p) ? "Not connected" : "Not configured yet"}</small></span><button className="quiet small" disabled={busy || (!me.providers.includes(p) && !config?.providers.includes(p))} onClick={() => me.providers.includes(p) ? run(async () => { await confirmPrimary(); await request(`oauth/${p}`, { code: mfaCode }, "DELETE"); setMfaCode(""); await loadAccount(); setMessage(`${name} disconnected.`); }) : oauth(p, "link")}>{me.providers.includes(p) ? "Unlink" : "Connect"}</button></div>)}</div><Link href="/forgot">Reset or add a password</Link></section>
      <section className="panel"><span className="eyebrow">04 / DEVICES</span><h2>Active sessions</h2><p>Only keep devices you recognize.</p><div className="sessions">{sessions.map(s => <div className="session" key={s.id}><div><strong>{s.id === me.session_id ? "This device" : "Another device"}</strong><small title={s.user_agent}>{s.user_agent}</small><small>Active {new Date(s.last_seen_at).toLocaleString()}</small></div><button className="quiet small" disabled={busy} onClick={() => run(async () => { await request(`sessions/${s.id}`, undefined, "DELETE"); if (s.id === me.session_id) window.location.assign("/login"); else await loadAccount(); })}>Revoke</button></div>)}</div><button className="quiet" disabled={busy} onClick={() => run(async () => { await request("sessions", undefined, "DELETE"); window.location.assign("/login"); })}>Sign out everywhere</button></section></div>
      <section className="panel deletion"><div><span className="eyebrow">ACCOUNT CONTROL</span><h2>Delete your account</h2><p>Start a 14-day grace period. You can sign in and cancel during that time. Afterward, your account is erased.</p><label className="checkbox"><input type="checkbox" checked={deleteConfirmed} onChange={e => setDeleteConfirmed(e.target.checked)} /> I understand and want to schedule deletion.</label></div><button className="danger" disabled={busy || !deleteConfirmed} onClick={() => securityAction("account/delete")}>Schedule deletion</button></section>
      <button className="quiet signout" disabled={busy} onClick={() => run(async () => { await request("logout", {}); window.location.assign("/login"); })}>Sign out of this device</button>
    </>}</div>;

  const isSignup = screen === "signup" || screen === "oauth-signup";
  const botRequired = isSignup || screen === "forgot";
  const withProviders = screen === "login" || screen === "signup";
  const providerButtons = withProviders && <div className="providers">{Object.entries(providerNames).map(([p, name]) => <button type="button" className="provider" key={p} disabled={busy || !config?.providers.includes(p)} title={!config?.providers.includes(p) ? `${name} sign-in is not configured yet` : undefined} onClick={() => oauth(p, screen === "signup" ? "signup" : "login")}><ProviderIcon name={p} /><span>Continue with {name}</span></button>)}{config && !config.providers.length && <p className="provider-note">Google, Twitch and Discord sign-in arrive once those connections are configured.</p>}</div>;
  return <div className="entry-page">
    {isSignup && <SignupSteps current={1} />}
    <section className="auth-panel frame" aria-labelledby="auth-title"><h1 id="auth-title" className="auth-title">{titles[screen][0]}</h1><p className="intro">{titles[screen][1]}</p>{notice}
      {screen === "signup" && <>{providerButtons}<div className="divider"><span>or with email</span></div></>}
      {screen === "oauth-signup" && !pendingSignup && <p className="small-text">{error ? <Link href="/signup">Start signup again</Link> : "Checking your provider sign-in…"}</p>}
      {(screen !== "oauth-signup" || pendingSignup) && <form ref={form} onSubmit={submit}>
        {pendingSignup && <p className="small-text">Connected with {providerNames[pendingSignup.provider]}. Confirm your username and age to finish.</p>}
        {screen === "signup" && <Field label="Email" name="email" type="email" autoComplete="email" maxLength={320} />}
        {isSignup && <UsernameField initialUsername={pendingSignup?.username} />}
        {screen === "login" && <Field label="Email or username" name="identifier" autoComplete="username" maxLength={320} />}
        {screen === "forgot" && <Field label="Email address" name="email" type="email" autoComplete="email" maxLength={320} />}
        {["login", "signup", "reset"].includes(screen) && <Field label={screen === "reset" ? "New password" : "Password"} name="password" type="password" autoComplete={screen === "login" ? "current-password" : "new-password"} minLength={screen === "login" ? undefined : 10} maxLength={screen === "login" ? 4096 : 128} hint={screen === "login" ? undefined : "At least 10 characters."} />}
        {screen === "login" && <p className="form-meta"><Link href="/forgot">Forgot password?</Link></p>}
        {isSignup && <Field label="Date of birth" name="date_of_birth" type="date" autoComplete="bday" hint="You must be 13 or older. Never shown on your profile." />}
        {screen === "mfa" && <Field label="Authenticator or recovery code" name="code" autoComplete="one-time-code" maxLength={64} />}
        {botRequired && config && <Turnstile key={botKey} sitekey={config.turnstile_site_key} action={isSignup ? "signup" : "recovery"} onToken={setBotToken} />}
        {(screen === "reset" || screen === "verify") && !linkToken && !message && <p className="notice">Open the link from your email to continue.</p>}
        <button className="primary" disabled={busy || (botRequired && !botToken) || ((screen === "reset" || screen === "verify") && !linkToken)}>{busy ? "Please wait…" : ({ login: "Log in", signup: "Continue", "oauth-signup": "Continue", forgot: "Send recovery link", reset: "Set new password", verify: "Verify email", mfa: "Confirm code" })[screen]}</button>
      </form>}
      {isSignup && <p className="signup-policy">By continuing you agree to the <Link href="/terms">Terms</Link> and <Link href="/guidelines">Community Guidelines</Link>, and you&apos;ve read the <Link href="/privacy">Privacy Policy</Link>.</p>}
      {screen === "login" && <><div className="divider"><span>or</span></div>{providerButtons}</>}
      <div className="form-footer">{screen === "login" ? <>New here? <Link href="/signup">Enlist</Link></> : isSignup ? <>Already enlisted? <Link href="/login">Log in</Link></> : <Link href="/login">Back to log in</Link>}</div>
    </section></div>;
}

/** The three sign-up steps from the sign-up mockup: Account, Choose your side, Confirm email. */
export function SignupSteps({ current }: { current: 1 | 2 | 3 }) {
  return <ol className="signup-steps" aria-label="Sign-up steps">{["Account", "Choose your side", "Confirm email"].map((label, i) =>
    <li key={label} className={i + 1 === current ? "on" : undefined} aria-current={i + 1 === current ? "step" : undefined}><span className="hex">{i + 1}</span>{label}</li>)}</ol>;
}
