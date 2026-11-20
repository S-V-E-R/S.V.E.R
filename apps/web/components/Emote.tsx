/* eslint-disable @next/next/no-img-element -- already-sized, immutable media bucket variants */
type Sizes = { "28": string; "56": string; "112": string };
/** `owner` and `faction` are set on another channel's signature emote (`username/Code`). `still` is an animated outside emote's first frame. */
export type ChannelEmote = { id: string; code: string; image: Sizes; still?: Sizes | null; tier?: number | null; provider?: OutsideProvider; owner?: string; faction?: string | null };
export type OutsideProvider = "7tv" | "bttv" | "ffz";
export const providerNames: Record<OutsideProvider, string> = { "7tv": "7TV", bttv: "BTTV", ffz: "FFZ" };
const srcSet = (image: Sizes, size: 28 | 112) => size === 28 ? `${image["56"]} 2x, ${image["112"]} 4x` : undefined;
/** Channel emotes are square; 7TV, BTTV and FFZ emotes keep their own width at the same height. Viewers who reduce motion see an animated one's first frame. */
export function EmoteImage({ emote, size = 28 }: { emote: ChannelEmote; size?: 28 | 112 }) {
  const wide = !!emote.provider;
  const img = <img className={wide ? "chat-emote chat-emote-wide" : "chat-emote"} src={emote.image[size]} srcSet={srcSet(emote.image, size)} width={wide ? undefined : size} height={size} style={wide ? { height: size } : { width: size, height: size }} alt={emote.code} title={emote.provider ? `${emote.code} (${providerNames[emote.provider]})` : emote.code} loading="lazy" />;
  if (!emote.still) return img;
  return <picture><source media="(prefers-reduced-motion: reduce)" srcSet={size === 28 ? `${emote.still["28"]}, ${srcSet(emote.still, 28)}` : emote.still["112"]} />{img}</picture>;
}
