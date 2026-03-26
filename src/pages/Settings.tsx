import { useState, useEffect } from 'react';
import { useApp } from '../context/AppContext';
import { getConfig, saveConfig, getDockerStatus } from '../lib/invoke';
import type { AppConfig } from '../lib/types';

export function Settings() {
  const { dispatch } = useApp();
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [dockerAvailable, setDockerAvailable] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [showKey, setShowKey] = useState(false);

  useEffect(() => {
    Promise.all([getConfig(), getDockerStatus()]).then(([c, d]) => {
      setConfig(c);
      setDockerAvailable(d.available);
    });
  }, []);

  const handleSave = async () => {
    if (!config) return;
    setSaving(true);
    setSaved(false);
    try {
      await saveConfig(config);
      dispatch({ type: 'SET_CONFIG', config });
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (e) {
      console.error('Failed to save config:', e);
    } finally {
      setSaving(false);
    }
  };

  if (!config) {
    return <div className="text-[var(--text-muted)]">Loading...</div>;
  }

  const update = (path: string, value: string | number | boolean) => {
    setConfig((prev) => {
      if (!prev) return prev;
      const copy = JSON.parse(JSON.stringify(prev)) as AppConfig;
      const parts = path.split('.');
      let obj: Record<string, unknown> = copy as unknown as Record<string, unknown>;
      for (let i = 0; i < parts.length - 1; i++) {
        obj = obj[parts[i]] as Record<string, unknown>;
      }
      obj[parts[parts.length - 1]] = value;
      return copy;
    });
  };

  return (
    <div className="max-w-2xl">
      <h1 className="text-xl font-bold mb-6">Settings</h1>

      {/* Agent Mode */}
      <section className="mb-8">
        <h2 className="text-base font-medium mb-2 text-[var(--text-secondary)]">Agent Mode</h2>
        <div
          className="rounded-lg border p-4"
          style={{ backgroundColor: 'var(--bg-secondary)', borderColor: config.ui.alive_mode ? 'var(--accent)' : 'var(--border)' }}
        >
          <div className="flex items-center justify-between">
            <div>
              <div className="text-sm font-medium">{config.ui.alive_mode ? 'Alive Mode' : 'Core Mode'}</div>
              <p className="text-[10px] text-[var(--text-muted)] mt-1 max-w-md">
                {config.ui.alive_mode
                  ? 'Reflection, idle thinking, knowledge extraction, goals, and notifications are active. Uses background tokens.'
                  : 'Proxy, memory, audit, and sandbox only. Zero background token usage.'}
              </p>
            </div>
            <button
              onClick={() => update('ui.alive_mode', !config.ui.alive_mode)}
              className="px-3 py-1.5 rounded-md text-xs font-medium border transition"
              style={config.ui.alive_mode
                ? { borderColor: 'var(--accent)', color: 'var(--accent)', backgroundColor: 'var(--accent-subtle)' }
                : { borderColor: 'var(--border)', color: 'var(--text-secondary)' }
              }
            >
              {config.ui.alive_mode ? 'Switch to Core' : 'Switch to Alive'}
            </button>
          </div>
        </div>
      </section>

      {/* LLM Configuration */}
      <section className="mb-8">
        <h2 className="text-base font-medium mb-4 text-[var(--text-secondary)]">LLM Configuration</h2>
        <div className="space-y-4">
          <div>
            <label className="block text-xs text-[var(--text-muted)] mb-1">API Base URL</label>
            <input
              type="text"
              value={config.llm.api_base_url}
              onChange={(e) => update('llm.api_base_url', e.target.value)}
              className="w-full"
            />
          </div>
          <div>
            <label className="block text-xs text-[var(--text-muted)] mb-1">API Key</label>
            <div className="flex gap-2">
              <input
                type={showKey ? 'text' : 'password'}
                value={config.llm.api_key}
                onChange={(e) => update('llm.api_key', e.target.value)}
                className="flex-1 font-mono"
              />
              <button
                onClick={() => setShowKey(!showKey)}
                className="px-3 py-1 rounded border text-xs text-[var(--text-muted)]"
                style={{ borderColor: 'var(--border)' }}
              >
                {showKey ? 'Hide' : 'Show'}
              </button>
            </div>
          </div>
          <div>
            <label className="block text-xs text-[var(--text-muted)] mb-1">Default Model</label>
            <input
              type="text"
              value={config.llm.default_model}
              onChange={(e) => update('llm.default_model', e.target.value)}
              className="w-full"
            />
          </div>
          <div className="mt-4">
            <label className="block text-xs text-[var(--text-muted)] mb-1">Memory Mode</label>
            <select value={config.llm.memory_mode} onChange={(e) => update('llm.memory_mode', e.target.value)} className="w-48">
              <option value="off">Off</option>
              <option value="keyword">Keyword matching</option>
            </select>
            <p className="text-[10px] text-[var(--text-muted)] mt-1">
              When enabled, injects relevant knowledge from past tasks into agent context.
            </p>
          </div>
          <div className="flex items-start gap-3 mt-4">
            <input
              type="checkbox"
              id="self-reflection"
              checked={config.llm.self_reflection_enabled}
              onChange={(e) => update('llm.self_reflection_enabled', e.target.checked)}
              className="w-4 h-4 mt-0.5 accent-[var(--accent)]"
            />
            <div>
              <label htmlFor="self-reflection" className="text-sm text-[var(--text-secondary)] cursor-pointer">
                Self-reflection after tasks
              </label>
              <p className="text-[10px] text-[var(--text-muted)] mt-0.5">
                Agents review what they learned after each task. Sends an additional LLM request. Free with local models (Ollama).
              </p>
            </div>
          </div>
        </div>
      </section>

      {/* Server */}
      <section className="mb-8">
        <h2 className="text-base font-medium mb-4 text-[var(--text-secondary)]">Server</h2>
        <div>
          <label className="block text-xs text-[var(--text-muted)] mb-1">Port (restart required to change)</label>
          <input
            type="number"
            value={config.server.port}
            onChange={(e) => update('server.port', parseInt(e.target.value) || 9000)}
            className="w-32"
          />
        </div>
      </section>

      {/* Sandbox */}
      <section className="mb-8">
        <h2 className="text-base font-medium mb-4 text-[var(--text-secondary)]">Sandbox Defaults</h2>
        <div className="space-y-4">
          <div>
            <label className="block text-xs text-[var(--text-muted)] mb-1">Docker Image</label>
            <input
              type="text"
              value={config.sandbox.image}
              onChange={(e) => update('sandbox.image', e.target.value)}
              className="w-full"
            />
          </div>
          <div className="grid grid-cols-3 gap-4">
            <div>
              <label className="block text-xs text-[var(--text-muted)] mb-1">CPU Limit (cores)</label>
              <input
                type="number"
                step="0.5"
                value={config.sandbox.cpu_limit_cores}
                onChange={(e) => update('sandbox.cpu_limit_cores', parseFloat(e.target.value) || 1)}
                className="w-full"
              />
            </div>
            <div>
              <label className="block text-xs text-[var(--text-muted)] mb-1">Memory (MB)</label>
              <input
                type="number"
                value={config.sandbox.memory_limit_mb}
                onChange={(e) => update('sandbox.memory_limit_mb', parseInt(e.target.value) || 512)}
                className="w-full"
              />
            </div>
            <div>
              <label className="block text-xs text-[var(--text-muted)] mb-1">Timeout (seconds)</label>
              <input
                type="number"
                value={config.sandbox.timeout_seconds}
                onChange={(e) => update('sandbox.timeout_seconds', parseInt(e.target.value) || 300)}
                className="w-full"
              />
            </div>
          </div>
          <div className="flex items-center gap-2">
            <input
              type="checkbox"
              id="network"
              checked={config.sandbox.network_enabled}
              onChange={(e) => update('sandbox.network_enabled', e.target.checked)}
              className="w-4 h-4 accent-[var(--accent)]"
            />
            <label htmlFor="network" className="text-sm text-[var(--text-secondary)]">
              Network Enabled
            </label>
          </div>
        </div>
      </section>

      {/* About */}
      <section className="mb-8">
        <h2 className="text-base font-medium mb-4 text-[var(--text-secondary)]">About</h2>
        <div className="text-sm text-[var(--text-muted)] space-y-1">
          <div>Version: 0.7.0</div>
          <div className="flex items-center gap-2">
            Docker:{' '}
            <span style={{ color: dockerAvailable ? 'var(--status-active)' : 'var(--status-error)' }}>
              {dockerAvailable ? 'Available' : 'Not Available'}
            </span>
          </div>
          <div>Data directory: ~/.greencube/</div>
        </div>
      </section>

      {/* Save button */}
      <div className="flex items-center gap-4">
        <button
          onClick={handleSave}
          disabled={saving}
          className="px-6 py-2 rounded-lg text-black font-medium disabled:opacity-50 hover:brightness-110 transition"
          style={{ backgroundColor: 'var(--accent)' }}
        >
          {saving ? 'Saving...' : saved ? 'Saved!' : 'Save Settings'}
        </button>
      </div>

      {/* Danger zone */}
      <section className="mt-12 pt-6 border-t" style={{ borderColor: 'var(--border)' }}>
        <h2 className="text-base font-medium mb-2 text-[var(--status-error)]">Danger Zone</h2>
        <p className="text-xs text-[var(--text-muted)] mb-4">
          This will delete all agents, memories, and settings. The app will restart with a fresh onboarding.
        </p>
        <button
          onClick={async () => {
            if (!confirm('Delete all data and restart? This cannot be undone.')) return;
            try {
              const { resetApp } = await import('../lib/invoke');
              await resetApp();
              window.location.reload();
            } catch (e) {
              console.error('Reset failed:', e);
            }
          }}
          className="px-4 py-2 rounded-lg border text-sm font-medium transition hover:bg-[rgba(239,68,68,0.1)]"
          style={{ borderColor: 'var(--status-error)', color: 'var(--status-error)' }}
        >
          Reset All Data
        </button>
      </section>
    </div>
  );
}
