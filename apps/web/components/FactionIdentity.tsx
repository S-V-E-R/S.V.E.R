import Image from "next/image";
import { factionInfo, type Faction } from "../lib/factions";

export function Crest({ faction, size = 32 }: { faction: Faction; size?: number }) {
  return <Image src={`/factions/${faction}.webp`} width={size} height={size} alt={factionInfo(faction).name} className="identity-crest" unoptimized />;
}
export function EnrollmentSteps({ step }: { step: 1 | 2 | 3 }) {
  return <ol className="enrollment-steps" aria-label="Signup steps">{["Account", "Choose your side", "Confirm email"].map((name, i) => <li key={name} aria-current={step === i + 1 ? "step" : undefined}><span>{i + 1}</span>{name}</li>)}</ol>;
}
