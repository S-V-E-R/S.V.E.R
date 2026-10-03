import Link from "next/link";

const policyLinks = [
  ["/terms", "Terms"], ["/privacy", "Privacy"],
  ["/guidelines", "Guidelines"], ["/dmca", "Copyright & DMCA"],
] as const;

export default function SitePage({ path, title, intro, children, policy = false, wide = false }: {
  path: string;
  title: string;
  intro: string;
  children: React.ReactNode;
  policy?: boolean;
  wide?: boolean;
}) {
  return <div className={`site-page ${policy ? "policy-page" : "info-page"}`}>
    <header className="site-page-heading">
      <div className="eyebrow">S.V.E.R / {policy ? "POLICIES" : "COMMUNITY & SUPPORT"}</div>
      <h1>{title}</h1>
      <p className="site-intro">{intro}</p>
      {policy && <>
        <p className="site-updated">Last updated <time dateTime="2026-10-03">October 3, 2026</time> · Effective <time dateTime="2026-10-03">October 3, 2026</time></p>
        <nav className="site-links policy-tabs" aria-label="Policies">{policyLinks.map(([href, label]) =>
          <Link key={href} href={href} aria-current={path === href ? "page" : undefined}>{label}</Link>)}</nav>
      </>}
    </header>
    <article className={`site-document${wide ? " site-document-wide" : ""}`} aria-label={title}>{children}</article>
    {!policy && <section className="site-cta frame" aria-labelledby="site-cta-title">
      <div><h2 id="site-cta-title">{path === "/contact" ? "Manage your account" : "Set up your S.V.E.R account"}</h2>
        <p>{path === "/contact" ? "Review your sign-in methods, active sessions and security settings." : "Create a profile, follow channels and manage your account security."}</p></div>
      <Link href={path === "/contact" ? "/account" : "/signup"} className="button">{path === "/contact" ? "Account security" : "Enlist"}</Link>
    </section>}
  </div>;
}

export function PolicyPage({ path, title, intro, summary, sections }: {
  path: string;
  title: string;
  intro: string;
  summary: string[];
  sections: { title: string; content: React.ReactNode }[];
}) {
  return <SitePage path={path} title={title} intro={intro} policy>
    <section className="policy-summary frame" aria-labelledby="short-version">
      <h2 id="short-version">The short version</h2>
      <ul>{summary.map(point => <li key={point}>{point}</li>)}</ul>
      <p className="muted">This summary is a guide. The full text below is what applies.</p>
    </section>
    <div className="policy-layout">
      <nav className="policy-contents" aria-label="On this page">
        <h2>On this page</h2>
        <ol>{sections.map((section, index) => <li key={section.title}><a href={`#section-${index + 1}`}>{section.title}</a></li>)}</ol>
      </nav>
      <div className="policy-text">{sections.map((section, index) =>
        <section key={section.title} id={`section-${index + 1}`}>
          <h2>{index + 1}. {section.title}</h2>{section.content}
        </section>)}</div>
    </div>
  </SitePage>;
}
