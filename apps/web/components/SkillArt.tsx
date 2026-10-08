/**
 * Built-in art for the Skills catalog (apps/api/crates/sver/src/skills.rs). Flat SVG in the
 * channel's accent and a few fixed colors; no glows or images, so tiles stay light.
 */
const ART: Record<string, React.ReactNode> = {
  crown: <><path d="M10 44 6 18l14 12 12-18 12 18 14-12-4 26Z" fill="#F2C14E" stroke="#8A5A12" strokeWidth="2.5" strokeLinejoin="round" /><rect x="10" y="44" width="44" height="8" rx="2" fill="#E0A82E" stroke="#8A5A12" strokeWidth="2.5" /><circle cx="32" cy="36" r="4" fill="var(--accent)" /></>,
  heart: <path d="M32 54S8 40 8 24a12 12 0 0 1 24-4 12 12 0 0 1 24 4c0 16-24 30-24 30Z" fill="#E5484D" stroke="#7A1E22" strokeWidth="2.5" strokeLinejoin="round" />,
  trophy: <><path d="M20 10h24v14a12 12 0 0 1-24 0Z" fill="#F2C14E" stroke="#8A5A12" strokeWidth="2.5" /><path d="M20 14h-8a8 8 0 0 0 8 10M44 14h8a8 8 0 0 1-8 10" fill="none" stroke="#8A5A12" strokeWidth="2.5" /><path d="M28 36h8v8h-8Z" fill="#E0A82E" stroke="#8A5A12" strokeWidth="2.5" /><rect x="20" y="44" width="24" height="8" rx="2" fill="var(--accent)" stroke="#8A5A12" strokeWidth="2.5" /></>,
  starfall: <><path d="m24 8 4 9 10 1-7 7 2 10-9-5-9 5 2-10-7-7 10-1Z" fill="#F2C14E" stroke="#8A5A12" strokeWidth="2" strokeLinejoin="round" /><path d="m46 26 2.5 5.5 6 .5-4.5 4 1.5 6-5.5-3-5.5 3 1.5-6-4.5-4 6-.5Z" fill="var(--accent)" stroke="#8A5A12" strokeWidth="2" strokeLinejoin="round" /><path d="M10 46l8-6M20 54l8-6M36 56l6-5" stroke="#F2C14E" strokeWidth="3" strokeLinecap="round" /></>,
  quake: <><path d="M6 40h12l4-10 6 20 6-28 6 22 4-8h14" fill="none" stroke="var(--accent)" strokeWidth="4" strokeLinejoin="round" strokeLinecap="round" /><path d="M10 52h44" stroke="#8B6B4A" strokeWidth="4" strokeLinecap="round" /></>,
  fireworks: <><circle cx="22" cy="24" r="3" fill="#F2C14E" /><path d="M22 10v6M22 32v6M8 24h6M30 24h6M12 14l4 4M28 30l4 4M32 14l-4 4M16 30l-4 4" stroke="#F2C14E" strokeWidth="3" strokeLinecap="round" /><circle cx="44" cy="40" r="3" fill="var(--accent)" /><path d="M44 28v6M44 46v6M32 40h6M50 40h6M36 32l4 4M48 44l4 4M52 32l-4 4M40 44l-4 4" stroke="var(--accent)" strokeWidth="3" strokeLinecap="round" /></>,
  chime: <><path d="M32 8a4 4 0 0 1 4 4v2a14 14 0 0 1 10 14v12l4 6H14l4-6V28a14 14 0 0 1 10-14v-2a4 4 0 0 1 4-4Z" fill="#F2C14E" stroke="#8A5A12" strokeWidth="2.5" strokeLinejoin="round" /><circle cx="32" cy="52" r="4" fill="var(--accent)" /></>,
  fanfare: <><path d="M8 26h10l22-12v36L18 38H8Z" fill="#F2C14E" stroke="#8A5A12" strokeWidth="2.5" strokeLinejoin="round" /><path d="M46 22c4 4 4 16 0 20M52 16c7 8 7 24 0 32" fill="none" stroke="var(--accent)" strokeWidth="3" strokeLinecap="round" /></>,
};

export function SkillArt({ id, size = 48 }: { id: string; size?: number }) {
  return <svg width={size} height={size} viewBox="0 0 64 64" aria-hidden="true" focusable="false" className="skill-art">
    {ART[id] ?? <circle cx="32" cy="32" r="20" fill="var(--accent)" />}
  </svg>;
}
