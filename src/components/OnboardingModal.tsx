import { useState } from 'react';
import { useApp } from '../context/AppContext';
import { saveConfig, createAgent, createProvider, getConfig } from '../lib/invoke';
import type { AppConfig } from '../lib/types';

const PRESETS = [
  { name: 'OpenAI', url: 'https://api.openai.com/v1', model: 'gpt-4o', type: 'openai', placeholder: 'sk-...' },
  { name: 'Ollama (local)', url: 'http://localhost:11434/v1', model: 'llama3', type: 'ollama', placeholder: 'not needed for local' },
  { name: 'OpenRouter', url: 'https://openrouter.ai/api/v1', model: 'openai/gpt-4o', type: 'openai', placeholder: 'sk-or-...' },
  { name: 'LM Studio', url: 'http://localhost:1234/v1', model: 'local-model', type: 'openai', placeholder: 'not needed for local' },
];

const TOOL_INFO: Record<string, { label: string; desc: string }> = {
  shell: { label: 'Run commands', desc: 'Let your agent run terminal commands on your computer' },
  read_file: { label: 'Read files', desc: 'Let your agent read files from your disk' },
  write_file: { label: 'Write files', desc: 'Let your agent create and edit files' },
  http_get: { label: 'Browse the web', desc: 'Let your agent fetch web pages and APIs' },
  update_context: { label: 'Take notes', desc: 'Let your agent write down things to remember' },
};

export function OnboardingModal() {
  const { state, dispatch, refreshAgents } = useApp();
  const [step, setStep] = useState(1);
  const [preset, setPreset] = useState(0);
  const [apiKey, setApiKey] = useState('');
  const [createdProviderId, setCreatedProviderId] = useState('');
  const [agentName, setAgentName] = useState('Dev');
  const [selectedTools, setSelectedTools] = useState<string[]>(['shell', 'read_file', 'write_file', 'update_context']);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');

  const p = PRESETS[preset];
  const isLocal = p.type === 'ollama' || p.name === 'LM Studio';

  const handleContinue = async () => {
    if (!isLocal && !apiKey.trim()) return;
    setLoading(true);
    setError('');
    try {
      const key = isLocal ? 'local' : apiKey;
      const provider = await createProvider(p.name, p.url, key, p.model, p.type);
      setCreatedProviderId(provider.id);

      const config = state.config ?? await getConfig();
      const updated: AppConfig = {
        ...config,
        llm: { ...config.llm, api_key: key, api_base_url: p.url, default_model: p.model },
      };
      await saveConfig(updated);
      dispatch({ type: 'SET_CONFIG', config: updated });
      setStep(2);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  const handleCreateAgent = async () => {
    if (!agentName.trim()) return;
    setLoading(true);
    setError('');
    try {
      const agent = await createAgent(agentName, 'You are a helpful assistant.', selectedTools, createdProviderId || undefined);
      dispatch({ type: 'ADD_AGENT', agent });
      const config = state.config ?? await getConfig();
      const updated: AppConfig = { ...config, ui: { ...config.ui, onboarding_complete: true } };
      await saveConfig(updated);
      dispatch({ type: 'SET_CONFIG', config: updated });
      await refreshAgents();
      setStep(3);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  const toggleTool = (tool: string) => {
    setSelectedTools((prev) =>
      prev.includes(tool) ? prev.filter((t) => t !== tool) : [...prev, tool]
    );
  };

  return (
    <div className="flex items-center justify-center min-h-screen p-4" style={{ backgroundColor: 'var(--bg-primary)' }}>
      <div className="w-full max-w-lg p-8 rounded-xl border" style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}>
        {step === 1 && (
          <div>
            <div className="flex items-center gap-3 mb-3">
              <svg width="28" height="28" viewBox="0 0 512 512">
                <rect x="64" y="64" width="384" height="384" rx="48" ry="48" fill="none" stroke="#22C55E" strokeWidth="40"/>
              </svg>
              <h1 className="text-2xl font-bold">Welcome to GreenCube</h1>
            </div>
            <p className="text-base text-[var(--text-secondary)] mb-8">
              Your agent is about to get a memory. First, pick your AI provider.
            </p>

            {/* Provider presets */}
            <label className="block text-xs text-[var(--text-muted)] mb-2 uppercase tracking-wide">Provider</label>
            <div className="grid grid-cols-2 gap-2 mb-6">
              {PRESETS.map((pr, i) => (
                <button key={pr.name} onClick={() => { setPreset(i); setApiKey(''); }}
                  className={`px-3 py-2.5 rounded-lg text-sm text-left border transition-colors ${
                    preset === i ? 'text-[var(--accent)] border-[var(--accent)]' : 'text-[var(--text-secondary)] border-[var(--border)] hover:border-[var(--border-hover)]'
                  }`}
                  style={preset === i ? { backgroundColor: 'var(--accent-subtle)' } : undefined}>
                  {pr.name}
                </button>
              ))}
            </div>

            {/* API Key (skip for local) */}
            {!isLocal ? (
              <div className="mb-6">
                <label className="block text-xs text-[var(--text-muted)] mb-1.5 uppercase tracking-wide">API Key</label>
                <input type="password" value={apiKey} onChange={(e) => setApiKey(e.target.value)}
                  className="w-full font-mono" placeholder={p.placeholder}
                  onKeyDown={(e) => e.key === 'Enter' && handleContinue()} />
                <p className="text-[10px] text-[var(--text-muted)] mt-1.5">Your key stays on your computer. Never sent anywhere except {p.name}.</p>
              </div>
            ) : (
              <div className="mb-6 p-3 rounded-lg border text-xs text-[var(--text-muted)]" style={{ borderColor: 'var(--border)', backgroundColor: 'var(--bg-tertiary)' }}>
                No API key needed. Make sure {p.name} is running on your machine.
              </div>
            )}

            {error && <p className="text-xs text-[var(--status-error)] mb-4">{error}</p>}

            <button onClick={handleContinue} disabled={(!isLocal && !apiKey.trim()) || loading}
              className="w-full py-2.5 rounded-lg text-black font-semibold disabled:opacity-50 hover:brightness-110 transition"
              style={{ backgroundColor: 'var(--accent)' }}>
              {loading ? 'Setting up...' : 'Continue'}
            </button>
          </div>
        )}

        {step === 2 && (
          <div>
            <h1 className="text-2xl font-bold mb-2">Create your agent</h1>
            <p className="text-base text-[var(--text-secondary)] mb-6">
              Give it a name. Pick what it's allowed to do.
            </p>

            <label className="block text-xs text-[var(--text-muted)] mb-1.5 uppercase tracking-wide">Name</label>
            <input type="text" value={agentName} onChange={(e) => setAgentName(e.target.value)} className="w-full mb-6" placeholder="e.g. Dev" autoFocus />

            <label className="block text-xs text-[var(--text-muted)] mb-2.5 uppercase tracking-wide">Permissions</label>
            <div className="space-y-2 mb-6">
              {Object.entries(TOOL_INFO).map(([tool, info]) => (
                <button key={tool} onClick={() => toggleTool(tool)}
                  className="w-full flex items-start gap-3 px-3.5 py-2.5 rounded-lg text-left border transition-colors"
                  style={{
                    borderColor: selectedTools.includes(tool) ? 'var(--accent)' : 'var(--border)',
                    backgroundColor: selectedTools.includes(tool) ? 'var(--accent-subtle)' : 'transparent',
                  }}>
                  <div className="w-4 h-4 mt-0.5 rounded border flex-shrink-0 flex items-center justify-center text-[10px]"
                    style={{
                      borderColor: selectedTools.includes(tool) ? 'var(--accent)' : 'var(--border)',
                      color: selectedTools.includes(tool) ? 'var(--accent)' : 'transparent',
                    }}>
                    {selectedTools.includes(tool) && '\u2713'}
                  </div>
                  <div>
                    <div className="text-sm font-medium" style={{ color: selectedTools.includes(tool) ? 'var(--accent)' : 'var(--text-primary)' }}>{info.label}</div>
                    <div className="text-xs text-[var(--text-muted)]">{info.desc}</div>
                  </div>
                </button>
              ))}
            </div>

            {error && <p className="text-xs text-[var(--status-error)] mb-4">{error}</p>}

            <button onClick={handleCreateAgent} disabled={!agentName.trim() || loading}
              className="w-full py-2.5 rounded-lg text-black font-semibold disabled:opacity-50 hover:brightness-110 transition"
              style={{ backgroundColor: 'var(--accent)' }}>
              {loading ? 'Creating...' : 'Create Agent'}
            </button>
          </div>
        )}

        {step === 3 && (
          <div className="text-center py-6">
            <div className="w-12 h-12 rounded-full flex items-center justify-center mx-auto mb-5" style={{ backgroundColor: 'var(--accent-subtle)' }}>
              <span className="text-2xl" style={{ color: 'var(--accent)' }}>{'\u2713'}</span>
            </div>
            <h1 className="text-2xl font-bold mb-2">You're all set</h1>
            <p className="text-base text-[var(--text-secondary)] mb-3">Your agent has a memory now. Connect it and start working.</p>
            <div className="rounded-lg border p-3 mb-6 text-left" style={{ backgroundColor: 'var(--bg-tertiary)', borderColor: 'var(--border)' }}>
              <code className="text-xs font-mono" style={{ color: 'var(--accent)' }}>export OPENAI_API_BASE=http://localhost:9000/v1</code>
              <p className="text-[10px] text-[var(--text-muted)] mt-1">Add this line before running your agent. Then check on it anytime:</p>
              <code className="text-[10px] font-mono text-[var(--text-muted)]">curl localhost:9000/brain</code>
            </div>
            <button onClick={() => window.location.reload()}
              className="px-8 py-2.5 rounded-lg text-black font-semibold hover:brightness-110 transition"
              style={{ backgroundColor: 'var(--accent)' }}>
              Go to Dashboard
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
