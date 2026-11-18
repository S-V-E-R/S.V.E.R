import { isFaction } from "../../web/lib/factions";
import { palette } from "./theme";

export function factionColor(faction: string | null | undefined): string {
  return isFaction(faction) ? palette[faction].accent : palette.neutral.accent;
}
