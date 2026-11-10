import { useId } from "react";

export function LogoMark({ size = 32 }: { size?: number }) {
  const gradient = useId();
  return (
    <svg width={size} height={size} viewBox="0 0 1024 1024" role="img" aria-label="Disweep">
      <defs>
        <linearGradient id={gradient} x1="0.08" y1="0.04" x2="0.94" y2="0.98">
          <stop offset="0" stopColor="#4FB2FF" />
          <stop offset="1" stopColor="#8B7CF6" />
        </linearGradient>
      </defs>
      <rect x="56" y="56" width="912" height="912" rx="216" fill={`url(#${gradient})`} />
      <path
        fill="#fff"
        fillRule="evenodd"
        d="M404 272h172c132 0 240 107 240 240S708 752 576 752H404V272Zm104 100v280h68c77 0 140-63 140-140s-63-140-140-140h-68Z"
      />
      <g fill="#fff">
        <rect x="236" y="340" width="128" height="60" rx="30" opacity=".95" />
        <rect x="164" y="482" width="200" height="60" rx="30" />
        <rect x="268" y="624" width="96" height="60" rx="30" opacity=".9" />
      </g>
    </svg>
  );
}
