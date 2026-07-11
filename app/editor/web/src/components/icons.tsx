import type { SVGProps } from 'react';

const iconProps = {
  width: 18,
  height: 18,
  viewBox: '0 0 24 24',
  fill: 'none',
  stroke: 'currentColor',
  strokeWidth: 1.8,
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
};

export function RefreshIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...iconProps} {...props} aria-hidden="true">
      <path d="M20 7v5h-5" />
      <path d="M4.9 17a8 8 0 0 0 13.5-2M4 12a8 8 0 0 1 13.5-5" />
      <path d="M4 7v5h5" />
    </svg>
  );
}

export function SearchIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...iconProps} {...props} aria-hidden="true">
      <circle cx="11" cy="11" r="6.5" />
      <path d="m16 16 4 4" />
    </svg>
  );
}

export function ChevronIcon({ direction, ...props }: SVGProps<SVGSVGElement> & { direction: 'left' | 'right' }) {
  return (
    <svg {...iconProps} {...props} aria-hidden="true">
      <path d={direction === 'left' ? 'm14.5 6-6 6 6 6' : 'm9.5 6 6 6-6 6'} />
    </svg>
  );
}

export function TruvisMark(props: SVGProps<SVGSVGElement>) {
  return (
    <svg width="26" height="26" viewBox="0 0 26 26" fill="none" {...props} aria-hidden="true">
      <path d="M13 2.8 23.2 21H2.8L13 2.8Z" stroke="currentColor" strokeWidth="2" />
      <path d="m13 8.2 4 7.1H9l4-7.1Z" fill="currentColor" />
    </svg>
  );
}
