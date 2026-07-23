import type { SVGProps } from 'react';

import appIconUrl from '../../../../../assets/resources/DruvisIII.png';

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

export function EnvironmentIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg {...iconProps} {...props} aria-hidden="true">
      <circle cx="12" cy="12" r="8.5" />
      <path d="M3.5 12h17" />
      <path d="M12 3.5c2.4 2.3 3.7 5.1 3.7 8.5S14.4 18.2 12 20.5C9.6 18.2 8.3 15.4 8.3 12S9.6 5.8 12 3.5Z" />
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

export function TruvisMark({ className }: { className?: string }) {
  return <img className={className} src={appIconUrl} alt="" aria-hidden="true" draggable={false} />;
}
