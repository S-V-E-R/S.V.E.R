import type { Sizes } from "../lib/types";

/** Avatar with srcset; null renders the default monogram tile. Alt text is the display name. */
export function Avatar({ sizes, name, size = 160 }: { sizes: Sizes; name: string; size?: number }) {
  if (!sizes) return <span className="avatar default" style={{ width: size, height: size }} role="img" aria-label={name}>{name.slice(0, 1).toUpperCase()}</span>;
  const set = Object.entries(sizes).map(([w, url]) => `${url} ${w}w`).join(", ");
  const src = sizes["160"] || Object.values(sizes)[0];
  // Inline size, like the monogram above, so a global .avatar width rule cannot override the size asked for.
  // eslint-disable-next-line @next/next/no-img-element
  return <img className="avatar" src={src} srcSet={set} sizes={`${size}px`} width={size} height={size} style={{ width: size, height: size }} alt={name} />;
}
