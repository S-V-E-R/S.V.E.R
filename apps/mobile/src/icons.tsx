import type { ReactNode } from "react";
import type { ColorValue } from "react-native";
import Svg, { Path } from "react-native-svg";

// The website's chrome icons (apps/web/components/shell/Icons.tsx, docs/DESIGN.md "Icons"): 12-unit strokes drawn at about 20 px.
type Props = { color: ColorValue; size?: number };
const svg = (color: ColorValue, size: number, children: ReactNode) => (
  <Svg width={size} height={size} viewBox="0 0 12 12" fill="none" stroke={color} strokeWidth={1} strokeLinecap="round" strokeLinejoin="round">{children}</Svg>
);

export const HomeIcon = ({ color, size = 20 }: Props) => svg(color, size, <Path d="M1.5 6 6 2l4.5 4M3 5v5h6V5" />);
export const FollowingIcon = ({ color, size = 20 }: Props) => svg(color, size, <Path d="M6 10.2 1.8 6.1a2.3 2.3 0 0 1 3.3-3.2L6 3.8l.9-.9a2.3 2.3 0 0 1 3.3 3.2z" />);
export const BeaconsIcon = ({ color, size = 20 }: Props) => svg(color, size, <><Path d="M3.5 1h5v10h-5z" /><Path d="M5.3 4.5v3l2.5-1.5z" /></>);
export const FactionIcon = ({ color, size = 20 }: Props) => svg(color, size, <Path d="M2.5 11V1.5h7L8 3.75 9.5 6h-7" />);
