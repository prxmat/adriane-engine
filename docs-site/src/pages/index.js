import React from "react";
import Link from "@docusaurus/Link";
import Layout from "@theme/Layout";
import CodeBlock from "@theme/CodeBlock";

const FEATURES = [
  {
    eyebrow: "Deterministic",
    title: "Resumable by construction",
    body: "Every node completion is checkpointed and every lifecycle transition emits an event. Runs suspend cleanly at human gates and resume from the latest checkpoint — replay is exact, not best-effort."
  },
  {
    eyebrow: "Governed",
    title: "Governed by construction",
    body: "Governance — PII redaction, the human-approval gate, the filesystem policy — is sealed engine middleware you cannot turn off. An agent never approves its own output. An ungoverned agent is unrepresentable, not just discouraged."
  },
  {
    eyebrow: "Composable",
    title: "Middleware & profiles",
    body: "Compose an agent from one ordered stack of governed + efficiency middleware. Dial its whole posture with a profile — `profile: \"governed-deep\"` is a governed deep agent in a single word; explicit overrides always win."
  },
  {
    eyebrow: "Deep agents",
    title: "Filesystem, todos, tasks",
    body: "Give an agent a scratchpad bounded by a fail-closed path policy, a durable plan via writeTodos, and isolated sub-agents that return one compressed report — every mutation checkpointed and attributed."
  },
  {
    eyebrow: "One engine",
    title: "Two first-class SDKs",
    body: "The graph model, validator and DSL compiler live once in Rust. The TypeScript and Python SDKs are thin shims over the same engine — a graph that validates in TS validates identically in Python."
  },
  {
    eyebrow: "Fast",
    title: "Rust at the core",
    body: "Adriane runs on a native Rust engine — required, not optional. The TypeScript SDK pulls it in as a dependency; the Python wheel ships it. One engine answers in both languages."
  }
];

function Hero() {
  return (
    <header className="heroBanner">
      <div className="container">
        <h1 className="heroTitle">Adriane</h1>
        <p className="heroTagline">
          The governed agentic graph framework. Build stateful, resumable agent graphs —
          deterministic by default, checkpointed after every step, observable end to end, and
          governed with human-approval gates.
        </p>
        <div className="heroButtons">
          <Link className="button button--primary button--lg" to="/docs/introduction/why-adriane">
            Why Adriane →
          </Link>
          <Link className="button button--secondary button--lg" to="/docs/getting-started/installation">
            Get started
          </Link>
        </div>
        <div className="heroCode">
          <CodeBlock language="bash">{`# TypeScript
npm i @adriane-ai/graph-sdk

# Python
pip install adriane-ai`}</CodeBlock>
        </div>
      </div>
    </header>
  );
}

function Features() {
  return (
    <section className="container">
      <div className="featureGrid">
        {FEATURES.map((f) => (
          <div className="featureCard" key={f.title}>
            <div className="featureEyebrow">{f.eyebrow}</div>
            <h3>{f.title}</h3>
            <p>{f.body}</p>
          </div>
        ))}
      </div>
    </section>
  );
}

export default function Home() {
  return (
    <Layout
      title="The governed agentic graph framework"
      description="Adriane — build stateful, resumable agent graphs. Deterministic, checkpointed, observable, governed. One Rust engine, TypeScript and Python SDKs."
    >
      <Hero />
      <main>
        <Features />
      </main>
    </Layout>
  );
}
