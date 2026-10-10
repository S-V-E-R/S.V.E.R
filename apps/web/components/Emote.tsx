/* eslint-disable @next/next/no-img-element -- already-sized, immutable media bucket variants */
export type ChannelEmote = { id: string; code: string; image: { "28": string; "56": string; "112": string }; tier?: number | null; provider?: OutsideProvider };
export type OutsideProvider = "7tv" | "bttv" | "ffz";
export const providerNames: Record<OutsideProvider, string> = { "7tv": "7TV", bttv: "BTTV", ffz: "FFZ" };
/** Channel emotes are square; 7TV, BTTV and FFZ emotes keep their own width at the same height. */
export function EmoteImage({ emote, size = 28 }: { emote: ChannelEmote; size?: 28 | 112 }) {
  const wide = !!emote.provider;
  return <img className={wide ? "chat-emote chat-emote-wide" : "chat-emote"} src={emote.image[size]} srcSet={size === 28 ? `${emote.image["56"]} 2x, ${emote.image["112"]} 4x` : undefined} width={wide ? undefined : size} height={size} style={wide ? { height: size } : { width: size, height: size }} alt={emote.code} title={emote.provider ? `${emote.code} (${providerNames[emote.provider]})` : emote.code} loading="lazy" />;
}
