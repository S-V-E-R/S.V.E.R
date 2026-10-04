export type RoadmapItem = {
  id: string;
  number: number;
  name: string;
  detail: string;
  status: "Done" | "In progress" | "Started" | "Planned";
};
export type Roadmap = { revision: string; items: RoadmapItem[] };

export function parseRoadmap(value: unknown): Roadmap {
  if (!value || typeof value !== "object" || !("revision" in value) || typeof value.revision !== "string"
    || !("items" in value) || !Array.isArray(value.items) || value.items.length !== 10) throw new Error("Roadmap unavailable");
  const ids = new Set<string>();
  for (const item of value.items) {
    if (!item || typeof item !== "object" || typeof item.id !== "string" || !/^[a-z-]+$/.test(item.id)
      || ids.has(item.id) || item.number !== ids.size || typeof item.name !== "string" || !item.name
      || typeof item.detail !== "string" || !item.detail || !["Done", "In progress", "Started", "Planned"].includes(item.status)) throw new Error("Roadmap unavailable");
    ids.add(item.id);
  }
  return value as Roadmap;
}
