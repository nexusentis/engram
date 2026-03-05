import React, { useEffect, useRef } from 'react';
import styles from './SectionDivider.module.css';
import { useScrollReveal } from '../hooks/useScrollReveal';

interface Props {
  direction?: 'left' | 'right';
}

export default function SectionDivider({ direction = 'left' }: Props) {
  const [wrapRef, isVisible] = useScrollReveal<HTMLDivElement>(0.5);
  const pathRef = useRef<SVGPathElement | null>(null);

  useEffect(() => {
    if (typeof window === 'undefined') return;
    const path = pathRef.current;
    if (!path) return;

    const length = path.getTotalLength();
    path.style.strokeDasharray = `${length}`;
    path.style.strokeDashoffset = `${length}`;

    if (isVisible) {
      path.style.transition = 'stroke-dashoffset 1.2s ease-out';
      path.style.strokeDashoffset = '0';
    }
  }, [isVisible]);

  const d =
    direction === 'left'
      ? 'M 0 20 L 120 20 L 180 10 L 600 10 L 660 20 L 960 20'
      : 'M 0 20 L 300 20 L 360 10 L 780 10 L 840 20 L 960 20';

  return (
    <div ref={wrapRef} className={styles.wrapper}>
      <svg
        viewBox="0 0 960 40"
        preserveAspectRatio="none"
        className={styles.svg}
        role="presentation"
      >
        <defs>
          <linearGradient id={`trace-grad-${direction}`} x1="0" y1="0" x2="1" y2="0">
            <stop offset="0%" stopColor="var(--engram-muted)" stopOpacity="0.3" />
            <stop offset="50%" stopColor="var(--engram-amber)" stopOpacity="0.8" />
            <stop offset="100%" stopColor="var(--engram-muted)" stopOpacity="0.3" />
          </linearGradient>
        </defs>
        <path
          ref={pathRef}
          d={d}
          fill="none"
          stroke={`url(#trace-grad-${direction})`}
          strokeWidth="1.5"
        />
      </svg>
    </div>
  );
}
