import {themes as prismThemes} from 'prism-react-renderer';
import type {Config} from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';

const config: Config = {
  title: 'Engram Docs',
  tagline: 'Developer documentation for Engram — a Rust-native AI agent memory system',
  favicon: 'img/favicon.ico',

  url: 'https://engram.nexusentis.ie',
  baseUrl: '/docs/',

  organizationName: 'nexusentis',
  projectName: 'engram',

  onBrokenLinks: 'warn',
  onBrokenMarkdownLinks: 'warn',

  i18n: {
    defaultLocale: 'en',
    locales: ['en'],
  },

  markdown: {
    format: 'md',
  },

  presets: [
    [
      'classic',
      {
        docs: {
          path: '.',
          exclude: [
            'node_modules/**',
            'package.json',
            'package-lock.json',
            '*.ts',
            '*.js',
            'tsconfig.json',
            'build/**',
            '.docusaurus/**',
          ],
          routeBasePath: '/',
          sidebarPath: './sidebars.ts',
        },
        blog: false,
        theme: {},
      } satisfies Preset.Options,
    ],
  ],

  themeConfig: {
    navbar: {
      title: 'Engram Docs',
      items: [
        {
          href: 'https://github.com/nexusentis/engram',
          label: 'GitHub',
          position: 'right',
        },
      ],
    },
    footer: {
      style: 'dark',
      copyright: `Engram — Federico Rinaldi, ${new Date().getFullYear()}`,
    },
    prism: {
      theme: prismThemes.github,
      darkTheme: prismThemes.dracula,
      additionalLanguages: ['rust', 'toml', 'bash', 'json'],
    },
  } satisfies Preset.ThemeConfig,
};

export default config;
