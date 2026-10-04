/* eslint-disable @next/next/no-img-element -- already-sized, immutable media bucket variants */
export type ChannelEmote = { id: string; code: string; image: { "28": string; "56": string; "112": string } };
export function EmoteImage({ emote, size = 28 }: { emote: ChannelEmote; size?: 28 | 112 }) {
  return <img className="chat-emote" src={emote.image[size]} srcSet={size === 28 ? `${emote.image["56"]} 2x, ${emote.image["112"]} 4x` : undefined} width={size} height={size} style={{ width: size, height: size }} alt={emote.code} title={emote.code} loading="lazy" />;
}
