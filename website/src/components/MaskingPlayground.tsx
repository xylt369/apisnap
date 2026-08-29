'use client';

import React, { useState } from 'react';
import { Sparkles, Wand2, Check } from 'lucide-react';

const INITIAL_RAW_JSON = `{
  "status": "success",
  "data": {
    "user_id": "c9bf9e57-1685-4c89-bafb-ff5af830be8a",
    "name": "Sarah Connor",
    "email": "sarah@cyberdyne.com",
    "auth_token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.token_payload_hash",
    "created_at": "2026-08-29T23:20:00.123Z",
    "mongo_id": "507f1f77bcf86cd799439011"
  }
}`;

export default function MaskingPlayground() {
  const [rawInput, setRawInput] = useState(INITIAL_RAW_JSON);
  const [copied, setCopied] = useState(false);

  // Client-side regex simulation of Rust auto-masker
  const maskClientJson = (jsonStr: string) => {
    try {
      const parsed = JSON.parse(jsonStr);
      const maskRecursive = (obj: any): any => {
        if (typeof obj === 'string') {
          // UUID
          if (/^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/.test(obj)) {
            return '<MASKED_UUID>';
          }
          // JWT
          if (/^[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+$/.test(obj)) {
            return '<MASKED_JWT>';
          }
          // ISO8601
          if (/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}/.test(obj)) {
            return '<MASKED_TIMESTAMP>';
          }
          // MongoDB ObjectId
          if (/^[0-9a-fA-F]{24}$/.test(obj)) {
            return '<MASKED_OBJECT_ID>';
          }
          return obj;
        }
        if (Array.isArray(obj)) {
          return obj.map(maskRecursive);
        }
        if (obj !== null && typeof obj === 'object') {
          const res: any = {};
          for (const key of Object.keys(obj)) {
            res[key] = maskRecursive(obj[key]);
          }
          return res;
        }
        return obj;
      };

      return JSON.stringify(maskRecursive(parsed), null, 2);
    } catch {
      return '// Invalid JSON Syntax';
    }
  };

  const maskedOutput = maskClientJson(rawInput);

  return (
    <section className="py-20 px-4 max-w-6xl mx-auto">
      <div className="text-center mb-12">
        <div className="inline-flex items-center gap-2 px-3 py-1 rounded-full border border-cyan-500/30 bg-cyan-500/10 text-cyan-400 text-xs font-mono mb-4">
          <Sparkles className="w-3.5 h-3.5" />
          Interactive Live Demo
        </div>
        <h2 className="text-3xl md:text-4xl font-extrabold text-white">
          Smart Auto-Masking in Action
        </h2>
        <p className="text-gray-400 mt-3 max-w-2xl mx-auto text-sm md:text-base">
          Dynamic UUIDs, ISO-8601 timestamps, and JWT tokens break ordinary diffing tools.
          ApiSnap strips volatile noise automatically without changing structural semantics.
        </p>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6 items-stretch">
        {/* Left: Editable Raw JSON */}
        <div className="rounded-2xl border border-cardBorder bg-card p-5 flex flex-col">
          <div className="flex items-center justify-between pb-3 border-b border-cardBorder mb-4">
            <span className="text-xs font-mono text-gray-400 font-bold uppercase tracking-wider">
              1. Raw API Response (Editable)
            </span>
            <span className="text-xs font-mono text-cyan-400">Live Input</span>
          </div>
          <textarea
            value={rawInput}
            onChange={(e) => setRawInput(e.target.value)}
            className="w-full flex-1 min-h-[320px] bg-[#090b10] p-4 rounded-xl font-mono text-xs md:text-sm text-gray-200 border border-cardBorder focus:outline-none focus:border-cyan-500/50 resize-none"
            spellCheck={false}
          />
        </div>

        {/* Right: Auto-Masked Result */}
        <div className="rounded-2xl border border-cardBorder bg-card p-5 flex flex-col glow-cyan">
          <div className="flex items-center justify-between pb-3 border-b border-cardBorder mb-4">
            <span className="text-xs font-mono text-emerald-400 font-bold uppercase tracking-wider flex items-center gap-1.5">
              <Wand2 className="w-3.5 h-3.5" />
              2. Clean Golden Snapshot (.snap.json)
            </span>
            <button
              onClick={() => {
                navigator.clipboard.writeText(maskedOutput);
                setCopied(true);
                setTimeout(() => setCopied(false), 2000);
              }}
              className="text-xs font-mono text-gray-400 hover:text-white flex items-center gap-1 bg-[#161b26] px-2.5 py-1 rounded border border-cardBorder transition"
            >
              {copied ? <Check className="w-3 h-3 text-green-400" /> : null}
              {copied ? 'Copied' : 'Copy'}
            </button>
          </div>
          <pre className="w-full flex-1 min-h-[320px] bg-[#090b10] p-4 rounded-xl font-mono text-xs md:text-sm text-emerald-400/90 border border-cardBorder overflow-auto leading-relaxed">
            {maskedOutput}
          </pre>
        </div>
      </div>
    </section>
  );
}
