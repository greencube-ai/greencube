import { useState, useEffect } from 'react';
import { useApp } from '../context/AppContext';
import { getConfig, saveConfig } from '../lib/invoke';
import type { AppConfig } from '../lib/types';

export function Settings() {
  const { dispatch } = useApp();
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [showKey, setShowKey] = useState(false);

  useEffect(() => { getConfig().then(setConfig); }, []);

  const handleSave = async () => {
    if (!config) return;
    setSaving(true);
    try {
      await saveConfig(config);
      dispatch({ type: 'SET_CONFIG', config });
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (e) { console.error(e); }
    finally { setSaving(false); }
  };

  if (!config) return <div className="text-[var(--text-muted)]">Loading...</div>;

  const update = (path: string, value: string | number | boolean) => {
    setConfig((prev) => {
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
    <div className="max-w-lg">
      <h1 className="text-2xl font-bold mb-8">Settings</h1>

      {/* Alive Mode */}
      <div className="rounded-xl border p-5 mb-8 transition-all" style={{
        backgroundColor: config.ui.alive_mode ? 'rgba(34, 197, 94, 0.04)' : 'var(--bg-secondary)',
        borderColor: config.ui.alive_mode ? 'var(--accent)' : 'var(--border)',
      }}>
        <div className="flex items-center justify-between">
          <div>
            <div className="text-base font-semibold">{config.ui.alive_mode ? 'Alive' : 'Core'}</div>
            <p className="text-xs text-[var(--text-muted)] mt-1">
              {config.ui.alive_mode ? 'Reflection, knowledge extraction, drives, moods active.' : 'Proxy + memory only. Zero background tokens.'}
            </p>
          </div>
          <button onClick={async () => {
            const c = JSON.parse(JSON.stringify(config)) as AppConfig;
            c.ui.alive_mode = !config.ui.alive_mode;
            setConfig(c);
            await saveConfig(c);
            dispatch({ type: 'SET_CONFIG', config: c });
          }} className="px-3 py-1.5 rounded-lg text-xs font-medium border" style={config.ui.alive_mode
            ? { borderColor: 'var(--accent)', color: 'var(--accent)' }
            : { borderColor: 'var(--border)', color: 'var(--text-secondary)' }
          }>{config.ui.alive_mode ? 'Switch to Core' : 'Switch to Alive'}</button>
        </div>
      </div>

      {/* API Key */}
      <div className="mb-6">
        <label className="block text-xs text-[var(--text-muted)] mb-1">API Key</label>
        <div className="flex gap-2">
          <input type={showKey ? 'text' : 'password'} value={config.llm.api_key}
            onChange={(e) => update('llm.api_key', e.target.value)} className="flex-1" placeholder="sk-..." />
          <button onClick={() => setShowKey(!showKey)} className="text-xs text-[var(--text-muted)] px-2">{showKey ? 'hide' : 'show'}</button>
        </div>
      </div>

      <div className="mb-6">
        <label className="block text-xs text-[var(--text-muted)] mb-1">API Base URL</label>
        <input type="text" value={config.llm.api_base_url} onChange={(e) => update('llm.api_base_url', e.target.value)} className="w-full" />
      </div>

      <div className="mb-8">
        <label className="block text-xs text-[var(--text-muted)] mb-1">Model</label>
        <input type="text" value={config.llm.default_model} onChange={(e) => update('llm.default_model', e.target.value)} className="w-48" />
      </div>

      {/* Save */}
      <button onClick={handleSave} disabled={saving}
        className="px-6 py-2 rounded-lg text-black font-medium disabled:opacity-50 hover:brightness-110 transition"
        style={{ backgroundColor: 'var(--accent)' }}>
        {saving ? 'Saving...' : saved ? 'Saved' : 'Save'}
      </button>

      {/* About + Reset */}
      <div className="mt-12 pt-6 border-t text-xs text-[var(--text-muted)] space-y-2" style={{ borderColor: 'var(--border)' }}>
        <div>GreenCube v1.0.0</div>
        <div>Data: ~/.greencube/</div>
        <button onClick={async () => {
          if (!confirm('Delete all data?')) return;
          const { resetApp } = await import('../lib/invoke');
          await resetApp();
          window.location.reload();
        }} className="text-[var(--status-error)] hover:underline">Reset all data</button>
      </div>
    </div>
  );
}
