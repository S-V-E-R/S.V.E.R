/** Setup categories for the parts picker (docs/PROFILES.md, "Setup parts picker"), in display order.
 *  OTHER holds entries kept from before the picker and can't be chosen for new items. */
export const SETUP_CATEGORIES = ["CPU", "GPU", "RAM", "MOTHERBOARD", "CAMERA", "MIC", "PERIPHERALS"] as const;
const LABELS: Record<string, string> = { CPU: "CPU", GPU: "GPU", RAM: "RAM", MOTHERBOARD: "Motherboard", CAMERA: "Camera", MIC: "Mic & audio interface", PERIPHERALS: "Peripherals", OTHER: "Other" };
/** Shown beside audio interfaces and mixers listed under Mic (Joe's decision, Oct 3 2026, 5:52 PM ET). */
export const kindLabel = (kind?: string | null) => kind === "AUDIO_INTERFACE" ? "Audio interface" : null;
export const setupLabel = (category: string) => LABELS[category] ?? category.toLowerCase().replaceAll("_", " ").replace(/^./, c => c.toUpperCase());
/** Items grouped in the picker's category order, "Other" last; owner order within a category. */
export function groupByCategory<T extends { category: string }>(items: T[]): [string, T[]][] {
  const order = [...SETUP_CATEGORIES, "OTHER"] as string[];
  const groups = new Map<string, T[]>();
  for (const item of items) groups.set(item.category, [...(groups.get(item.category) ?? []), item]);
  return [...groups.entries()].sort(([a], [b]) => (order.indexOf(a) + 1 || 99) - (order.indexOf(b) + 1 || 99));
}
