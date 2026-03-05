import React from 'react';
import styles from './StatsGrid.module.css';
import { stats } from '../data/benchmarkData';
import { useScrollReveal } from '../hooks/useScrollReveal';

export default function StatsGrid() {
  const [ref, isVisible] = useScrollReveal();
  // First 2 stats are featured (95.8%, #1), rest are supporting
  const featured = stats.slice(0, 2);
  const supporting = stats.slice(2);

  return (
    <section ref={ref} className={`${styles.section} ${isVisible ? styles.visible : ''}`}>
      <h2 className={styles.heading}>By the Numbers</h2>
      <div className={styles.featured}>
        {featured.map((s) => (
          <div key={s.label} className={styles.featuredCard}>
            <span className={styles.featuredValue}>{s.value}</span>
            <span className={styles.featuredLabel}>{s.label}</span>
            <span className={styles.featuredSublabel}>{s.sublabel}</span>
          </div>
        ))}
      </div>
      <div className={styles.grid}>
        {supporting.map((s) => (
          <div key={s.label} className={styles.card}>
            <span className={styles.value}>{s.value}</span>
            <span className={styles.label}>{s.label}</span>
            <span className={styles.sublabel}>{s.sublabel}</span>
          </div>
        ))}
      </div>
    </section>
  );
}
