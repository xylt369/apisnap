'use client';

import React from 'react';
import { Camera, Terminal, Shield, Zap, Sparkles, Github, ArrowRight, CheckCircle2, Code2, AlertTriangle, Layers } from 'lucide-react';
import TerminalDemo from '../components/TerminalDemo';
import MaskingPlayground from '../components/MaskingPlayground';
import Pricing from '../components/Pricing';

export default function Home() {
  return (
    <div className="bg-[#090b10] text-gray-100 min-h-screen bg-grid-pattern relative selection:bg-cyan-500 selection:text-black">
      {/* Glow Ambient Blobs */}
      <div className="absolute top-0 left-1/2 -translate-x-1/2 w-[700px] h-[350px] bg-gradient-to-tr from-cyan-600/20 to-purple-600/20 blur-[130px] pointer-events-none rounded-full" />

      {/* Navbar */}
      <header className="sticky top-0 z-50 backdrop-blur-md bg-[#090b10]/80 border-b border-cardBorder">
        <div className="max-w-6xl mx-auto px-4 h-16 flex items-center justify-between">
          <div className="flex items-center gap-2.5">
            <div className="p-1.5 rounded-lg bg-gradient-to-tr from-cyan-500 to-blue-600 text-black">
              <Camera className="w-5 h-5 font-bold" />
            </div>
            <span className="font-extrabold text-lg tracking-tight text-white">
              ApiSnap<span className="text-cyan-400">.</span>
            </span>
          </div>

          <nav className="hidden md:flex items-center gap-8 text-sm text-gray-400 font-medium">
            <a href="#features" className="hover:text-white transition">Features</a>
            <a href="#demo" className="hover:text-white transition">Live Demo</a>
            <a href="#pricing" className="hover:text-white transition">Pricing</a>
            <a href="https://github.com/xylt369/apisnap" target="_blank" rel="noreferrer" className="hover:text-white transition">Docs</a>
          </nav>

          <div className="flex items-center gap-3">
            <a
              href="https://github.com/xylt369/apisnap"
              target="_blank"
              rel="noreferrer"
              className="flex items-center gap-2 px-3 py-1.5 rounded-xl border border-cardBorder bg-[#11141d] text-sm text-gray-300 hover:text-white hover:border-gray-600 transition"
            >
              <Github className="w-4 h-4" />
              <span>Star on GitHub</span>
            </a>
            <a
              href="#pricing"
              className="hidden sm:inline-flex items-center gap-1.5 px-4 py-1.5 rounded-xl bg-gradient-to-r from-cyan-500 to-blue-500 text-black font-semibold text-sm hover:opacity-90 transition shadow-lg"
            >
              <span>Get Pro ($19)</span>
            </a>
          </div>
        </div>
      </header>

      {/* Hero Section */}
      <section className="pt-24 pb-16 px-4 max-w-5xl mx-auto text-center relative z-10">
        <div className="inline-flex items-center gap-2 px-3.5 py-1.5 rounded-full border border-cyan-500/30 bg-cyan-500/10 text-cyan-300 text-xs font-mono mb-8 animate-pulse">
          <Sparkles className="w-3.5 h-3.5" />
          <span>v0.1.0 Released • Zero-SDK Backend API Snapshot Guard</span>
        </div>

        <h1 className="text-4xl sm:text-6xl md:text-7xl font-black tracking-tight text-white leading-[1.1]">
          The <span className="bg-gradient-to-r from-cyan-400 via-teal-300 to-blue-500 bg-clip-text text-transparent">Jest Snapshot</span> for Backend APIs.
        </h1>

        <p className="mt-6 text-lg sm:text-xl text-gray-400 max-w-2xl mx-auto leading-relaxed">
          Stop writing 5,000 lines of brittle <code className="text-cyan-300 bg-[#161b26] px-1.5 py-0.5 rounded font-mono text-sm">assert</code> statements.
          Capture live HTTP & gRPC responses, auto-mask volatile UUID/timestamp noise in milliseconds, and catch contract drift before production.
        </p>

        {/* Quick CLI Install & CTA */}
        <div className="mt-10 flex flex-col sm:flex-row items-center justify-center gap-4">
          <div className="flex items-center gap-2 bg-[#121620] border border-cardBorder px-4 py-3 rounded-2xl font-mono text-sm text-gray-300 shadow-inner">
            <span className="text-cyan-400 font-bold">$</span>
            <span>cargo install apisnap</span>
          </div>

          <a
            href="#demo"
            className="flex items-center gap-2 px-6 py-3 rounded-2xl bg-gradient-to-r from-cyan-500 to-blue-500 text-black font-bold text-sm hover:opacity-95 transition shadow-xl"
          >
            <span>See Interactive Demo</span>
            <ArrowRight className="w-4 h-4" />
          </a>
        </div>

        {/* Badges / Tech Proof */}
        <div className="mt-10 flex flex-wrap items-center justify-center gap-6 text-xs font-mono text-gray-400">
          <span className="flex items-center gap-1.5">
            <CheckCircle2 className="w-4 h-4 text-cyan-400" />
            Zero-SDK Dependency (Go, Python, Rust, Node, Java)
          </span>
          <span className="flex items-center gap-1.5">
            <CheckCircle2 className="w-4 h-4 text-cyan-400" />
            Order-Insensitive AST Diffing
          </span>
          <span className="flex items-center gap-1.5">
            <CheckCircle2 className="w-4 h-4 text-cyan-400" />
            100% Deterministic & Safe
          </span>
        </div>
      </section>

      {/* Terminal Demo Section */}
      <section id="demo" className="py-12 px-4 relative z-10">
        <TerminalDemo />
      </section>

      {/* Interactive Masking Playground */}
      <MaskingPlayground />

      {/* Feature Grid */}
      <section id="features" className="py-24 px-4 max-w-6xl mx-auto">
        <div className="text-center mb-16">
          <h2 className="text-3xl md:text-5xl font-extrabold text-white">
            Built for Modern Backend Teams
          </h2>
          <p className="text-gray-400 mt-4 max-w-xl mx-auto text-sm md:text-base">
            Engineered in pure Rust for sub-millisecond execution in CI pipelines.
          </p>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-3 gap-6">
          <div className="p-7 rounded-3xl border border-cardBorder bg-card hover:border-gray-700 transition">
            <div className="w-12 h-12 rounded-2xl bg-cyan-500/10 text-cyan-400 flex items-center justify-center mb-6">
              <Sparkles className="w-6 h-6" />
            </div>
            <h3 className="text-xl font-bold text-white mb-2">Smart Auto-Masker</h3>
            <p className="text-gray-400 text-sm leading-relaxed">
              Auto-detects and sanitizes ISO timestamps, UUIDs, JWT tokens, and Mongo ObjectIds before saving snapshots. Zero manual regex needed.
            </p>
          </div>

          <div className="p-7 rounded-3xl border border-cardBorder bg-card hover:border-gray-700 transition">
            <div className="w-12 h-12 rounded-2xl bg-blue-500/10 text-blue-400 flex items-center justify-center mb-6">
              <Layers className="w-6 h-6" />
            </div>
            <h3 className="text-xl font-bold text-white mb-2">Order-Insensitive Diff</h3>
            <p className="text-gray-400 text-sm leading-relaxed">
              Compares JSON keys as sets, not plain text lines. Never suffer from spurious false-positives caused by backend key reordering.
            </p>
          </div>

          <div className="p-7 rounded-3xl border border-cardBorder bg-card hover:border-gray-700 transition">
            <div className="w-12 h-12 rounded-2xl bg-purple-500/10 text-purple-400 flex items-center justify-center mb-6">
              <Terminal className="w-6 h-6" />
            </div>
            <h3 className="text-xl font-bold text-white mb-2">Interactive Review TUI</h3>
            <p className="text-gray-400 text-sm leading-relaxed">
              Like <code className="text-purple-300 font-mono">cargo-insta</code>, review API changes and accept or reject them with a single keystroke directly in your terminal.
            </p>
          </div>
        </div>
      </section>

      {/* Comparison: Manual Assertions vs ApiSnap */}
      <section className="py-20 px-4 max-w-5xl mx-auto">
        <div className="rounded-3xl border border-cardBorder bg-card p-8 md:p-12">
          <h3 className="text-2xl md:text-3xl font-extrabold text-white text-center mb-10">
            Why Developers Are Ditching Manual Assertions
          </h3>

          <div className="grid grid-cols-1 md:grid-cols-2 gap-8">
            {/* The Old Way */}
            <div className="p-6 rounded-2xl bg-[#0d1017] border border-red-500/30 space-y-3">
              <div className="flex items-center gap-2 text-red-400 font-bold text-sm">
                <AlertTriangle className="w-4 h-4" />
                <span>The Painful Old Way (Manual Asserts)</span>
              </div>
              <p className="text-gray-400 text-xs leading-relaxed">
                Writing 50 lines of boilerplate assert checks per API endpoint. Adding a new non-breaking field breaks 40 unit tests instantly.
              </p>
              <pre className="text-xs font-mono text-red-400/80 bg-[#090b10] p-3 rounded-xl overflow-x-auto">
{`# 50 lines of brittle code...
assert res.status == 200
assert res.json()["data"]["user"]["id"] == 1
assert res.json()["data"]["user"]["name"] == "Alice"
assert res.json()["data"]["user"]["email"] == "..."
# 1 field changes -> 50 tests break!`}
              </pre>
            </div>

            {/* The ApiSnap Way */}
            <div className="p-6 rounded-2xl bg-[#0d1017] border border-cyan-500/50 glow-cyan space-y-3">
              <div className="flex items-center gap-2 text-cyan-400 font-bold text-sm">
                <CheckCircle2 className="w-4 h-4" />
                <span>The ApiSnap Way (Zero-Code Snapshots)</span>
              </div>
              <p className="text-gray-400 text-xs leading-relaxed">
                Run 1 command. Captures real responses, sanitizes volatile noise, and performs sub-millisecond AST delta comparison in CI.
              </p>
              <pre className="text-xs font-mono text-cyan-300 bg-[#090b10] p-3 rounded-xl overflow-x-auto">
{`# 1 command tests all endpoints:
$ apisnap test

# Exit code 0 if matched, 1 if regression!
# Review & accept changes with 1 key:
$ apisnap review`}
              </pre>
            </div>
          </div>
        </div>
      </section>

      {/* Pricing Section */}
      <Pricing />

      {/* Footer */}
      <footer className="border-t border-cardBorder py-12 px-4 text-center text-xs text-gray-500 font-mono">
        <div className="max-w-6xl mx-auto flex flex-col sm:flex-row items-center justify-between gap-4">
          <div>
            © 2026 ApiSnap Open Source Contributors. MIT / Apache-2.0 License.
          </div>
          <div className="flex items-center gap-6">
            <a href="https://github.com/xylt369/apisnap" target="_blank" rel="noreferrer" className="hover:text-gray-300 transition">GitHub</a>
            <a href="#pricing" className="hover:text-gray-300 transition">Pricing</a>
            <a href="https://github.com/xylt369/apisnap/issues" target="_blank" rel="noreferrer" className="hover:text-gray-300 transition">Issues</a>
          </div>
        </div>
      </footer>
    </div>
  );
}
