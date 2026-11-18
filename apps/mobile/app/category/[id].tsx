import { useCallback, useState } from "react";
import { router, useLocalSearchParams } from "expo-router";
import { send } from "../../src/api";
import { useAuth } from "../../src/auth";
import { StreamCard } from "../../src/cards";
import { useLoad } from "../../src/load";
import { streamCards, type StreamCard as Card } from "../../src/types";
import { CardSkeleton, EmptyState, HeaderBar, Notice, Page } from "../../src/ui";

/** One category's live streams, in the API's fair rotation order (docs/DESIGN.md "Browse"). */
export default function CategoryScreen() {
  const { id, name } = useLocalSearchParams<{ id: string; name?: string }>();
  const { account } = useAuth();
  const title = name || "Category";
  const [items, setItems] = useState<Card[] | null>(null);
  const [minute, setMinute] = useState(0);
  const [error, setError] = useState("");

  const load = useCallback(async () => {
    const result = await send<{ items?: unknown }>("GET", `/api/discovery/live?category=${encodeURIComponent(id)}`);
    if (!result.ok) {
      setError(result.error);
      return;
    }
    setItems(streamCards(result.data.items));
    setMinute(Math.floor(Date.now() / 60000));
    setError("");
  }, [id]);
  useLoad(load);

  const back = () => (router.canGoBack() ? router.back() : router.replace("/browse"));

  return (
    <Page header={<HeaderBar title={title} onBack={back} />} onRefresh={load}>
      {error ? <Notice onRetry={() => { void load(); }}>{error}</Notice> : null}
      {!items ? (
        error ? null : <><CardSkeleton /><CardSkeleton /></>
      ) : items.length === 0 ? (
        <EmptyState title={`Nothing live in ${name || "this category"} right now.`} action={{ label: "Back to Browse", onPress: () => router.navigate("/browse") }} />
      ) : (
        items.map(stream => <StreamCard key={stream.broadcast_id} stream={stream} size="full" minute={minute} viewerFaction={account?.faction ?? null} />)
      )}
    </Page>
  );
}
