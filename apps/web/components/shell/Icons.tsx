/* Inline SVG icons for the site chrome (docs/DESIGN.md "Icons"): 18 px strokes in currentColor. */
type Props = { size?: number };
const svg = (size: number, children: React.ReactNode) => <svg width={size} height={size} viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" focusable="false">{children}</svg>;

export const HomeIcon = ({ size = 18 }: Props) => svg(size, <path d="M1.5 6 6 2l4.5 4M3 5v5h6V5" />);
export const FollowingIcon = ({ size = 18 }: Props) => svg(size, <path d="M6 10.2 1.8 6.1a2.3 2.3 0 0 1 3.3-3.2L6 3.8l.9-.9a2.3 2.3 0 0 1 3.3 3.2z" />);
export const WalletIcon = ({ size = 18 }: Props) => svg(size, <><path d="M1.5 3.5h9v6h-9z" /><path d="M7.5 6.5h3" /></>);
export const StudioIcon = ({ size = 18 }: Props) => svg(size, <><path d="M1.5 3h6.5v6H1.5z" /><path d="m8 5 2.5-1.5v5L8 7" /></>);
export const SettingsIcon = ({ size = 18 }: Props) => svg(size, <><circle cx="6" cy="6" r="1.6" /><path d="M6 1v1.5M6 9.5V11M1 6h1.5M9.5 6H11M2.5 2.5l1 1M8.5 8.5l1 1M2.5 9.5l1-1M8.5 3.5l1-1" /></>);
export const ShieldIcon = ({ size = 18 }: Props) => svg(size, <><path d="M6 1 10 2.5V6c0 2.4-1.7 4.2-4 5-2.3-.8-4-2.6-4-5V2.5z" /><path d="m4.3 6 1.2 1.2L7.8 4.8" /></>);
export const BrowseIcon = ({ size = 18 }: Props) => svg(size, <path d="M1.5 1.5h3.5v3.5H1.5zM7 1.5h3.5v3.5H7zM1.5 7h3.5v3.5H1.5zM7 7h3.5v3.5H7z" />);
export const BeaconsIcon = ({ size = 18 }: Props) => svg(size, <><path d="M3.5 1h5v10h-5z" /><path d="M5.3 4.5v3l2.5-1.5z" /></>);
export const WarMapIcon = ({ size = 18 }: Props) => svg(size, <path d="M6 1 10.3 3.5v5L6 11 1.7 8.5v-5z" />);
export const FactionIcon = ({ size = 18 }: Props) => svg(size, <path d="M2.5 11V1.5h7L8 3.75 9.5 6h-7" />);
export const SearchIcon = ({ size = 18 }: Props) => svg(size, <><circle cx="5" cy="5" r="3.5" /><path d="M7.6 7.6 11 11" /></>);
export const BellIcon = ({ size = 18 }: Props) => svg(size, <path d="M3 8.5V5.5a3 3 0 0 1 6 0v3l1 1H2zM5 10.5h2" />);
export const MessageIcon = ({ size = 18 }: Props) => svg(size, <path d="M1.5 2.5h9v6h-5l-2.5 2v-2h-1.5z" />);
export const MenuIcon = ({ size = 18 }: Props) => svg(size, <path d="M1.5 3h9M1.5 6h9M1.5 9h9" />);
export const CloseIcon = ({ size = 18 }: Props) => svg(size, <path d="m2.5 2.5 7 7M9.5 2.5l-7 7" />);

/** The MAGNet mark: three 4×10 px bars in the three faction colors (M.A.G. = Myria, Aetheron, Glint). */
export const MagnetMark = () => <span className="magnet-mark" aria-hidden="true"><span /><span /><span /></span>;
export const CheckIcon = ({ size = 14 }: Props) => svg(size, <path d="m2 6.5 2.5 2.5L10 3" />);
export const CrossIcon = ({ size = 14 }: Props) => svg(size, <path d="m3 3 6 6M9 3 3 9" />);
export const DashIcon = ({ size = 14 }: Props) => svg(size, <path d="M3 6h6" />);
export const CircleIcon = ({ size = 14 }: Props) => svg(size, <circle cx="6" cy="6" r="3.5" />);
export const ExternalIcon = ({ size = 14 }: Props) => svg(size, <path d="M5 2H2v8h8V7M7 1.5h3.5V5M10.5 1.5 5.5 6.5" />);
export const ArrowIcon = ({ size = 18, dir = "right" }: Props & { dir?: "up" | "down" | "left" | "right" }) => <span style={{ display: "inline-flex", transform: `rotate(${({ right: 0, down: 90, left: 180, up: 270 })[dir]}deg)` }}>{svg(size, <path d="M2 6h8M7 3l3 3-3 3" />)}</span>;
export const HeartIcon = ({ size = 14, filled = false }: Props & { filled?: boolean }) => <svg width={size} height={size} viewBox="0 0 12 12" fill={filled ? "currentColor" : "none"} stroke="currentColor" strokeLinejoin="round" aria-hidden="true" focusable="false"><path d="M6 10.2 1.8 6.1a2.3 2.3 0 0 1 3.3-3.2L6 3.8l.9-.9a2.3 2.3 0 0 1 3.3 3.2z" /></svg>;
/** A done / not done mark for checklists, with the state in text for screen readers. */
export const StatusMark = ({ ok, optional = false }: { ok: boolean; optional?: boolean }) => <span className={ok ? "status-mark ok" : "status-mark"}>{ok ? <CheckIcon /> : optional ? <DashIcon /> : <CrossIcon />}<span className="sr-only">{ok ? "Done: " : optional ? "Optional: " : "Not done: "}</span></span>;
