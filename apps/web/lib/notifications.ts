type Row = { kind: string; read: boolean; channel?: { username: string } };

/**
 * The notifications list shows one go-live row per channel: the newest, with how many there were
 * and unread if any of them is. Other notices pass through. `items` is newest first.
 */
export function collapseLive<T extends Row>(items: T[]): (T & { times: number })[] {
  const first = new Map<string, T & { times: number }>();
  const rows: (T & { times: number })[] = [];
  for (const item of items) {
    const channel = item.kind === "live" ? item.channel?.username : undefined;
    const row = channel ? first.get(channel) : undefined;
    if (row) { row.times++; row.read &&= item.read; continue; }
    const next = { ...item, times: 1 };
    if (channel) first.set(channel, next);
    rows.push(next);
  }
  return rows;
}
