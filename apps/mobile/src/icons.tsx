import type { ReactNode } from "react";
import type { ColorValue } from "react-native";
import Svg, { Circle, Path } from "react-native-svg";

// The website's chrome icons (apps/web/components/shell/Icons.tsx, docs/DESIGN.md "Icons"): 12-unit strokes drawn at about 20 px.
// One glyph per meaning across the app. The ones the website lacks are drawn in the same style.
type Props = { color: ColorValue; size?: number };
const svg = (color: ColorValue, size: number, children: ReactNode, filled = false) => (
  <Svg width={size} height={size} viewBox="0 0 12 12" fill={filled ? color : "none"} stroke={color} strokeWidth={1} strokeLinecap="round" strokeLinejoin="round">{children}</Svg>
);

export const HomeIcon = ({ color, size = 20 }: Props) => svg(color, size, <Path d="M1.5 6 6 2l4.5 4M3 5v5h6V5" />);
export const FollowingIcon = ({ color, size = 20 }: Props) => svg(color, size, <Path d="M6 10.2 1.8 6.1a2.3 2.3 0 0 1 3.3-3.2L6 3.8l.9-.9a2.3 2.3 0 0 1 3.3 3.2z" />);
export const BeaconsIcon = ({ color, size = 20 }: Props) => svg(color, size, <><Path d="M3.5 1h5v10h-5z" /><Path d="M5.3 4.5v3l2.5-1.5z" /></>);
export const FactionIcon = ({ color, size = 20 }: Props) => svg(color, size, <Path d="M2.5 11V1.5h7L8 3.75 9.5 6h-7" />);
export const BrowseIcon = ({ color, size = 20 }: Props) => svg(color, size, <Path d="M1.5 1.5h3.5v3.5H1.5zM7 1.5h3.5v3.5H7zM1.5 7h3.5v3.5H1.5zM7 7h3.5v3.5H7z" />);
export const SearchIcon = ({ color, size = 20 }: Props) => svg(color, size, <><Circle cx="5" cy="5" r="3.5" /><Path d="M7.6 7.6 11 11" /></>);
export const ChatIcon = ({ color, size = 20 }: Props) => svg(color, size, <Path d="M1.5 2.5h9v6h-5l-2.5 2v-2h-1.5z" />);
export const CloseIcon = ({ color, size = 20 }: Props) => svg(color, size, <Path d="m2.5 2.5 7 7M9.5 2.5l-7 7" />);
export const CheckIcon = ({ color, size = 20 }: Props) => svg(color, size, <Path d="m2 6.5 2.5 2.5L10 3" />);
export const MoreIcon = ({ color, size = 20 }: Props) => svg(color, size, <><Circle cx="2.5" cy="6" r="1" /><Circle cx="6" cy="6" r="1" /><Circle cx="9.5" cy="6" r="1" /></>, true);
export const HeartIcon = ({ color, size = 20, filled = false }: Props & { filled?: boolean }) => svg(color, size, <Path d="M6 10.2 1.8 6.1a2.3 2.3 0 0 1 3.3-3.2L6 3.8l.9-.9a2.3 2.3 0 0 1 3.3 3.2z" />, filled);
export const ArrowIcon = ({ color, size = 20, dir = "right" }: Props & { dir?: "up" | "down" | "left" | "right" }) =>
  svg(color, size, <Path d="M2 6h8M7 3l3 3-3 3" transform={`rotate(${{ right: 0, down: 90, left: 180, up: 270 }[dir]} 6 6)`} />);
export const ChevronLeftIcon = ({ color, size = 20 }: Props) => svg(color, size, <Path d="M7.5 2.5 4 6l3.5 3.5" />);
export const ChevronDownIcon = ({ color, size = 20 }: Props) => svg(color, size, <Path d="M2.5 4.5 6 8l3.5-3.5" />);
export const SendIcon = ({ color, size = 20 }: Props) => svg(color, size, <Path d="M1.5 5.5 10.5 1.5 6.5 10.5 5.5 6.5zM5.5 6.5l5-5" />);
/** Sound on. */
export const VolumeIcon = ({ color, size = 20 }: Props) => svg(color, size, <><Path d="M1.5 4.5h2l3-2.5v8l-3-2.5h-2z" /><Path d="M8.5 4.3a2.4 2.4 0 0 1 0 3.4M9.9 2.9a4.4 4.4 0 0 1 0 6.2" /></>);
/** Sound off (the player's mute). */
export const MutedIcon = ({ color, size = 20 }: Props) => svg(color, size, <><Path d="M1.5 4.5h2l3-2.5v8l-3-2.5h-2z" /><Path d="m8.3 4.6 2.7 2.8M11 4.6 8.3 7.4" /></>);
export const ExpandIcon = ({ color, size = 20 }: Props) => svg(color, size, <Path d="M1.5 4.5v-3h3M7.5 1.5h3v3M10.5 7.5v3h-3M4.5 10.5h-3v-3" />);
export const ContractIcon = ({ color, size = 20 }: Props) => svg(color, size, <Path d="M4.5 1.5v3h-3M10.5 4.5h-3v-3M7.5 10.5v-3h3M1.5 7.5h3v3" />);
/** Report. A pennant, so it never reads as the faction banner. */
export const FlagIcon = ({ color, size = 20 }: Props) => svg(color, size, <Path d="M2.5 11V1.5M2.5 2c2-1 3.5 1 6 0v4.5c-2.5 1-4-1-6 0" />);
