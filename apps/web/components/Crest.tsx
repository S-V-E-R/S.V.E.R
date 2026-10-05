import Image from "next/image";
import { crestSrc, factionOf } from "../lib/factions";

/** A faction crest, or a neutral initial before the account has chosen a side. */
export function Crest({ faction, initial, size = 36, label }: { faction: string | null; initial: string; size?: number; label?: string }) {
  const f = factionOf(faction);
  if (!f) return <span className="crest-slot" style={{ width: size, height: size, fontSize: Math.round(size / 2.1) }} aria-hidden={label ? undefined : true} aria-label={label}>{initial}</span>;
  return <Image src={crestSrc(f.slug)} width={size} height={size} alt={label ?? ""} className="crest" unoptimized />;
}
