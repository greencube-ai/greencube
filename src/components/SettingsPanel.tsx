import { useState, useEffect } from 'react';
import { getConfig, saveConfig, getServerInfo, createProvider } from '../lib/invoke';
import { onToast } from '../lib/events';
import type { AppConfig } from '../lib/types';

interface Toast { id: number; type: string; message: string; }

export function SettingsPanel() {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [port, setPort] = useState(9000);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [showKey, setShowKey] = useState(false);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [copied, setCopied] = useState(false);

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

  if (!config) return <div className="flex items-center justify-center min-h-screen text-[var(--text-muted)]">Loading...</div>;

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

  const copyGc = () => { navigator.clipboard.writeText('gc'); setCopied(true); setTimeout(() => setCopied(false), 1500); };

  return (
    <div className="min-h-screen" style={{ backgroundColor: 'var(--bg-primary)' }}>
      <div className="max-w-md mx-auto px-6 py-12">

        {/* Logo */}
        <div className="flex items-center gap-2.5 mb-10">
          <svg width="18" height="18" viewBox="0 0 512 512">
            <rect x="64" y="64" width="384" height="384" rx="48" ry="48" fill="none" stroke="#22C55E" strokeWidth="40"/>
          </svg>
          <span className="text-sm font-semibold text-[var(--text-muted)]">GreenCube</span>
        </div>

        {/* Status card */}
        <div className="rounded-xl p-5 mb-10" style={{ background: 'linear-gradient(135deg, rgba(34,197,94,0.06) 0%, rgba(34,197,94,0.02) 100%)', border: '1px solid rgba(34,197,94,0.15)' }}>
          <div className="flex items-center gap-2 mb-3">
            <div className="w-1.5 h-1.5 rounded-full" style={{ backgroundColor: '#22c55e', boxShadow: '0 0 6px rgba(34,197,94,0.5)' }}></div>
            <span className="text-sm font-medium" style={{ color: '#22c55e' }}>Running on port {port}</span>
          </div>
          <div className="flex items-center justify-between">
            <div>
              <p className="text-xs text-[var(--text-muted)] mb-1">See what your agent learned:</p>
              <button onClick={copyGc} className="flex items-center gap-2 group">
                <code className="text-lg font-mono font-bold" style={{ color: '#22c55e' }}>gc</code>
                <span className="text-[10px] text-[var(--text-muted)] opacity-0 group-hover:opacity-100 transition-opacity">
                  {copied ? 'copied' : 'click to copy'}
                </span>
              </button>
            </div>
          </div>
        </div>

        {/* Provider config */}
        <div className="space-y-4 mb-8">
          <div>
            <label className="block text-[10px] text-[var(--text-muted)] uppercase tracking-wider mb-1.5">API Key</label>
            <div className="flex gap-2">
              <input type={showKey ? 'text' : 'password'} value={config.llm.api_key}
                onChange={e => update('llm.api_key', e.target.value)}
                className="flex-1 font-mono text-sm rounded-lg border px-3 py-2"
                style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)', color: 'var(--text-primary)' }}
                placeholder="sk-..." />
              <button onClick={() => setShowKey(!showKey)}
                className="text-[10px] text-[var(--text-muted)] px-2 hover:text-[var(--text-primary)] transition-colors">
                {showKey ? 'hide' : 'show'}
              </button>
            </div>
          </div>

          <div>
            <label className="block text-[10px] text-[var(--text-muted)] uppercase tracking-wider mb-1.5">Base URL</label>
            <input type="text" value={config.llm.api_base_url} onChange={e => update('llm.api_base_url', e.target.value)}
              className="w-full text-sm rounded-lg border px-3 py-2"
              style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)', color: 'var(--text-primary)' }} />
          </div>

          <div>
            <label className="block text-[10px] text-[var(--text-muted)] uppercase tracking-wider mb-1.5">Model</label>
            <input type="text" value={config.llm.default_model} onChange={e => update('llm.default_model', e.target.value)}
              className="w-44 text-sm rounded-lg border px-3 py-2"
              style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)', color: 'var(--text-primary)' }} />
          </div>
        </div>

        {/* Save */}
        <button onClick={handleSave} disabled={saving}
          className="px-6 py-2 rounded-lg text-sm text-black font-semibold disabled:opacity-50 transition-all hover:brightness-110"
          style={{ backgroundColor: '#22c55e' }}>
          {saving ? 'Saving...' : saved ? 'Saved' : 'Save'}
        </button>

        {/* Footer */}
        <div className="mt-16 pt-4 border-t space-y-1.5" style={{ borderColor: 'var(--border)' }}>
          <p className="text-[10px] text-[var(--text-muted)]">Close this window — the proxy keeps running in the system tray.</p>
          <p className="text-[10px] text-[var(--text-muted)]">v1.0.0 · ~/.greencube/</p>
          <button onClick={async () => {
            if (!confirm('Delete all data and start fresh?')) return;
            const { resetApp } = await import('../lib/invoke');
            await resetApp();
            window.location.reload();
          }} className="text-[10px] text-[var(--status-error)] hover:underline">Reset all data</button>
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
              <div key={toast.id} className="toast-enter flex items-center gap-2 px-4 py-2.5 rounded-lg border text-sm"
                style={{ backgroundColor: 'var(--bg-secondary)', borderColor: color + '30', boxShadow: `0 4px 12px ${color}15`, maxWidth: 320 }}>
                <span className="text-[var(--text-secondary)]">{toast.message}</span>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
