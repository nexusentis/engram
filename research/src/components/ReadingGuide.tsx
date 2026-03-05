import React from 'react';
import styles from './ReadingGuide.module.css';
import { useScrollReveal } from '../hooks/useScrollReveal';

const sections = [
  {
    num: '01',
    title: 'Context & Landscape',
    description: 'LongMemEval-S benchmark, competitors, and the state of the art.',
    href: '/research/context/',
  },
  {
    num: '02',
    title: 'Architecture',
    description: 'Ingestion, storage, retrieval, and agentic answering pipeline.',
    href: '/research/architecture/',
  },
  {
    num: '03',
    title: 'The Journey',
    description: 'From 0% to 95.8% — eleven phases of iterative engineering.',
    href: '/research/journey/',
  },
  {
    num: '04',
    title: 'Forensics',
    description: 'Deep analysis of the remaining 21 failures at 95.8%.',
    href: '/research/forensics/',
  },
  {
    num: '05',
    title: 'Dead Ends',
    description: '$2,500+ of failed experiments and the rules we extracted.',
    href: '/research/dead-ends/',
  },
  {
    num: '06',
    title: 'Lessons & Future',
    description: 'What actually moves the score and the path beyond 95.8%.',
    href: '/research/lessons/',
  },
];

export default function ReadingGuide() {
  const [ref, isVisible] = useScrollReveal();

  return (
    <section ref={ref} className={`${styles.section} ${isVisible ? styles.visible : ''}`}>
      <h2 className={styles.heading}>Read the Research</h2>
      <p className={styles.subheading}>
        The full story of building Engram, from blank slate to #1.
      </p>
      <div className={styles.grid}>
        {sections.map((s) => (
          <a key={s.num} href={s.href} className={styles.card}>
            <span className={styles.num}>{s.num}</span>
            <span className={styles.title}>{s.title}</span>
            <span className={styles.desc}>{s.description}</span>
          </a>
        ))}
      </div>
      <p className={styles.attribution}>By Federico Rinaldi</p>
    </section>
  );
}
