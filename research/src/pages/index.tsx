import React from 'react';
import Layout from '@theme/Layout';
import styles from './index.module.css';

import Hero from '../components/Hero';
import SectionDivider from '../components/SectionDivider';
import Leaderboard from '../components/Leaderboard';
import CategoryBreakdown from '../components/CategoryBreakdown';
import PhaseTimeline from '../components/PhaseTimeline';
import StatsGrid from '../components/StatsGrid';
import ReadingGuide from '../components/ReadingGuide';

export default function BenchmarksPage() {
  return (
    <Layout
      title="Engram — #1 on LongMemEval-S"
      description="479/500 (95.8%) on LongMemEval-S. Rust-native long-term memory for LLMs, surpassing Mastra OM, Honcho, Hindsight, and Emergence."
    >
      <main className={styles.main}>
        <Hero />
        <div className={styles.container}>
          <p className={styles.intro}>
            Engram is a Rust-native long-term memory system for LLMs. Over
            approximately four weeks, we systematically engineered it from a
            blank slate to 479/500 (95.8%) on the LongMemEval-S benchmark —
            reaching #1 globally, surpassing Mastra OM (94.87%) and ahead of
            Honcho (92.6%), Hindsight (91.4%), and Emergence AI (86%).
          </p>
          <Leaderboard />
          <SectionDivider direction="left" />
          <CategoryBreakdown />
          <PhaseTimeline />
          <SectionDivider direction="right" />
          <StatsGrid />
          <ReadingGuide />
        </div>
      </main>
    </Layout>
  );
}
