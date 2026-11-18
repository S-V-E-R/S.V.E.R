import { useCallback, useEffect, useRef, useState, type ComponentRef } from "react";
import { Image, Pressable, ScrollView, StyleSheet, Text, TextInput, View } from "react-native";
import { router, useLocalSearchParams } from "expo-router";
import { send } from "../../src/api";
import { useAuth } from "../../src/auth";
import { CloseIcon, SearchIcon } from "../../src/icons";
import { useLoad } from "../../src/load";
import { fill, font, hexAlpha, ink, inkDim, line, space } from "../../src/theme";
import { isBrowse, type Browse, type BrowseCategory, type SearchResult } from "../../src/types";
import { IconButton, Notice, Page, Panel, RowSkeleton, SectionHead, Skeleton, Tag, type } from "../../src/ui";
import { ChannelRow } from "../../src/cards";
import { absolute } from "../../src/urls";

const openCategory = (id: string, name: string) => router.push(`/category/${encodeURIComponent(id)}?name=${encodeURIComponent(name)}`);

/** docs/DESIGN.md "Browse": search at the top, then each genre's categories as a sideways rail. */
export default function BrowseScreen() {
  const { colors } = useAuth();
  const { focus } = useLocalSearchParams<{ focus?: string }>();
  const input = useRef<ComponentRef<typeof TextInput>>(null);
  const [browse, setBrowse] = useState<Browse | null>(null);
  const [error, setError] = useState("");
  const [query, setQuery] = useState("");
  const [found, setFound] = useState<SearchResult | null>(null);
  const [searching, setSearching] = useState(false);
  const [searchError, setSearchError] = useState("");
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const seq = useRef(0);

  const load = useCallback(async () => {
    const result = await send<unknown>("GET", "/api/discovery/browse");
    if (!result.ok || !isBrowse(result.data)) {
      setError(result.ok ? "Browse could not be loaded." : result.error);
      return;
    }
    setBrowse(result.data);
    setError("");
  }, []);
  useLoad(load);

  // Home's search icon opens Browse with ?focus=1. Clearing the param lets the next tap focus again.
  useEffect(() => {
    if (focus !== "1") return;
    input.current?.focus();
    router.setParams({ focus: undefined });
  }, [focus]);

  /** Searches after `delay` ms; a newer keystroke cancels the older request's result. */
  function search(text: string, delay: number) {
    clearTimeout(timer.current);
    const n = ++seq.current;
    const q = text.trim();
    setSearchError("");
    if (q.length < 2) {
      setFound(null);
      setSearching(false);
      return;
    }
    setSearching(true);
    timer.current = setTimeout(() => {
      void send<SearchResult>("GET", `/api/search?q=${encodeURIComponent(q)}`).then(result => {
        if (n !== seq.current) return;
        setSearching(false);
        if (result.ok) setFound(result.data);
        else setSearchError(result.error);
      });
    }, delay);
  }

  const genreName = (id: string) => browse?.genres.find(g => g.id === id)?.name ?? id;
  const searchMode = query.trim().length >= 2;
  const channels = found?.channels.filter(c => c.username) ?? [];

  return (
    <Page tabbed head={{ eyebrow: "Browse", title: "Browse", lead: "Discover live streams, creators and communities." }} onRefresh={load}>
      <View style={[styles.search, { borderColor: line(colors.accent) }]}>
        <SearchIcon color={inkDim} />
        <TextInput
          ref={input}
          value={query}
          onChangeText={text => { setQuery(text); search(text, 300); }}
          onSubmitEditing={() => search(query, 0)}
          placeholder="Search channels and categories"
          placeholderTextColor={inkDim}
          accessibilityLabel="Search channels and categories"
          autoCapitalize="none"
          autoCorrect={false}
          returnKeyType="search"
          maxLength={50}
          style={styles.input}
        />
        {query ? <IconButton icon={c => <CloseIcon color={c} />} label="Clear search" onPress={() => { setQuery(""); search("", 0); }} /> : null}
      </View>

      {searchMode ? (
        searchError ? <Notice onRetry={() => search(query, 0)}>{searchError}</Notice>
        : !found ? <View accessible accessibilityLabel="Loading">{[0, 1, 2, 3].map(i => <RowSkeleton key={i} />)}</View>
        : channels.length === 0 && found.categories.length === 0 ? <Text style={type.small}>{searching ? "Searching…" : "No channels or categories match."}</Text>
        : (
          <>
            {channels.length > 0 ? (
              <Panel>
                <Text style={type.label} accessibilityRole="header">Channels</Text>
                {channels.map(c => <ChannelRow key={c.username} user={c} meta={`@${c.username}`} onPress={() => router.push(`/watch/${c.username}`)} />)}
              </Panel>
            ) : null}
            {found.categories.length > 0 ? (
              <Panel>
                <Text style={type.label} accessibilityRole="header">Categories</Text>
                {found.categories.map(c => (
                  <Pressable key={c.id} accessibilityRole="button" accessibilityLabel={`${c.name}, ${genreName(c.genre)}`} onPress={() => openCategory(c.id, c.name)} style={({ pressed }) => [styles.categoryRow, pressed && styles.pressed]}>
                    <Text style={type.strong} numberOfLines={1}>{c.name}</Text>
                    <Text style={type.small} numberOfLines={1}>{genreName(c.genre)}</Text>
                  </Pressable>
                ))}
              </Panel>
            ) : null}
          </>
        )
      ) : error ? (
        <Notice onRetry={() => { void load(); }}>{error}</Notice>
      ) : !browse ? (
        <View style={styles.gap} accessible accessibilityLabel="Loading">
          {[0, 1].map(row => <View key={row} style={styles.rail}>{[0, 1, 2].map(i => <Skeleton key={i} w={112} h={149} />)}</View>)}
        </View>
      ) : (
        <>
          <View style={styles.start}><Tag label={`${browse.live.toLocaleString()} live now`} /></View>
          {browse.genres.map(genre => (
            <View key={genre.id} style={styles.gap}>
              <SectionHead title={genre.name} />
              <ScrollView horizontal showsHorizontalScrollIndicator={false} contentContainerStyle={styles.rail}>
                {/* Live categories first; API order inside each half. Never sorted by how many are live. */}
                {[...genre.categories.filter(c => c.live > 0), ...genre.categories.filter(c => c.live <= 0)].map(c => <CategoryTile key={c.id} category={c} />)}
              </ScrollView>
            </View>
          ))}
        </>
      )}
    </Page>
  );
}

/** A 3:4 category tile: art (or the name ghosted in Cinzel), the name, and "{n} live" when anything is. */
function CategoryTile({ category }: { category: BrowseCategory }) {
  const { colors } = useAuth();
  const art = category.image?.["300"];
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={category.live > 0 ? `${category.name}, ${category.live} live` : category.name}
      onPress={() => openCategory(category.id, category.name)}
      style={({ pressed }) => [styles.tile, pressed && styles.pressed]}
    >
      <View style={[styles.art, { backgroundColor: colors.accentDeep, borderColor: line(colors.accent) }]}>
        {art ? <Image source={{ uri: absolute(art) }} style={StyleSheet.absoluteFill} accessibilityIgnoresInvertColors /> : <Text style={styles.ghost} numberOfLines={1}>{category.name}</Text>}
      </View>
      <Text style={styles.name} numberOfLines={1}>{category.name}</Text>
      {category.live > 0 ? <Text style={[styles.count, { color: colors.accentLight }]}>{category.live.toLocaleString()} live</Text> : null}
    </Pressable>
  );
}

const styles = StyleSheet.create({
  search: { flexDirection: "row", alignItems: "center", minHeight: 44, borderWidth: 1, paddingLeft: space.m, gap: space.s, backgroundColor: fill },
  input: { flex: 1, minHeight: 44, color: ink, fontFamily: font.body, fontSize: 16 },
  categoryRow: { minHeight: 56, justifyContent: "center" },
  pressed: { opacity: 0.85 },
  gap: { gap: space.m },
  rail: { flexDirection: "row", gap: space.m },
  start: { alignSelf: "flex-start" },
  tile: { width: 112, gap: space.xs },
  art: { width: 112, aspectRatio: 3 / 4, borderWidth: 1, alignItems: "center", justifyContent: "center", overflow: "hidden" },
  ghost: { fontFamily: font.display, fontSize: 14, color: hexAlpha(ink, 0.12), paddingHorizontal: space.s },
  name: { fontFamily: font.bodySemi, fontSize: 13, color: ink },
  count: { fontFamily: font.label, fontSize: 12, letterSpacing: 1, textTransform: "uppercase" },
});
