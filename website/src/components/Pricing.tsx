'use client';

import React from 'react';
import { Check, Zap, Sparkles, ShieldCheck } from 'lucide-react';

export default function Pricing() {
  return (
    <section id="pricing" className="py-24 px-4 max-w-6xl mx-auto">
      <div className="text-center mb-16">
        <div className="inline-flex items-center gap-2 px-3 py-1 rounded-full border border-purple-500/30 bg-purple-500/10 text-purple-400 text-xs font-mono mb-4">
          <Zap className="w-3.5 h-3.5" />
          Simple, Transparent Pricing
        </div>
        <h2 className="text-3xl md:text-5xl font-extrabold text-white">
          Invest in 100% Reliable APIs
        </h2>
        <p className="text-gray-400 mt-4 max-w-xl mx-auto text-sm md:text-base">
          Start for free with our open-source CLI. Upgrade for lifetime developer superpowers or team PR governance.
        </p>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-3 gap-8 items-stretch">
        {/* Tier 1: Community */}
        <div className="rounded-3xl border border-cardBorder bg-card p-8 flex flex-col justify-between hover:border-gray-700 transition">
          <div>
            <div className="text-sm font-mono text-gray-400 uppercase tracking-wider">Community</div>
            <div className="mt-4 flex items-baseline gap-1">
              <span className="text-4xl font-extrabold text-white">$0</span>
              <span className="text-gray-400 text-sm">/ forever</span>
            </div>
            <p className="text-gray-400 text-xs mt-3 leading-relaxed">
              Full-featured open-source CLI for individual developers.
            </p>

            <ul className="mt-8 space-y-3 text-sm text-gray-300">
              <li className="flex items-center gap-2.5">
                <Check className="w-4 h-4 text-cyan-400" />
                <span>Single-binary Rust CLI</span>
              </li>
              <li className="flex items-center gap-2.5">
                <Check className="w-4 h-4 text-cyan-400" />
                <span>Deterministic Smart Auto-Masker</span>
              </li>
              <li className="flex items-center gap-2.5">
                <Check className="w-4 h-4 text-cyan-400" />
                <span>Order-insensitive AST diffing</span>
              </li>
              <li className="flex items-center gap-2.5">
                <Check className="w-4 h-4 text-cyan-400" />
                <span>Interactive Terminal Review (TUI)</span>
              </li>
              <li className="flex items-center gap-2.5">
                <Check className="w-4 h-4 text-cyan-400" />
                <span>Unlimited local snapshots</span>
              </li>
            </ul>
          </div>

          <a
            href="https://github.com/xylt369/apisnap"
            target="_blank"
            rel="noreferrer"
            className="mt-8 block text-center py-3 px-4 rounded-xl border border-cardBorder bg-[#161b26] text-white font-medium text-sm hover:bg-[#1c2230] transition"
          >
            Star on GitHub
          </a>
        </div>

        {/* Tier 2: Pro Lifetime (The Bruno Golden Edition Model) */}
        <div className="rounded-3xl border border-cyan-500/50 bg-gradient-to-b from-[#0f172a] to-[#090b10] p-8 flex flex-col justify-between shadow-2xl glow-cyan relative">
          <div className="absolute -top-3.5 left-1/2 -translate-x-1/2 px-3 py-1 rounded-full bg-gradient-to-r from-cyan-500 to-blue-500 text-black text-xs font-bold uppercase tracking-wider flex items-center gap-1 shadow-lg">
            <Sparkles className="w-3 h-3" />
            Most Popular • Lifetime
          </div>

          <div>
            <div className="text-sm font-mono text-cyan-400 uppercase tracking-wider">Pro Edition</div>
            <div className="mt-4 flex items-baseline gap-1">
              <span className="text-4xl font-extrabold text-white">$19</span>
              <span className="text-gray-400 text-sm">/ one-time buy</span>
            </div>
            <p className="text-gray-400 text-xs mt-3 leading-relaxed">
              Early Bird Lifetime License for power developers & indie hackers.
            </p>

            <ul className="mt-8 space-y-3 text-sm text-gray-200">
              <li className="flex items-center gap-2.5">
                <Check className="w-4 h-4 text-cyan-400" />
                <span className="font-semibold text-white">Everything in Community</span>
              </li>
              <li className="flex items-center gap-2.5">
                <Check className="w-4 h-4 text-cyan-400" />
                <span>Official VS Code Extension</span>
              </li>
              <li className="flex items-center gap-2.5">
                <Check className="w-4 h-4 text-cyan-400" />
                <span>Local Web GUI Dashboard</span>
              </li>
              <li className="flex items-center gap-2.5">
                <Check className="w-4 h-4 text-cyan-400" />
                <span>Advanced Multi-Env Switcher</span>
              </li>
              <li className="flex items-center gap-2.5">
                <Check className="w-4 h-4 text-cyan-400" />
                <span>Lifetime updates & Discord VIP</span>
              </li>
            </ul>
          </div>

          <button
            onClick={() => alert('Redirecting to Stripe / LemonSqueezy Checkout for ApiSnap Pro ($19)...')}
            className="mt-8 w-full py-3 px-4 rounded-xl bg-gradient-to-r from-cyan-500 to-blue-500 text-black font-bold text-sm hover:opacity-90 transition shadow-lg"
          >
            Get Pro Lifetime ($19)
          </button>
        </div>

        {/* Tier 3: Team */}
        <div className="rounded-3xl border border-cardBorder bg-card p-8 flex flex-col justify-between hover:border-gray-700 transition">
          <div>
            <div className="text-sm font-mono text-purple-400 uppercase tracking-wider">Team & Cloud</div>
            <div className="mt-4 flex items-baseline gap-1">
              <span className="text-4xl font-extrabold text-white">$15</span>
              <span className="text-gray-400 text-sm">/ seat / month</span>
            </div>
            <p className="text-gray-400 text-xs mt-3 leading-relaxed">
              Automated PR contract guard for engineering organizations.
            </p>

            <ul className="mt-8 space-y-3 text-sm text-gray-300">
              <li className="flex items-center gap-2.5">
                <Check className="w-4 h-4 text-purple-400" />
                <span className="font-semibold text-white">Everything in Pro</span>
              </li>
              <li className="flex items-center gap-2.5">
                <Check className="w-4 h-4 text-purple-400" />
                <span>GitHub PR Visual Diff Bot</span>
              </li>
              <li className="flex items-center gap-2.5">
                <Check className="w-4 h-4 text-purple-400" />
                <span>Breaking Change Blocker in CI</span>
              </li>
              <li className="flex items-center gap-2.5">
                <Check className="w-4 h-4 text-purple-400" />
                <span>Cross-Repo Snapshot Cloud Sync</span>
              </li>
              <li className="flex items-center gap-2.5">
                <Check className="w-4 h-4 text-purple-400" />
                <span>SOC2 & Audit Logs Support</span>
              </li>
            </ul>
          </div>

          <button
            onClick={() => alert('Contacting enterprise sales for ApiSnap Team...')}
            className="mt-8 w-full py-3 px-4 rounded-xl border border-purple-500/30 bg-purple-500/10 text-purple-300 font-medium text-sm hover:bg-purple-500/20 transition"
          >
            Start 14-Day Free Trial
          </button>
        </div>
      </div>
    </section>
  );
}
