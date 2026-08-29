'use client';

import React, { useState } from 'react';
import { Terminal, Play, CheckCircle2, RefreshCw } from 'lucide-react';

export default function TerminalDemo() {
  const [activeTab, setActiveTab] = useState<'record' | 'test' | 'review'>('record');

  return (
    <div className="w-full max-w-4xl mx-auto rounded-2xl overflow-hidden border border-cardBorder bg-card shadow-2xl glow-cyan">
      {/* Terminal Titlebar */}
      <div className="flex items-center justify-between px-4 py-3 bg-[#0d1017] border-b border-cardBorder">
        <div className="flex items-center space-x-2">
          <div className="w-3 h-3 rounded-full bg-red-500/80" />
          <div className="w-3 h-3 rounded-full bg-yellow-500/80" />
          <div className="w-3 h-3 rounded-full bg-green-500/80" />
          <span className="text-xs text-gray-400 font-mono ml-2 flex items-center gap-1.5">
            <Terminal className="w-3.5 h-3.5 text-cyan-400" />
            apisnap-terminal — zsh
          </span>
        </div>

        {/* Tab Switcher */}
        <div className="flex items-center space-x-1 bg-[#161b26] p-1 rounded-lg border border-cardBorder">
          <button
            onClick={() => setActiveTab('record')}
            className={`px-3 py-1 text-xs font-mono rounded-md transition-all ${
              activeTab === 'record'
                ? 'bg-cyan-500/20 text-cyan-400 border border-cyan-500/30'
                : 'text-gray-400 hover:text-gray-200'
            }`}
          >
            1. apisnap record
          </button>
          <button
            onClick={() => setActiveTab('test')}
            className={`px-3 py-1 text-xs font-mono rounded-md transition-all ${
              activeTab === 'test'
                ? 'bg-cyan-500/20 text-cyan-400 border border-cyan-500/30'
                : 'text-gray-400 hover:text-gray-200'
            }`}
          >
            2. apisnap test
          </button>
          <button
            onClick={() => setActiveTab('review')}
            className={`px-3 py-1 text-xs font-mono rounded-md transition-all ${
              activeTab === 'review'
                ? 'bg-cyan-500/20 text-cyan-400 border border-cyan-500/30'
                : 'text-gray-400 hover:text-gray-200'
            }`}
          >
            3. apisnap review
          </button>
        </div>
      </div>

      {/* Terminal Screen Output */}
      <div className="p-6 font-mono text-xs md:text-sm leading-relaxed overflow-x-auto min-h-[300px] bg-[#090b10]">
        {activeTab === 'record' && (
          <div className="space-y-2 animate-fadeIn">
            <div className="flex items-center gap-2 text-cyan-400">
              <span className="text-gray-500">$</span>
              <span>apisnap record --config apisnap.toml</span>
            </div>
            <p className="text-gray-400">
              <span className="text-cyan-400 font-bold">ApiSnap</span> Recording 3 endpoint(s) with concurrency 10...
            </p>
            <div className="text-green-400 flex items-center gap-2">
              <span className="font-bold">[RECORDED]</span> get_user_profile <span className="text-gray-500">→ __snapshots__/get_user_profile.snap.json</span>
            </div>
            <div className="text-green-400 flex items-center gap-2">
              <span className="font-bold">[RECORDED]</span> create_order <span className="text-gray-500">→ __snapshots__/create_order.snap.json</span>
            </div>
            <div className="text-green-400 flex items-center gap-2">
              <span className="font-bold">[RECORDED]</span> list_invoices <span className="text-gray-500">→ __snapshots__/list_invoices.snap.json</span>
            </div>
            <div className="pt-2 text-emerald-400 font-bold flex items-center gap-1.5">
              <CheckCircle2 className="w-4 h-4" />
              Successfully recorded 3 snapshot(s) with dynamic fields sanitized!
            </div>
          </div>
        )}

        {activeTab === 'test' && (
          <div className="space-y-2 animate-fadeIn">
            <div className="flex items-center gap-2 text-cyan-400">
              <span className="text-gray-500">$</span>
              <span>apisnap test</span>
            </div>
            <p className="text-gray-500">============================================================</p>
            <p className="text-white font-bold">
              ApiSnap Test Execution Summary <span className="text-gray-400 font-normal">(42ms)</span>
            </p>
            <p className="text-gray-500">============================================================</p>
            <div className="text-green-400">
              <span className="font-bold">[PASS]</span> get_user_profile <span className="text-gray-500">(HTTP 200)</span>
            </div>
            <div className="text-green-400">
              <span className="font-bold">[PASS]</span> create_order <span className="text-gray-500">(HTTP 201)</span>
            </div>
            <div className="text-green-400">
              <span className="font-bold">[PASS]</span> list_invoices <span className="text-gray-500">(HTTP 200)</span>
            </div>
            <p className="text-gray-500">------------------------------------------------------------</p>
            <p className="text-emerald-400 font-bold">
              Results: 3 total | 3 passed | 0 failed (Exit Code: 0)
            </p>
          </div>
        )}

        {activeTab === 'review' && (
          <div className="space-y-2 animate-fadeIn">
            <div className="flex items-center gap-2 text-cyan-400">
              <span className="text-gray-500">$</span>
              <span>apisnap review</span>
            </div>
            <p className="text-yellow-400 font-bold">
              [!] Snapshot Diff Detected for: <span className="text-cyan-300">get_user_profile</span>
            </p>
            <div className="bg-[#121620] p-3 rounded-lg border border-cardBorder space-y-1">
              <div className="text-gray-300">~ $.data.role</div>
              <div className="text-red-400 pl-4">- "member"</div>
              <div className="text-green-400 pl-4">+ "super_admin"</div>
            </div>
            <div className="pt-2 text-white flex items-center gap-2">
              <span>Decision [</span>
              <span className="text-green-400 font-bold">(a)ccept</span>
              <span>/</span>
              <span className="text-red-400 font-bold">(r)eject</span>
              <span>/</span>
              <span className="text-yellow-400 font-bold">(s)kip</span>
              <span>]:</span>
              <span className="text-cyan-400 font-bold animate-pulse">a</span>
            </div>
            <p className="text-green-400 font-bold">
              [ACCEPTED] Snapshot updated atomically: __snapshots__/get_user_profile.snap.json
            </p>
          </div>
        )}
      </div>
    </div>
  );
}
