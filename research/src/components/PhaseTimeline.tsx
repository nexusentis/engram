import React, { useState } from 'react';
import styles from './PhaseTimeline.module.css';
import { phases } from '../data/benchmarkData';
import { useScrollReveal } from '../hooks/useScrollReveal';

// Normalize bar heights between min and max scores
const scores = phases.map((p) => p.score);
const minScore = Math.min(...scores);
const maxScore = Math.max(...scores);
const range = maxScore - minScore;

export default function PhaseTimeline() {
  const [ref, isVisible] = useScrollReveal();
  const [hoveredId, setHoveredId] = useState<number | null>(null);

  return (
    <section ref={ref} className={`${styles.section} ${isVisible ? styles.visible : ''}`}>
      <h2 className={styles.heading}>The Journey</h2>
      <p className={styles.subheading}>
        From blank slate to #1 globally in 4 weeks
      </p>

      {/* Compact bar chart */}
      <div className={styles.chart}>
        {phases.map((p, i) => {
          const prevPhase = i > 0 ? phases[i - 1] : null;
          const delta = prevPhase ? p.score - prevPhase.score : null;
          // Bar height: 20% min, 100% max
          const heightPct = 20 + ((p.score - minScore) / range) * 80;
          const isHovered = hoveredId === p.id;

          return (
            <div
              key={p.id}
              className={styles.barGroup}
              onMouseEnter={() => setHoveredId(p.id)}
              onMouseLeave={() => setHoveredId(null)}
            >
              {/* Tooltip */}
              {isHovered && (
                <div className={styles.tooltip}>
                  <span className={styles.tooltipName}>{p.name}</span>
                  <span className={styles.tooltipDesc}>{p.description}</span>
                  <span className={styles.tooltipMeta}>
                    Week {p.week} · {p.passing}/{p.total}
                  </span>
                </div>
              )}

              {/* Score label */}
              <span
                className={`${styles.barLabel} ${
                  p.isCurrent ? styles.labelCurrent : ''
                } ${p.isRegression ? styles.labelRegression : ''}`}
              >
                {p.score.toFixed(1)}%
                {delta !== null && delta !== 0 && (
                  <span
                    className={
                      delta > 0 ? styles.deltaUp : styles.deltaDown
                    }
                  >
                    {delta > 0 ? '+' : ''}{delta.toFixed(1)}
                  </span>
                )}
              </span>

              {/* Bar */}
              <div
                className={`${styles.bar} ${
                  p.isCurrent ? styles.barCurrent : ''
                } ${p.isRegression ? styles.barRegression : ''}`}
                style={{
                  height: isVisible ? `${heightPct}%` : '0%',
                  transitionDelay: `${i * 60}ms`,
                }}
              />

              {/* Phase number */}
              <span className={styles.phaseNum}>{p.id}</span>
            </div>
          );
        })}
      </div>

      <div className={styles.legend}>
        <span className={styles.legendItem}>
          <span className={`${styles.legendDot} ${styles.legendMuted}`} />
          Phase
        </span>
        <span className={styles.legendItem}>
          <span className={`${styles.legendDot} ${styles.legendAmber}`} />
          Current best
        </span>
        <span className={styles.legendItem}>
          <span className={`${styles.legendDot} ${styles.legendRed}`} />
          Regression
        </span>
      </div>
    </section>
  );
}
