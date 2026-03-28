import { useState, useEffect } from 'react';
import { useApp } from '../context/AppContext';
import { getServerInfo } from '../lib/invoke';
import { ChatPanel } from '../components/ChatPanel';

export function Connect() {
  const { state } = useApp();
  const [port, setPort] = useState(9000);
  const [copied, setCopied] = useState(false);

  useEffect(() => { getServerInfo().then((info) => setPort(info.port)).catch(() => {}); }, []);

  const envLine = `export OPENAI_API_BASE=http://localhost:${port}/v1`;

  return (
    <div className="max-w-xl">
      <h1 className="text-2xl font-bold mb-2">Connect</h1>
      <p className="text-sm text-[var(--text-muted)] mb-8">Add this line before running your agent.</p>

      <div className="rounded-xl border p-5 mb-4" style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--accent)' }}>
        <div className="flex items-center justify-between">
          <code className="text-sm font-mono" style={{ color: 'var(--accent)' }}>{envLine}</code>
          <button onClick={() => { navigator.clipboard.writeText(envLine); setCopied(true); setTimeout(() => setCopied(false), 2000); }}
            className="text-xs px-3 py-1 rounded border ml-3 flex-shrink-0" style={{ borderColor: 'var(--border)', color: 'var(--text-muted)' }}>
            {copied ? 'copied' : 'copy'}
          </button>
        </div>
      </div>

      <p className="text-xs text-[var(--text-muted)] mb-10">Works with OpenClaw, LangChain, CrewAI, Python, curl — anything OpenAI-compatible.</p>

      {/* Test chat */}
      {state.agents.length > 0 && (
        <div>
          <div className="text-xs text-[var(--text-muted)] mb-3">Test your connection</div>
          <ChatPanel agents={state.agents} apiPort={port} hasApiKey={true} />
        </div>
      )}
    </div>
  );
}
