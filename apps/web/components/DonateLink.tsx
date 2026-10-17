"use client";

/** A streamer-supplied charity link: nobody at S.V.E.R reviews it, so the viewer is told where it goes first. */
export function DonateLink({ url, className, children }: { url: string; className?: string; children: React.ReactNode }) {
  let host = url;
  try { host = new URL(url).hostname; } catch { /* shown as written */ }
  return <a className={className} href={url} target="_blank" rel="noopener noreferrer" onClick={e => {
    if (!window.confirm(`S.V.E.R hasn't verified this link. It opens ${host}, a site run by someone else. Only donate if you trust it.`)) e.preventDefault();
  }}>{children}</a>;
}
