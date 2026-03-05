import React from 'react';
import styles from './Hero.module.css';

export default function Hero() {
  return (
    <section className={styles.hero}>
      <div className={styles.glow} />
      <span className={styles.brand}>ENGRAM</span>
      <span className={styles.tagline}>Engineered to remember</span>
      <h1 className={styles.score}>95.8%</h1>
      <hr className={styles.rule} />
      <p className={styles.subtitle}>
        479/500 on LongMemEval-S — <strong>#1 globally</strong>
      </p>
      <div className={styles.pills}>
        <span className={styles.pill}>500 Questions</span>
        <span className={styles.pill}>5 Categories</span>
        <span className={styles.pill}>Rust Native</span>
      </div>
    </section>
  );
}
