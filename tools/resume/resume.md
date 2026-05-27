---
header-includes: |
  \usepackage[default]{sourcesanspro}
  \usepackage{sourceserifpro}
  \usepackage{sectsty}
  \allsectionsfont{\rmfamily\bfseries}
---

# Jon Edwards

**Principal Software Engineer**

*Product Engineering · Data Platforms · AI · Scaling Systems, Growing Engineers*

(360) 509-8185 • jkedwards@me.com • US Person • Bainbridge Island, WA

## Summary

Principal Software Engineer building streaming data platforms over datasets in the hundreds of millions of records, modernizing developer platforms with Nix and monorepo tooling, and operationalizing AI-assisted engineering practice. In the past year, shipped a Flow-Based Programming data graph (~2.5M events/day and growing), avmbench — a property and people delivery platform that unlocked $1M in net new revenue and new categories of products — and Scala load shedding for a high-traffic search layer. 10+ years bridging deep IC and engineering leadership; has managed teams of up to 12.

## Skills

- **Specialties:** streaming data platforms, developer-platform modernization, AI-assisted engineering practice, Flow-Based Programming, distributed data pipelines
- **Leadership:** team management, mentorship, high-volume substantive code review, engineering governance
- **Languages:** Python, TypeScript, JavaScript, CUE, Bash; working knowledge of Rust, Scala 2, Java, C#, Kotlin, Swift
- **Cloud & Infrastructure:** AWS, Kubernetes, Helm, Terraform, Azure
- **Build & Deploy:** Nix, Docker, monorepos, CI/CD pipelines (GitHub Actions, Jenkins)
- **Data & AI:** Databricks, Delta Lake, Parquet, PostgreSQL, Redis, Elasticsearch, MCP, Claude API
- **Frontend & Mobile:** React, Vue, React Native, Angular 2+, Cordova

## Work Experience

### Whitepages

*(consumer search platform: 1B+ indexed pages, 10M+ unique users/month)*

**Principal Software Engineer** — Jun 2023 – Present

- Architected and productionized a data graph streaming platform grounded in J. Paul Morrison's Flow-Based Programming, replacing parts of a multi-week, manually coordinated release process with on-demand delivery; currently handles ~2.5M events/day across two tenants in active onboarding.
- Backing datasets cover ~355M people, ~436M phones, ~483M emails, and ~234M addresses; the platform also powers a real-time pipeline streaming scraped public-records data to subscriber webhooks.
- Own architecture decisions, the function server, the pipeline compiler (built on a CUE-based DSL), metrics and logging, and bulk ingestion/export operations.
- Built avmbench, a Property and People data delivery platform that unlocked a $1M net new revenue source — covering ~125M properties across all 50 states with a 17.1% national median error rate on valuation estimates, built on a custom geographic aggregation DSL and Census TIGER pipelines.
- Designed and shipped P95-latency-based load shedding for the search layer in Scala — ring-buffer windowed computation, OPEN/TRIPPED state machine with hysteresis, 429-with-Retry-After rejection — wrapping all 18 route handlers.
- Led migration of 2.5M events/day from Amplitude to Databricks Delta tables, eliminating several thousand dollars per month in overage fees while preserving the Salesforce/Kinesis ingest path.
- Drove monorepo migration with shared tooling, standardized linting, Nix-based developer environments (wp-home-manager), and an internal developer-productivity CLI (pagecraft, PR review workflow with parallelized GitHub fetches and Nix/darwin tooling) — supporting ~100 service releases per week and shifting CVE scanning from infrequent reactive SRE scans to proactive CI gates.
- Built MCP integration and AI inference harness on the engineering platform; authored org-wide documentation to set AI-assisted engineering practice.
- Combined hands-on mentorship and operationalized agentic AI with a robust spec process to lift delivered work 2.5–4x year-over-year — CRO tickets, bug fixes, feature requests, and new tooling — leaving the team running ahead of business demand with real operational headroom, despite shrinking headcount.
- Sustained high-volume, substantive peer code review across the platform org — 261 reviews in the first 15 weeks of 2026 alone (~75/month, mean 6 / median 4 comments per PR, ~1,000-line PR average) — reinforcing engineering standards and a small-PR discipline.
- *Technology used:* Python, Rust, Scala, CUE, Nix, Kubernetes, Helm, Databricks, Delta Lake, MCP.

**Engineering Manager** — Aug 2022 – Jun 2023

- Managed and mentored a team of 6 engineers on the consumer web platform (SEO, Core Web Vitals, conversion optimization, GDPR compliance), leading delivery on LCP regression fixes for high-traffic page types, INP tracking ahead of Google's CWV INP rollout, and an org-wide cookie consent rollout.
- Returned to the IC track in Jun 2023 to focus on data platform architecture.

**Lead Sr. Software Engineer, Web & Mobile Apps** — Dec 2021 – Aug 2022

- Reversed a multi-quarter decline in Google indexation, increasing indexed page count by 30%.
- Drove dozens of millions of SERP-indexed URLs from red LCP (Core Web Vitals failure) to zero, with monitoring automation to keep them green.
- Increased mobile revenue by 20% YoY by focusing on stability and adding new features.
- Lifted mobile app release cadence from every other week to several times per week, focused on SEO performance and mobile feature delivery.

### LIFX / Buddy Technologies

*(contractor through Bluehawk Consulting, converted to FTE in Jan 2020)*

**Software Engineer** — Jul 2019 – Dec 2021

- Built a React Native app replacing separate native iOS and Android applications, and shipped features in the native apps to support a new LIFX power switch product.
- Worked with a team of three for four months to replace a fragile monolithic platform including a portal with a new microservice-based platform, saving $120k a year while improving availability and maintainability.
- Built a React Native application for Airstream's Smart platform; led mobile development and authored custom native plugins for unique connectivity scenarios.
- Built tooling and test frameworks for code quality (type-checking, linting, unit testing) and regression/UX testing (Storybook, screenshot diffing, Detox E2E).
- Architected an end-to-end "checklist service" as a fully tested Micronaut microservice with CRUD support and a performance-optimized client API.
- *Technology used:* React Native, TypeScript, Java, Swift, Kotlin, Micronaut, PostgreSQL, Azure (AKS, DevOps), MQTT, HomeKit.

### Belkin International

**Senior Software Engineer** — Nov 2018 – Jul 2019

- Extended a Cordova mobile application, built React Native and Flutter proof-of-concept apps, and created a test automation framework.

### UpTop

**Product Development Manager** — Jun 2018 – Nov 2018

- Delivered a React Native application for TERRA Staffing and an Alexa skill for TD Bank.
- Laid the groundwork for a platform for the cleaning industry by building a two-person product-focused team.

**Development Team Lead** — Apr 2016 – Jun 2018

- Contributed as IC at 120% billable utilization while building a full-stack development team that grew to 12.
- Successfully delivered 13 web and hybrid mobile application projects for clients including Microsoft, Premera, Capital One, Belkin, and Entertainment Partners.
- Led architecture shift from server-rendered ASP.NET MVC to Web API + Angular on Microsoft's Cloud Launchpad portal, decoupling frontend and backend concerns.
- Served as lead mobile developer on the Belkin Linksys consumer app, with the team delivering on schedule for the CES launch window.
- *Technology used:* ASP.NET Core, C#, Node, MS SQL, Azure, AWS, React, Angular 2+, PHP, WordPress, Elasticsearch.

**Software Developer** — Jul 2015 – Apr 2016

- Finished three client projects (each estimated between 4 and 6 months) early and under budget.

### Earlier Career (1997–2015)

Leadership roles spanning engineering, operations, sales and marketing, and general business management at manufacturing and industrial automation companies. Co-inventor on US patent 7,610,734 for siding installation tools, plus two related fiber-cement cutting applications, developed through 3D CAD, CNC prototyping, and 3D printing.

## Education

### Olympic College

Associates Degree (A.A.S.), General Studies — 3.75 GPA / Phi Theta Kappa
