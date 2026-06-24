// @ts-check
const { themes } = require("prism-react-renderer");

/** @type {import('@docusaurus/types').Config} */
const config = {
  title: "Adriane",
  tagline: "The governed agentic graph framework — deterministic, resumable, observable.",
  favicon: "img/favicon.svg",

  url: "https://prxmat.github.io",
  baseUrl: "/adriane-engine/",
  organizationName: "prxmat",
  projectName: "adriane-engine",

  onBrokenLinks: "warn",
  markdown: {
    mermaid: true,
    hooks: {
      onBrokenMarkdownLinks: "warn"
    }
  },

  i18n: {
    defaultLocale: "en",
    locales: ["en"]
  },

  presets: [
    [
      "classic",
      /** @type {import('@docusaurus/preset-classic').Options} */
      ({
        docs: {
          routeBasePath: "docs",
          sidebarPath: require.resolve("./sidebars.js"),
          editUrl: "https://github.com/prxmat/adriane-engine/tree/main/docs-site/"
        },
        blog: false,
        theme: {
          customCss: require.resolve("./src/css/custom.css")
        }
      })
    ]
  ],

  themes: [
    "@docusaurus/theme-mermaid",
    [
      // Offline full-text search (no Algolia signup; works on GitHub Pages).
      require.resolve("@easyops-cn/docusaurus-search-local"),
      {
        hashed: true,
        indexBlog: false,
        docsRouteBasePath: "/docs",
        highlightSearchTermsOnTargetPage: true
      }
    ]
  ],

  themeConfig:
    /** @type {import('@docusaurus/preset-classic').ThemeConfig} */
    ({
      colorMode: {
        defaultMode: "dark",
        respectPrefersColorScheme: true
      },
      image: "img/logo.svg",
      navbar: {
        title: "Adriane",
        logo: {
          alt: "Adriane",
          src: "img/logo.svg"
        },
        items: [
          {
            type: "docSidebar",
            sidebarId: "docs",
            position: "left",
            label: "Docs"
          },
          {
            to: "/docs/reference/builder-api",
            label: "API Reference",
            position: "left"
          },
          {
            to: "/docs/recipes/overview",
            label: "Cookbook",
            position: "left"
          },
          {
            to: "/docs/reference/built-for-ai-agents",
            label: "For AI agents",
            position: "left"
          },
          {
            href: "https://github.com/prxmat/adriane-engine/blob/main/CONTRIBUTING.md",
            label: "Contribute",
            position: "right"
          },
          {
            href: "https://github.com/prxmat/adriane-engine/releases",
            label: "v1.3.0",
            position: "right"
          },
          {
            href: "https://github.com/prxmat/adriane-engine",
            label: "GitHub",
            position: "right"
          }
        ]
      },
      footer: {
        style: "dark",
        links: [
          {
            title: "Learn",
            items: [
              { label: "Why Adriane", to: "/docs/introduction/why-adriane" },
              { label: "Installation", to: "/docs/getting-started/installation" },
              { label: "Your first run", to: "/docs/getting-started/your-first-run" }
            ]
          },
          {
            title: "Build",
            items: [
              { label: "Core concepts", to: "/docs/core-concepts/graphs-nodes-edges-state" },
              { label: "Governance", to: "/docs/governance/governance-model" },
              { label: "SDK parity", to: "/docs/sdk-parity/one-engine-two-languages" }
            ]
          },
          {
            title: "More",
            items: [
              { label: "GitHub", href: "https://github.com/prxmat/adriane-engine" },
              { label: "npm — @adriane-ai/graph-sdk", href: "https://www.npmjs.com/package/@adriane-ai/graph-sdk" },
              { label: "PyPI — adriane-ai", href: "https://pypi.org/project/adriane-ai/" }
            ]
          }
        ],
        copyright: `Apache-2.0 licensed. The Adriane framework.`
      },
      prism: {
        theme: themes.github,
        darkTheme: themes.dracula,
        additionalLanguages: ["bash", "python", "rust", "yaml", "json", "toml"]
      }
    })
};

module.exports = config;
