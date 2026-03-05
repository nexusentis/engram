import React from 'react';
import styles from './Leaderboard.module.css';
import { competitors } from '../data/benchmarkData';
import { useScrollReveal } from '../hooks/useScrollReveal';

// Scale bars relative to the highest score, starting from a baseline
// so differences are visually meaningful
const MIN_DISPLAY = 80; // lowest score we'd ever show
const maxScore = Math.max(...competitors.map((c) => c.score));

export default function Leaderboard() {
  const [ref, isVisible] = useScrollReveal();

  return (
    <section ref={ref} className={`${styles.section} ${isVisible ? styles.visible : ''}`}>
      <h2 className={styles.heading}>SOTA Leaderboard</h2>
      <p className={styles.subheading}>LongMemEval-S, March 2026</p>
      <div className={styles.rows}>
        {competitors.map((c) => {
          // Bar width: normalize between MIN_DISPLAY and maxScore
          const barPct = ((c.score - MIN_DISPLAY) / (maxScore - MIN_DISPLAY)) * 100;
          return (
            <div
              key={c.name}
              className={`${styles.row} ${c.isEngram ? styles.engram : ''}`}
            >
              <span className={styles.rank}>{c.rank}</span>
              <div className={styles.info}>
                <div className={styles.nameRow}>
                  <span className={styles.name}>{c.name}</span>
                  <span className={styles.score}>
                    {c.score.toFixed(c.score % 1 === 0 ? 1 : 2)}%
                  </span>
                </div>
                <div className={styles.barTrack}>
                  <div
                    className={styles.barFill}
                    style={{
                      width: isVisible ? `${barPct}%` : '0%',
                    }}
                  />
                </div>
                <span className={styles.arch}>{c.architecture}</span>
              </div>
            </div>
          );
        })}
      </div>
    </section>
  );
}
