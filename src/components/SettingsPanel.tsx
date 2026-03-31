import { useState, useEffect } from 'react';
import { getConfig, saveConfig, getServerInfo, createProvider } from '../lib/invoke';
import { onToast } from '../lib/events';
import type { AppConfig } from '../lib/types';

interface Toast { id: number; type: string; message: string; }

const PROVIDERS = [
  { id: 'openai', name: 'OpenAI', url: 'https://api.openai.com/v1', model: 'gpt-4o', needsKey: true },
  { id: 'openrouter', name: 'OpenRouter', url: 'https://openrouter.ai/api/v1', model: 'openai/gpt-4o', needsKey: true },
  { id: 'ollama', name: 'Ollama', url: 'http://localhost:11434/v1', model: 'llama3', needsKey: false },
  { id: 'lmstudio', name: 'LM Studio', url: 'http://localhost:1234/v1', model: 'local-model', needsKey: false },
];

export function SettingsPanel() {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [port, setPort] = useState(9000);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [showKey, setShowKey] = useState(false);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [copied, setCopied] = useState('');

  useEffect(() => {
    getConfig().then(setConfig);
    getServerInfo().then(info => setPort(info.port)).catch(() => {});
    const unlisten = onToast((data) => {
      const id = Date.now() + Math.random();
      setToasts(prev => [...prev, { id, type: data.type, message: data.message }]);
      setTimeout(() => setToasts(prev => prev.filter(t => t.id !== id)), 4000);
    });
    return () => { unlisten.then(fn => fn()); };
  }, []);

  const handleSave = async () => {
    if (!config) return;
    setSaving(true);
    try {
      try { await createProvider('default', config.llm.api_base_url, config.llm.api_key, config.llm.default_model, 'openai'); } catch { /* exists */ }
      await saveConfig(config);
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (e) { console.error(e); }
    finally { setSaving(false); }
  };

  if (!config) return (
    <div className="flex items-center justify-center min-h-screen">
      <div className="w-1.5 h-1.5 rounded-full animate-pulse" style={{ backgroundColor: '#22c55e' }}></div>
    </div>
  );

  const update = (path: string, value: string) => {
    setConfig(prev => {
      if (!prev) return prev;
      const copy = JSON.parse(JSON.stringify(prev)) as AppConfig;
      const parts = path.split('.');
      let obj: Record<string, unknown> = copy as unknown as Record<string, unknown>;
      for (let i = 0; i < parts.length - 1; i++) obj = obj[parts[i]] as Record<string, unknown>;
      obj[parts[parts.length - 1]] = value;
      return copy;
    });
  };

  const switchProvider = (p: typeof PROVIDERS[0]) => {
    update('llm.api_base_url', p.url);
    update('llm.default_model', p.model);
    if (!p.needsKey) update('llm.api_key', 'local');
  };

  const activeProvider = PROVIDERS.find(p => config.llm.api_base_url.includes(p.url.replace('https://', '').replace('http://', '').split('/')[0]));
  const copy = (text: string, id: string) => { navigator.clipboard.writeText(text); setCopied(id); setTimeout(() => setCopied(''), 1500); };

  return (
    <div className="min-h-screen" style={{ backgroundColor: '#050505' }}>
      {/* Subtle grid background like the website */}
      <div style={{
        position: 'fixed', top: 0, left: 0, width: '100%', height: '100%', pointerEvents: 'none', zIndex: 0,
        backgroundImage: 'linear-gradient(rgba(34,197,94,0.015) 1px,transparent 1px),linear-gradient(90deg,rgba(34,197,94,0.015) 1px,transparent 1px)',
        backgroundSize: '48px 48px',
      }} />
      {/* Top glow */}
      <div style={{
        position: 'fixed', top: -120, left: '50%', transform: 'translateX(-50%)', width: 600, height: 300,
        background: 'radial-gradient(ellipse,rgba(34,197,94,0.05) 0%,transparent 70%)',
        pointerEvents: 'none', zIndex: 0,
      }} />

      <div className="relative z-10 max-w-md mx-auto px-6 py-8">

        {/* Header */}
        <div className="flex items-center justify-between mb-8">
          <div className="flex items-center gap-2">
            <svg width="16" height="16" viewBox="0 0 512 512">
              <rect x="64" y="64" width="384" height="384" rx="48" ry="48" fill="none" stroke="#22C55E" strokeWidth="40"/>
            </svg>
            <span className="text-xs font-semibold" style={{ color: '#52525b' }}>GreenCube</span>
          </div>
          <div className="flex items-center gap-1.5">
            <div className="w-1.5 h-1.5 rounded-full" style={{ backgroundColor: '#22c55e', boxShadow: '0 0 6px rgba(34,197,94,0.4)' }}></div>
            <span className="text-[10px] font-mono" style={{ color: '#52525b' }}>:{port}</span>
          </div>
        </div>

        {/* gc command — the hero */}
        <div className="rounded-xl p-5 mb-6" style={{
          background: 'linear-gradient(135deg, rgba(34,197,94,0.05) 0%, rgba(34,197,94,0.01) 100%)',
          border: '1px solid rgba(34,197,94,0.12)',
        }}>
          <p className="text-[11px] mb-2" style={{ color: '#71717a' }}>check your agent's brain anytime:</p>
          <div className="flex items-center justify-between">
            <button onClick={() => copy('gc', 'gc')} className="group flex items-center gap-2">
              <code className="text-2xl font-mono font-bold" style={{ color: '#22c55e' }}>gc</code>
              <span className="text-[9px] opacity-0 group-hover:opacity-100 transition-opacity" style={{ color: '#52525b' }}>
                {copied === 'gc' ? 'copied!' : 'copy'}
              </span>
            </button>
            <button onClick={() => copy(`curl -s localhost:${port}/brain`, 'curl')}
              className="text-[9px] font-mono px-2 py-1 rounded border transition-colors hover:border-[rgba(34,197,94,0.3)]"
              style={{ color: '#52525b', borderColor: '#1a1a1e' }}>
              {copied === 'curl' ? 'copied!' : 'curl'}
            </button>
          </div>
        </div>

        {/* Provider switcher */}
        <div className="mb-5">
          <label className="block text-[9px] uppercase tracking-widest mb-2" style={{ color: '#52525b' }}>Provider</label>
          <div className="grid grid-cols-4 gap-1.5">
            {PROVIDERS.map(p => (
              <button key={p.id} onClick={() => switchProvider(p)}
                className="px-2 py-2 rounded-lg text-[11px] font-medium transition-all border"
                style={{
                  borderColor: activeProvider?.id === p.id ? 'rgba(34,197,94,0.4)' : '#141416',
                  backgroundColor: activeProvider?.id === p.id ? 'rgba(34,197,94,0.06)' : '#0a0a0c',
                  color: activeProvider?.id === p.id ? '#22c55e' : '#71717a',
                }}>
                {p.name}
              </button>
            ))}
          </div>
        </div>

        {/* Config fields */}
        <div className="space-y-3 mb-6">
          <div>
            <label className="block text-[9px] uppercase tracking-widest mb-1" style={{ color: '#52525b' }}>API Key</label>
            <div className="flex gap-1.5">
              <input type={showKey ? 'text' : 'password'} value={config.llm.api_key}
                onChange={e => update('llm.api_key', e.target.value)}
                className="flex-1 font-mono text-xs rounded-lg border px-3 py-2 outline-none transition-colors focus:border-[rgba(34,197,94,0.3)]"
                style={{ backgroundColor: '#0a0a0c', borderColor: '#141416', color: '#e4e4e7' }}
                placeholder="sk-..." />
              <button onClick={() => setShowKey(!showKey)}
                className="text-[9px] px-2 rounded-lg border transition-colors hover:border-[rgba(34,197,94,0.2)]"
                style={{ color: '#52525b', borderColor: '#141416', backgroundColor: '#0a0a0c' }}>
                {showKey ? 'hide' : 'show'}
              </button>
            </div>
          </div>

          <div className="grid grid-cols-2 gap-1.5">
            <div>
              <label className="block text-[9px] uppercase tracking-widest mb-1" style={{ color: '#52525b' }}>Base URL</label>
              <input type="text" value={config.llm.api_base_url} onChange={e => update('llm.api_base_url', e.target.value)}
                className="w-full text-xs rounded-lg border px-3 py-2 outline-none transition-colors focus:border-[rgba(34,197,94,0.3)]"
                style={{ backgroundColor: '#0a0a0c', borderColor: '#141416', color: '#e4e4e7' }} />
            </div>
            <div>
              <label className="block text-[9px] uppercase tracking-widest mb-1" style={{ color: '#52525b' }}>Model</label>
              <input type="text" value={config.llm.default_model} onChange={e => update('llm.default_model', e.target.value)}
                className="w-full text-xs rounded-lg border px-3 py-2 outline-none transition-colors focus:border-[rgba(34,197,94,0.3)]"
                style={{ backgroundColor: '#0a0a0c', borderColor: '#141416', color: '#e4e4e7' }} />
            </div>
          </div>
        </div>

        {/* Save */}
        <button onClick={handleSave} disabled={saving}
          className="w-full py-2.5 rounded-lg text-sm text-black font-semibold disabled:opacity-50 transition-all relative overflow-hidden"
          style={{ backgroundColor: '#22c55e' }}>
          <span className="relative z-10">{saving ? 'Saving...' : saved ? 'Saved' : 'Save'}</span>
        </button>

        {/* Footer */}
        <div className="mt-10 flex items-center justify-between text-[9px]" style={{ color: '#27272a' }}>
          <span>v1.0.0</span>
          <span>~/.greencube/</span>
          <button onClick={async () => {
            if (!confirm('Delete all data and start fresh?')) return;
            const { resetApp } = await import('../lib/invoke');
            await resetApp();
            window.location.reload();
          }} className="hover:text-[#ef4444] transition-colors">reset</button>
        </div>
      </div>

      {/* Toasts */}
      {toasts.length > 0 && (
        <div className="fixed bottom-4 right-4 flex flex-col gap-2 z-50">
          {toasts.map(toast => {
            const color = toast.type === 'verify_good' ? '#22c55e'
              : toast.type === 'verify_bad' ? '#eab308'
              : toast.type === 'learning' ? '#3b82f6'
              : toast.type === 'error' ? '#ef4444'
              : toast.type === 'warning' ? '#f97316'
              : '#71717a';
            return (
              <div key={toast.id} className="toast-enter flex items-center gap-2 px-4 py-2.5 rounded-lg border text-xs"
                style={{ backgroundColor: '#0a0a0c', borderColor: color + '25', boxShadow: `0 4px 16px ${color}10`, maxWidth: 300 }}>
                <div className="w-1 h-1 rounded-full flex-shrink-0" style={{ backgroundColor: color }}></div>
                <span style={{ color: '#a1a1aa' }}>{toast.message}</span>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
