import {themes as prismThemes} from 'prism-react-renderer';
import type {Config} from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';

const config: Config = {
  title: 'Engram Research',
  tagline: 'Long-term memory for LLMs — from 0% to 95.8% on LongMemEval-S',
  favicon: 'img/favicon.svg',

  url: 'https://engram.nexusentis.ie',
  baseUrl: '/research/',

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
            '_archive/**',
            'codex_prompts/**',
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
        theme: {
          customCss: './src/css/custom.css',
        },
      } satisfies Preset.Options,
    ],
  ],

  themeConfig: {
    colorMode: {
      defaultMode: 'dark',
      disableSwitch: true,
      respectPrefersColorScheme: false,
    },
    navbar: {
      title: 'Research',
      logo: {
        alt: 'Engram',
        src: 'img/logo.svg',
      },
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
      copyright: `Engram Research — Federico Rinaldi, ${new Date().getFullYear()}`,
    },
    prism: {
      theme: prismThemes.github,
      darkTheme: prismThemes.dracula,
      additionalLanguages: ['rust', 'toml', 'bash'],
    },
  } satisfies Preset.ThemeConfig,
};

export default config;
