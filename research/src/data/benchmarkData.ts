/* ==========================================================================
   Engram Benchmark Data — Typed, importable, with academic provenance.
   All numbers sourced from research/MEMORY.md and benchmark runs.
   ========================================================================== */

export interface Phase {
  id: number;
  name: string;
  score: number;
  passing: number;
  total: number;
  week: number;
  isRegression: boolean;
  isCurrent: boolean;
  description: string;
}

export interface Category {
  name: string;
  score: number;
  passing: number;
  total: number;
  isPerfect: boolean;
}

export interface Competitor {
  name: string;
  score: number;
  rank: number;
  architecture: string;
  isEngram: boolean;
}

export interface StatCard {
  label: string;
  value: string;
  sublabel: string;
}

export interface Provenance {
  ingestionSnapshot: string;
  benchmarkDate: string;
  datasetVersion: string;
  totalQuestions: number;
  totalSessions: number;
  totalFacts: number;
}

// Phase 11 truth run provenance
export const provenance: Provenance = {
  ingestionSnapshot: 'I-v11',
  benchmarkDate: '2026-03-02',
  datasetVersion: 'LongMemEval-S 500',
  totalQuestions: 500,
  totalSessions: 23867,
  totalFacts: 282879,
};

export const phases: Phase[] = [
  {
    id: 1,
    name: 'Baseline',
    score: 72.0,
    passing: 36,
    total: 50,
    week: 1,
    isRegression: false,
    isCurrent: false,
    description: 'Non-agentic baseline with simple retrieval',
  },
  {
    id: 2,
    name: 'Agentic',
    score: 90.0,
    passing: 45,
    total: 50,
    week: 1,
    isRegression: false,
    isCurrent: false,
    description: 'Agentic answering with tool use',
  },
  {
    id: 3,
    name: 'Full Benchmark',
    score: 83.8,
    passing: 419,
    total: 500,
    week: 2,
    isRegression: false,
    isCurrent: false,
    description: 'First full 500-question benchmark run',
  },
  {
    id: 4,
    name: 'Iteration',
    score: 88.0,
    passing: 440,
    total: 500,
    week: 2,
    isRegression: false,
    isCurrent: false,
    description: 'Gate engineering and prompt optimization',
  },
  {
    id: 5,
    name: 'Clean Data',
    score: 88.4,
    passing: 442,
    total: 500,
    week: 3,
    isRegression: false,
    isCurrent: false,
    description: 'Deduplicated ingestion (I-v11), gate loop fix',
  },
  {
    id: 6,
    name: 'Model Upgrade',
    score: 90.4,
    passing: 452,
    total: 500,
    week: 3,
    isRegression: false,
    isCurrent: false,
    description: 'Gemini 3.1 Pro as primary answerer',
  },
  {
    id: 7,
    name: 'Ensemble',
    score: 93.4,
    passing: 467,
    total: 500,
    week: 3,
    isRegression: false,
    isCurrent: false,
    description: 'Gemini primary + GPT-4o fallback ensemble',
  },
  {
    id: 8,
    name: 'Productionization',
    score: 93.4,
    passing: 467,
    total: 500,
    week: 3,
    isRegression: false,
    isCurrent: false,
    description: 'Monolith → 6 crates, REST server, -12K LOC',
  },
  {
    id: 9,
    name: 'Architecture Review',
    score: 94.4,
    passing: 472,
    total: 500,
    week: 4,
    isRegression: false,
    isCurrent: false,
    description: 'Quick wins, abstention 100%, architecture review',
  },
  {
    id: 10,
    name: 'Inverted Ensemble',
    score: 93.2,
    passing: 466,
    total: 500,
    week: 4,
    isRegression: true,
    isCurrent: false,
    description: 'GPT-5.2 primary + Gemini fallback (wrong direction)',
  },
  {
    id: 11,
    name: 'GPT-5.2 Ensemble',
    score: 95.8,
    passing: 479,
    total: 500,
    week: 4,
    isRegression: false,
    isCurrent: true,
    description: 'Gemini primary + GPT-5.2 fallback — #1 globally',
  },
];

export const categories: Category[] = [
  {
    name: 'Extraction',
    score: 100.0,
    passing: 150,
    total: 150,
    isPerfect: true,
  },
  {
    name: 'Abstention',
    score: 100.0,
    passing: 30,
    total: 30,
    isPerfect: true,
  },
  {
    name: 'Updates',
    score: 95.8,
    passing: 69,
    total: 72,
    isPerfect: false,
  },
  {
    name: 'Temporal',
    score: 93.7,
    passing: 119,
    total: 127,
    isPerfect: false,
  },
  {
    name: 'Multi-Session',
    score: 91.7,
    passing: 111,
    total: 121,
    isPerfect: false,
  },
];

export const competitors: Competitor[] = [
  {
    name: 'Engram',
    score: 95.8,
    rank: 1,
    architecture: 'Qdrant + Gemini/GPT-5.2 ensemble agentic',
    isEngram: true,
  },
  {
    name: 'Mastra OM',
    score: 94.87,
    rank: 2,
    architecture: 'No retrieval — observation logs in context',
    isEngram: false,
  },
  {
    name: 'Honcho',
    score: 92.6,
    rank: 3,
    architecture: 'Agentic + fine-tuned models',
    isEngram: false,
  },
  {
    name: 'Hindsight',
    score: 91.4,
    rank: 4,
    architecture: 'Entity graph + 4-way retrieval',
    isEngram: false,
  },
  {
    name: 'Emergence',
    score: 86.0,
    rank: 5,
    architecture: 'Accumulator (Chain-of-Note)',
    isEngram: false,
  },
];

export const stats: StatCard[] = [
  {
    label: 'Best Score',
    value: '95.8%',
    sublabel: '479 / 500 correct',
  },
  {
    label: 'SOTA Rank',
    value: '#1',
    sublabel: 'Globally on LongMemEval-S',
  },
  {
    label: 'Retrieval Recall',
    value: '99.6%',
    sublabel: '498 / 500 questions',
  },
  {
    label: 'Ingested Facts',
    value: '282,879',
    sublabel: 'From 23,867 sessions',
  },
  {
    label: 'Experiments',
    value: '50+',
    sublabel: '~$2,500 total API cost',
  },
  {
    label: 'Architecture',
    value: 'Rust',
    sublabel: 'Native, 6 crates',
  },
];
