import { useState, useEffect } from 'react';
import { useApp } from '../context/AppContext';
import { getServerInfo } from '../lib/invoke';
import { ChatPanel } from '../components/ChatPanel';

const OPENCLAW_CONFIG = (port: number) => `{
  "models": {
    "mode": "merge",
    "providers": {
      "greencube": {
        "baseUrl": "http://localhost:${port}/v1",
        "apiKey": "your-openai-key-here",
        "api": "openai-completions",
        "models": [
          {
            "id": "gpt-4o",
            "name": "gpt-4o",
            "reasoning": false,
            "input": ["text"],
            "contextWindow": 128000,
            "maxTokens": 16384
          }
        ]
      }
    }
  },
  "agents": {
    "defaults": {
      "model": {
        "primary": "greencube/gpt-4o"
      },
      "models": {
        "greencube/gpt-4o": {
          "alias": "gpt-4o"
        }
      }
    }
  }
}`;

export function Connect() {
  const { state } = useApp();
  const [port, setPort] = useState(9000);
  const [copied, setCopied] = useState('');
  const [tab, setTab] = useState<'openclaw' | 'other'>('openclaw');

  useEffect(() => { getServerInfo().then((info) => setPort(info.port)).catch(() => {}); }, []);

  const envLine = `export OPENAI_API_BASE=http://localhost:${port}/v1`;
  const copy = (text: string, id: string) => { navigator.clipboard.writeText(text); setCopied(id); setTimeout(() => setCopied(''), 2000); };

  return (
    <div className="max-w-xl">
      <h1 className="text-2xl font-bold mb-2">Connect your agent</h1>
      <p className="text-sm text-[var(--text-muted)] mb-6">Pick your framework. Takes 2 minutes.</p>

      {/* Tab switcher */}
      <div className="flex gap-0 mb-6 border-b" style={{ borderColor: 'var(--border)' }}>
        <button onClick={() => setTab('openclaw')}
          className={`px-4 py-2.5 text-sm font-medium border-b-2 -mb-px transition-colors ${
            tab === 'openclaw' ? 'text-[var(--accent)] border-[var(--accent)]' : 'text-[var(--text-muted)] border-transparent hover:text-[var(--text-primary)]'
          }`}>OpenClaw</button>
        <button onClick={() => setTab('other')}
          className={`px-4 py-2.5 text-sm font-medium border-b-2 -mb-px transition-colors ${
            tab === 'other' ? 'text-[var(--accent)] border-[var(--accent)]' : 'text-[var(--text-muted)] border-transparent hover:text-[var(--text-primary)]'
          }`}>Other (LangChain, CrewAI, etc.)</button>
      </div>

      {tab === 'openclaw' && (
        <div>
          {/* Step 1 */}
          <div className="flex items-baseline gap-2 mb-2">
            <span className="text-xs font-bold text-[var(--accent)]">1</span>
            <span className="text-sm text-[var(--text-secondary)]">Open your OpenClaw config</span>
          </div>
          <div className="rounded-lg border p-3 mb-5" style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}>
            <div className="flex items-center justify-between">
              <code className="text-xs font-mono text-[var(--text-muted)]">~/.openclaw/openclaw.json</code>
              <button onClick={() => copy('~/.openclaw/openclaw.json', 'path')} className="text-[10px] text-[var(--text-muted)]">{copied === 'path' ? 'copied' : 'copy'}</button>
            </div>
          </div>

          {/* Step 2 */}
          <div className="flex items-baseline gap-2 mb-2">
            <span className="text-xs font-bold text-[var(--accent)]">2</span>
            <span className="text-sm text-[var(--text-secondary)]">Add GreenCube as a provider (paste this)</span>
          </div>
          <div className="rounded-xl border mb-1" style={{ backgroundColor: '#08080a', borderColor: 'var(--border)' }}>
            <div className="flex items-center justify-between px-3 py-2 border-b" style={{ borderColor: 'var(--border)' }}>
              <span className="text-[10px] text-[var(--text-muted)]">openclaw.json</span>
              <button onClick={() => copy(OPENCLAW_CONFIG(port), 'json')}
                className="text-xs px-2 py-0.5 rounded border" style={{ borderColor: 'var(--border)', color: 'var(--text-muted)' }}>
                {copied === 'json' ? 'copied' : 'copy'}
              </button>
            </div>
            <pre className="p-3 text-xs font-mono overflow-x-auto" style={{ color: 'var(--accent)', maxHeight: 280 }}>{OPENCLAW_CONFIG(port)}</pre>
          </div>
          <p className="text-[10px] text-[var(--text-muted)] mb-5">Replace "your-openai-key-here" with your actual API key.</p>

          {/* Step 3 */}
          <div className="flex items-baseline gap-2 mb-2">
            <span className="text-xs font-bold text-[var(--accent)]">3</span>
            <span className="text-sm text-[var(--text-secondary)]">Restart OpenClaw</span>
          </div>
          <div className="rounded-lg border p-3 mb-5" style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}>
            <div className="flex items-center justify-between">
              <code className="text-xs font-mono text-[var(--text-muted)]">openclaw daemon restart</code>
              <button onClick={() => copy('openclaw daemon restart', 'restart')} className="text-[10px] text-[var(--text-muted)]">{copied === 'restart' ? 'copied' : 'copy'}</button>
            </div>
          </div>

          {/* Step 4 */}
          <div className="flex items-baseline gap-2 mb-2">
            <span className="text-xs font-bold text-[var(--accent)]">4</span>
            <span className="text-sm text-[var(--text-secondary)]">Check what your agent learned</span>
          </div>
          <div className="rounded-lg border p-3 mb-2" style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}>
            <div className="flex items-center justify-between">
              <code className="text-xs font-mono text-[var(--text-muted)]">curl localhost:{port}/brain</code>
              <button onClick={() => copy(`curl localhost:${port}/brain`, 'brain')} className="text-[10px] text-[var(--text-muted)]">{copied === 'brain' ? 'copied' : 'copy'}</button>
            </div>
          </div>
          <p className="text-[10px] text-[var(--text-muted)] mb-8">Every request now goes through GreenCube. Your agent remembers, learns, and improves automatically.</p>
        </div>
      )}

      {tab === 'other' && (
        <div>
          <p className="text-sm text-[var(--text-secondary)] mb-4">Add one line before running your agent:</p>

          <div className="rounded-xl border p-5 mb-4" style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--accent)' }}>
            <div className="flex items-center justify-between">
              <code className="text-sm font-mono" style={{ color: 'var(--accent)' }}>{envLine}</code>
              <button onClick={() => copy(envLine, 'env')}
                className="text-xs px-3 py-1 rounded border ml-3 flex-shrink-0" style={{ borderColor: 'var(--border)', color: 'var(--text-muted)' }}>
                {copied === 'env' ? 'copied' : 'copy'}
              </button>
            </div>
          </div>

          <p className="text-xs text-[var(--text-muted)] mb-4">Then check on your agent anytime:</p>
          <div className="rounded-lg border p-3 mb-2" style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}>
            <div className="flex items-center justify-between">
              <code className="text-xs font-mono text-[var(--text-muted)]">curl localhost:{port}/brain</code>
              <button onClick={() => copy(`curl localhost:${port}/brain`, 'brain2')} className="text-[10px] text-[var(--text-muted)]">{copied === 'brain2' ? 'copied' : 'copy'}</button>
            </div>
          </div>

          <p className="text-xs text-[var(--text-muted)] mb-8">Works with any OpenAI-compatible API: LangChain, CrewAI, Python, curl, OpenAI SDK. Does not support Anthropic API or Azure OpenAI directly.</p>
        </div>
      )}

      {/* Test chat */}
      {state.agents.length > 0 && (
        <div>
          <div className="text-[10px] text-[var(--text-muted)] uppercase tracking-wide mb-3">Test your connection</div>
          <ChatPanel agents={state.agents} apiPort={port} hasApiKey={true} />
        </div>
      )}
    </div>
  );
}
