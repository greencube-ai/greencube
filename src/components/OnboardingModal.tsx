import { useState } from 'react';
import { useApp } from '../context/AppContext';
import { saveConfig, createProvider, getConfig, getServerInfo, readOpenclawConfig, configureOpenclaw, restartOpenclaw, setEnvPermanently } from '../lib/invoke';
import type { AppConfig } from '../lib/types';

type Mode = 'pick' | 'openclaw' | 'openai' | 'ollama' | 'done';

function detectOS(): 'windows' | 'mac' | 'linux' {
  const ua = navigator.userAgent.toLowerCase();
  if (ua.includes('win')) return 'windows';
  if (ua.includes('mac')) return 'mac';
  return 'linux';
}

export function OnboardingModal() {
  const { state } = useApp();
  const [mode, setMode] = useState<Mode>('pick');
  const [apiKey, setApiKey] = useState('');
  const [port, setPort] = useState(9000);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [result, setResult] = useState('');
  const [envSet, setEnvSet] = useState(false);
  const [envLoading, setEnvLoading] = useState(false);
  const os = detectOS();

  useState(() => { getServerInfo().then(info => setPort(info.port)).catch(() => {}); });

  const finishSetup = async (key: string, baseUrl: string, model: string) => {
    // Create provider in DB so the proxy can use it
    try {
      await createProvider('default', baseUrl, key, model, 'openai');
    } catch {
      // Provider might already exist, that's ok
    }

    // Save config + mark onboarding complete
    const config = state.config ?? await getConfig();
    const updated: AppConfig = {
      ...config,
      llm: { ...config.llm, api_key: key, api_base_url: baseUrl, default_model: model },
      ui: { ...config.ui, onboarding_complete: true },
    };
    await saveConfig(updated);
  };

  const handleOpenclaw = async () => {
    setLoading(true);
    setError('');
    try {
      // Check if OpenClaw config exists
      await readOpenclawConfig();

      // Auto-configure
      const res = await configureOpenclaw(port);
      setResult(res.key_found ? `Configured with model ${res.model}. API key copied from your existing provider.` : `Configured with model ${res.model}. You'll need to add an API key to ~/.openclaw/openclaw.json.`);

      // Create GreenCube agent
      const key = res.key_found ? 'auto-configured' : '';
      await finishSetup(key, 'https://api.openai.com/v1', res.model);
      setMode('done');
    } catch (e: unknown) {
      const msg = String(e);
      if (msg.includes('not_found')) {
        setError('OpenClaw config not found. Install OpenClaw first, then try again.');
      } else {
        setError(msg);
      }
    } finally {
      setLoading(false);
    }
  };

  const handleOpenai = async () => {
    if (!apiKey.trim()) return;
    setLoading(true);
    setError('');
    try {
      await finishSetup(apiKey, 'https://api.openai.com/v1', 'gpt-4o');
      setMode('done');
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  const handleOllama = async () => {
    setLoading(true);
    setError('');
    try {
      await finishSetup('local', 'http://localhost:11434/v1', 'llama3');
      setMode('done');
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  const handleRestartOpenclaw = async () => {
    setLoading(true);
    try {
      await restartOpenclaw();
      setResult('OpenClaw restarted. You\'re all set.');
    } catch {
      setResult('Could not restart automatically. Run: openclaw daemon restart');
    } finally {
      setLoading(false);
    }
  };

  const handleDone = async () => {
    window.location.reload();
  };

  const envValue = `http://localhost:${port}/v1`;
  const envLine = os === 'windows'
    ? `$env:OPENAI_API_BASE = "${envValue}"`
    : `export OPENAI_API_BASE=${envValue}`;

  const handleSetEnvPermanently = async () => {
    setEnvLoading(true);
    try {
      await setEnvPermanently(envValue);
      setEnvSet(true);
    } catch {
      // Silently fail — user can do it manually
    } finally {
      setEnvLoading(false);
    }
  };

  return (
    <div className="flex items-center justify-center min-h-screen p-4" style={{ backgroundColor: 'var(--bg-primary)' }}>
      <div className="w-full max-w-lg p-8 rounded-xl border" style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}>

        {mode === 'pick' && (
          <div>
            <div className="flex items-center gap-3 mb-3">
              <svg width="28" height="28" viewBox="0 0 512 512">
                <rect x="64" y="64" width="384" height="384" rx="48" ry="48" fill="none" stroke="#22C55E" strokeWidth="40"/>
              </svg>
              <h1 className="text-2xl font-bold">Welcome to GreenCube</h1>
            </div>
            <p className="text-base text-[var(--text-secondary)] mb-8">
              Your agent is about to get a memory. What do you use?
            </p>

            <div className="space-y-3">
              <button onClick={() => { setMode('openclaw'); handleOpenclaw(); }}
                className="w-full p-4 rounded-xl border text-left hover:border-[var(--accent)] transition-colors" style={{ borderColor: 'var(--border)', backgroundColor: 'var(--bg-tertiary)' }}>
                <div className="text-base font-semibold">OpenClaw</div>
                <div className="text-xs text-[var(--text-muted)] mt-1">Auto-configures everything. Zero manual steps.</div>
              </button>

              <button onClick={() => setMode('openai')}
                className="w-full p-4 rounded-xl border text-left hover:border-[var(--accent)] transition-colors" style={{ borderColor: 'var(--border)', backgroundColor: 'var(--bg-tertiary)' }}>
                <div className="text-base font-semibold">OpenAI / OpenRouter / Other</div>
                <div className="text-xs text-[var(--text-muted)] mt-1">Just need your API key.</div>
              </button>

              <button onClick={() => { setMode('ollama'); handleOllama(); }}
                className="w-full p-4 rounded-xl border text-left hover:border-[var(--accent)] transition-colors" style={{ borderColor: 'var(--border)', backgroundColor: 'var(--bg-tertiary)' }}>
                <div className="text-base font-semibold">Ollama (local)</div>
                <div className="text-xs text-[var(--text-muted)] mt-1">No API key needed. Runs on your machine.</div>
              </button>
            </div>
          </div>
        )}

        {mode === 'openclaw' && !error && (
          <div className="text-center py-6">
            <div className="text-[var(--text-muted)] mb-4">{loading ? 'Configuring OpenClaw...' : 'Done.'}</div>
            {loading && <div className="animate-pulse text-[var(--accent)]">Reading config...</div>}
          </div>
        )}

        {mode === 'openclaw' && error && (
          <div>
            <h1 className="text-2xl font-bold mb-4">OpenClaw</h1>
            <div className="p-4 rounded-lg border mb-6 text-sm" style={{ borderColor: 'var(--status-error)', color: 'var(--status-error)', backgroundColor: 'rgba(239,68,68,0.05)' }}>
              {error}
            </div>
            <button onClick={() => { setMode('pick'); setError(''); }}
              className="text-sm text-[var(--text-muted)] hover:text-[var(--text-primary)]">
              Back
            </button>
          </div>
        )}

        {mode === 'openai' && (
          <div>
            <h1 className="text-2xl font-bold mb-2">API Key</h1>
            <p className="text-sm text-[var(--text-muted)] mb-6">Your key stays on your computer. Never sent anywhere except OpenAI.</p>

            <input type="password" value={apiKey} onChange={(e) => setApiKey(e.target.value)}
              className="w-full mb-6 font-mono" placeholder="sk-..." autoFocus
              onKeyDown={(e) => e.key === 'Enter' && handleOpenai()} />

            {error && <p className="text-xs text-[var(--status-error)] mb-4">{error}</p>}

            <button onClick={handleOpenai} disabled={!apiKey.trim() || loading}
              className="w-full py-2.5 rounded-lg text-black font-semibold disabled:opacity-50 hover:brightness-110 transition"
              style={{ backgroundColor: 'var(--accent)' }}>
              {loading ? 'Setting up...' : 'Continue'}
            </button>

            <button onClick={() => { setMode('pick'); setError(''); }}
              className="w-full text-center mt-3 text-sm text-[var(--text-muted)] hover:text-[var(--text-primary)]">
              Back
            </button>
          </div>
        )}

        {mode === 'ollama' && !error && (
          <div className="text-center py-6">
            <div className="text-[var(--text-muted)]">{loading ? 'Setting up Ollama...' : 'Done.'}</div>
          </div>
        )}

        {mode === 'done' && (
          <div className="text-center py-4">
            <div className="w-12 h-12 rounded-full flex items-center justify-center mx-auto mb-5" style={{ backgroundColor: 'var(--accent-subtle)' }}>
              <span className="text-2xl" style={{ color: 'var(--accent)' }}>{'\u2713'}</span>
            </div>
            <h1 className="text-2xl font-bold mb-2">You're all set</h1>
            {result && <p className="text-sm text-[var(--text-secondary)] mb-4">{result}</p>}

            {/* OpenClaw: show restart button */}
            {result.includes('Configured') && (
              <button onClick={handleRestartOpenclaw} disabled={loading}
                className="px-6 py-2 rounded-lg text-sm font-medium border mb-4" style={{ borderColor: 'var(--accent)', color: 'var(--accent)' }}>
                {loading ? 'Restarting...' : 'Restart OpenClaw'}
              </button>
            )}

            {/* Non-OpenClaw: show env var */}
            {!result.includes('Configured') && (
              <div className="mb-4">
                <div className="rounded-lg border p-3 text-left" style={{ backgroundColor: 'var(--bg-tertiary)', borderColor: 'var(--border)' }}>
                  <p className="text-[10px] text-[var(--text-muted)] mb-1">Add this line before running your agent:</p>
                  <code className="text-xs font-mono break-all" style={{ color: 'var(--accent)' }}>{envLine}</code>
                </div>
                {!envSet ? (
                  <button onClick={handleSetEnvPermanently} disabled={envLoading}
                    className="w-full mt-2 py-2 rounded-lg text-sm font-medium border transition-colors"
                    style={{ borderColor: 'var(--accent)', color: 'var(--accent)', backgroundColor: 'rgba(34,197,94,0.04)' }}>
                    {envLoading ? 'Setting...' : 'Set it permanently'}
                  </button>
                ) : (
                  <div className="mt-2 py-2 rounded-lg text-sm font-medium text-center" style={{ color: 'var(--accent)' }}>
                    {'\u2713'} Done. Restart your terminal.
                  </div>
                )}
              </div>
            )}

            <div className="rounded-lg border p-4 mb-6 text-left" style={{ backgroundColor: 'rgba(34,197,94,0.04)', borderColor: 'rgba(34,197,94,0.15)' }}>
              <p className="text-[10px] mb-2" style={{ color: '#71717a' }}>Check on your agent anytime:</p>
              <code className="text-xl font-mono font-bold block mb-1" style={{ color: '#22c55e' }}>gc</code>
              <p className="text-[10px] font-mono" style={{ color: '#52525b' }}>or: curl localhost:{port}/brain</p>
            </div>

            <button onClick={handleDone}
              className="px-8 py-2.5 rounded-lg text-black font-semibold hover:brightness-110 transition"
              style={{ backgroundColor: 'var(--accent)' }}>
              Go to Dashboard
            </button>
            <p className="text-xs mt-3" style={{ color: '#71717a' }}>Close this window anytime. GreenCube keeps running in the system tray.</p>
          </div>
        )}
      </div>
    </div>
  );
}
