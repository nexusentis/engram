import React from 'react';
import styles from './CategoryBreakdown.module.css';
import { categories } from '../data/benchmarkData';
import { useScrollReveal } from '../hooks/useScrollReveal';

export default function CategoryBreakdown() {
  const [ref, isVisible] = useScrollReveal();

  return (
    <section ref={ref} className={`${styles.section} ${isVisible ? styles.visible : ''}`}>
      <h2 className={styles.heading}>Category Breakdown</h2>
      <p className={styles.subheading}>Phase 11 — 479/500</p>
      <div className={styles.bars}>
        {categories.map((cat, i) => (
          <div key={cat.name} className={styles.row}>
            <span className={styles.label}>{cat.name}</span>
            <div className={styles.track}>
              <div
                className={`${styles.fill} ${cat.isPerfect ? styles.perfect : ''}`}
                style={{
                  width: isVisible ? `${cat.score}%` : '0%',
                  transitionDelay: `${i * 200}ms`,
                }}
              />
            </div>
            <span className={styles.score}>
              {cat.score.toFixed(1)}%
              <span className={styles.count}>
                {cat.passing}/{cat.total}
              </span>
            </span>
          </div>
        ))}
      </div>
    </section>
  );
}
