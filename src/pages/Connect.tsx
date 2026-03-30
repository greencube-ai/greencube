import { useState, useEffect } from 'react';
import { useApp } from '../context/AppContext';
import { getServerInfo } from '../lib/invoke';
import { ChatPanel } from '../components/ChatPanel';

export function Connect() {
  const { state } = useApp();
  const [port, setPort] = useState(9000);
  const [copied, setCopied] = useState('');

  useEffect(() => { getServerInfo().then((info) => setPort(info.port)).catch(() => {}); }, []);

  const envLine = `export OPENAI_API_BASE=http://localhost:${port}/v1`;
  const copy = (text: string, id: string) => { navigator.clipboard.writeText(text); setCopied(id); setTimeout(() => setCopied(''), 2000); };

  return (
    <div className="max-w-xl">
      <h1 className="text-2xl font-bold mb-2">Connect</h1>
      <p className="text-sm text-[var(--text-muted)] mb-6">Three commands. That's the whole setup.</p>

      {/* Step 1: Connect */}
      <div className="text-[10px] text-[var(--text-muted)] uppercase tracking-wide mb-2">1. connect your agent</div>
      <div className="rounded-xl border p-4 mb-6" style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--accent)' }}>
        <div className="flex items-center justify-between">
          <code className="text-sm font-mono" style={{ color: 'var(--accent)' }}>{envLine}</code>
          <button onClick={() => copy(envLine, 'env')}
            className="text-xs px-3 py-1 rounded border ml-3 flex-shrink-0" style={{ borderColor: 'var(--border)', color: 'var(--text-muted)' }}>
            {copied === 'env' ? 'copied' : 'copy'}
          </button>
        </div>
      </div>

      {/* Step 2: Check brain */}
      <div className="text-[10px] text-[var(--text-muted)] uppercase tracking-wide mb-2">2. check your agent's brain</div>
      <div className="rounded-xl border p-4 mb-2" style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}>
        <div className="flex items-center justify-between">
          <code className="text-sm font-mono text-[var(--text-secondary)]">curl localhost:{port}/brain</code>
          <button onClick={() => copy(`curl localhost:${port}/brain`, 'brain')}
            className="text-xs px-3 py-1 rounded border ml-3 flex-shrink-0" style={{ borderColor: 'var(--border)', color: 'var(--text-muted)' }}>
            {copied === 'brain' ? 'copied' : 'copy'}
          </button>
        </div>
      </div>
      <div className="flex gap-4 mb-6">
        <div className="rounded-lg border px-3 py-2 flex-1" style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}>
          <div className="flex items-center justify-between">
            <code className="text-xs font-mono text-[var(--text-muted)]">curl localhost:{port}/status</code>
            <button onClick={() => copy(`curl localhost:${port}/status`, 'status')} className="text-[10px] text-[var(--text-muted)] ml-2">{copied === 'status' ? 'copied' : 'copy'}</button>
          </div>
        </div>
        <div className="rounded-lg border px-3 py-2 flex-1" style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}>
          <div className="flex items-center justify-between">
            <code className="text-xs font-mono text-[var(--text-muted)]">curl localhost:{port}/log</code>
            <button onClick={() => copy(`curl localhost:${port}/log`, 'log')} className="text-[10px] text-[var(--text-muted)] ml-2">{copied === 'log' ? 'copied' : 'copy'}</button>
          </div>
        </div>
      </div>

      <p className="text-xs text-[var(--text-muted)] mb-8">Works with any OpenAI-compatible API: OpenAI, Ollama, LM Studio, OpenRouter. Does not support Anthropic API or Azure OpenAI directly.</p>

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
