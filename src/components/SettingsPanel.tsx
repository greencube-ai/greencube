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
      // Also update provider in DB
      try {
        await createProvider('default', config.llm.api_base_url, config.llm.api_key, config.llm.default_model, 'openai');
      } catch {
        // Already exists — saveConfig will sync the key
      }
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

  return (
    <div className="min-h-screen" style={{ backgroundColor: 'var(--bg-primary)' }}>
      <div className="max-w-lg mx-auto px-6 py-10">
        {/* Header */}
        <div className="flex items-center gap-3 mb-8">
          <svg width="24" height="24" viewBox="0 0 512 512">
            <rect x="64" y="64" width="384" height="384" rx="48" ry="48" fill="none" stroke="#22C55E" strokeWidth="40"/>
          </svg>
          <h1 className="text-xl font-bold">GreenCube</h1>
          <span className="text-xs text-[var(--text-muted)] ml-auto font-mono">localhost:{port}</span>
        </div>

        {/* Status */}
        <div className="rounded-xl border p-4 mb-8" style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--accent)', borderWidth: 1 }}>
          <div className="flex items-center gap-2 mb-2">
            <div className="w-2 h-2 rounded-full bg-[var(--accent)]"></div>
            <span className="text-sm font-medium">Proxy running</span>
          </div>
          <p className="text-xs text-[var(--text-muted)]">Your agent is connected. Check what it learned anytime:</p>
          <code className="text-xs font-mono mt-1 block" style={{ color: 'var(--accent)' }}>curl localhost:{port}/brain</code>
        </div>

        {/* API Key */}
        <div className="mb-5">
          <label className="block text-xs text-[var(--text-muted)] mb-1.5">API Key</label>
          <div className="flex gap-2">
            <input type={showKey ? 'text' : 'password'} value={config.llm.api_key}
              onChange={e => update('llm.api_key', e.target.value)} className="flex-1 font-mono" placeholder="sk-..." />
            <button onClick={() => setShowKey(!showKey)} className="text-xs text-[var(--text-muted)] px-2">{showKey ? 'hide' : 'show'}</button>
          </div>
        </div>

        {/* Base URL */}
        <div className="mb-5">
          <label className="block text-xs text-[var(--text-muted)] mb-1.5">API Base URL</label>
          <input type="text" value={config.llm.api_base_url} onChange={e => update('llm.api_base_url', e.target.value)} className="w-full" />
        </div>

        {/* Model */}
        <div className="mb-6">
          <label className="block text-xs text-[var(--text-muted)] mb-1.5">Model</label>
          <input type="text" value={config.llm.default_model} onChange={e => update('llm.default_model', e.target.value)} className="w-48" />
        </div>

        {/* Save */}
        <button onClick={handleSave} disabled={saving}
          className="px-6 py-2.5 rounded-lg text-black font-semibold disabled:opacity-50 hover:brightness-110 transition"
          style={{ backgroundColor: 'var(--accent)' }}>
          {saving ? 'Saving...' : saved ? 'Saved' : 'Save'}
        </button>

        {/* Footer */}
        <div className="mt-12 pt-6 border-t text-xs text-[var(--text-muted)] space-y-2" style={{ borderColor: 'var(--border)' }}>
          <div>GreenCube v1.0.0</div>
          <div>Data: ~/.greencube/</div>
          <div>Close this window — the proxy keeps running in the system tray.</div>
          <button onClick={async () => {
            if (!confirm('Delete all data and start fresh?')) return;
            const { resetApp } = await import('../lib/invoke');
            await resetApp();
            window.location.reload();
          }} className="text-[var(--status-error)] hover:underline">Reset all data</button>
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
