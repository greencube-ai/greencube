import { useState } from 'react';
import { useApp } from '../context/AppContext';
import { saveConfig, createAgent, getConfig } from '../lib/invoke';
import type { AppConfig } from '../lib/types';

const TOOLS = ['shell', 'read_file', 'write_file', 'http_get'];

export function OnboardingModal() {
  const { state, dispatch, refreshAgents } = useApp();
  const [step, setStep] = useState(1);
  const [apiKey, setApiKey] = useState('');
  const [apiBaseUrl, setApiBaseUrl] = useState('https://api.openai.com/v1');
  const [agentName, setAgentName] = useState('');
  const [systemPrompt, setSystemPrompt] = useState('You are a helpful assistant.');
  const [selectedTools, setSelectedTools] = useState<string[]>(['shell', 'read_file', 'write_file']);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');

  const handleContinue = async () => {
    if (!apiKey.trim()) return;
    setLoading(true);
    setError('');
    try {
      const config = state.config ?? await getConfig();
      const updated: AppConfig = {
        ...config,
        llm: { ...config.llm, api_key: apiKey, api_base_url: apiBaseUrl },
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
    if (!agentName.trim() || selectedTools.length === 0) return;
    setLoading(true);
    setError('');
    try {
      const agent = await createAgent(agentName, systemPrompt, selectedTools);
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
      <div
        className="w-full max-w-lg p-8 rounded-xl border"
        style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}
      >
        {step === 1 && (
          <div className="onboarding-step">
            <div className="flex items-center gap-3 mb-3">
              <div className="w-5 h-5 rounded-md logo-glow" style={{ backgroundColor: 'var(--accent)' }} />
              <h1 className="text-2xl font-bold">Welcome to GreenCube</h1>
            </div>
            <p className="text-base text-[var(--text-secondary)] mb-8">
              A world where AI agents live. Enter your API key to bring them to life.
            </p>

            <label className="block text-xs text-[var(--text-muted)] mb-1.5 uppercase tracking-wide">API Base URL</label>
            <input
              type="text"
              value={apiBaseUrl}
              onChange={(e) => setApiBaseUrl(e.target.value)}
              className="w-full mb-5"
              placeholder="https://api.openai.com/v1"
            />

            <label className="block text-xs text-[var(--text-muted)] mb-1.5 uppercase tracking-wide">API Key</label>
            <input
              type="password"
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              className="w-full mb-6 font-mono"
              placeholder="sk-..."
              onKeyDown={(e) => e.key === 'Enter' && handleContinue()}
            />

            {error && <p className="text-xs text-[var(--status-error)] mb-4">{error}</p>}

            <button
              onClick={handleContinue}
              disabled={!apiKey.trim() || loading}
              className="w-full py-2.5 rounded-lg text-black font-semibold disabled:opacity-50 hover:brightness-110 transition"
              style={{ backgroundColor: 'var(--accent)' }}
            >
              {loading ? 'Saving...' : 'Continue'}
            </button>

            <p className="text-[10px] text-[var(--text-muted)] mt-4 text-center">
              Works with OpenAI, Ollama, LM Studio, OpenRouter, or any OpenAI-compatible endpoint.
            </p>
          </div>
        )}

        {step === 2 && (
          <div className="onboarding-step">
            <h1 className="text-2xl font-bold mb-2">Create your first agent</h1>
            <p className="text-base text-[var(--text-secondary)] mb-8">
              Give it a name and choose its tools. You can always change these later.
            </p>

            <label className="block text-xs text-[var(--text-muted)] mb-1.5 uppercase tracking-wide">Agent Name</label>
            <input
              type="text"
              value={agentName}
              onChange={(e) => setAgentName(e.target.value)}
              className="w-full mb-5"
              placeholder="e.g. CodeBot"
              autoFocus
            />

            <label className="block text-xs text-[var(--text-muted)] mb-1.5 uppercase tracking-wide">System Prompt</label>
            <textarea
              value={systemPrompt}
              onChange={(e) => setSystemPrompt(e.target.value)}
              className="w-full mb-5 h-20 resize-none"
              placeholder="You are a helpful assistant."
            />

            <label className="block text-xs text-[var(--text-muted)] mb-2.5 uppercase tracking-wide">Tools</label>
            <div className="flex flex-wrap gap-2 mb-6">
              {TOOLS.map((tool) => (
                <button
                  key={tool}
                  onClick={() => toggleTool(tool)}
                  className={`px-3.5 py-1.5 rounded-md text-xs border transition-colors ${
                    selectedTools.includes(tool)
                      ? 'text-[var(--accent)] border-[var(--accent)]'
                      : 'text-[var(--text-muted)] border-[var(--border)] hover:border-[var(--border-hover)]'
                  }`}
                  style={selectedTools.includes(tool) ? { backgroundColor: 'var(--accent-subtle)' } : undefined}
                >
                  {tool}
                </button>
              ))}
            </div>

            {error && <p className="text-xs text-[var(--status-error)] mb-4">{error}</p>}

            <button
              onClick={handleCreateAgent}
              disabled={!agentName.trim() || selectedTools.length === 0 || loading}
              className="w-full py-2.5 rounded-lg text-black font-semibold disabled:opacity-50 hover:brightness-110 transition"
              style={{ backgroundColor: 'var(--accent)' }}
            >
              {loading ? 'Creating...' : 'Create Agent'}
            </button>
          </div>
        )}

        {step === 3 && (
          <div className="onboarding-step text-center py-6">
            <div
              className="w-12 h-12 rounded-full flex items-center justify-center mx-auto mb-5"
              style={{ backgroundColor: 'var(--accent-subtle)' }}
            >
              <span className="text-2xl" style={{ color: 'var(--accent)' }}>✓</span>
            </div>
            <h1 className="text-2xl font-bold mb-2">You're all set</h1>
            <p className="text-base text-[var(--text-secondary)] mb-8">
              Your agent is alive. The API is running. Let's go.
            </p>
            <button
              onClick={() => window.location.reload()}
              className="px-8 py-2.5 rounded-lg text-black font-semibold hover:brightness-110 transition"
              style={{ backgroundColor: 'var(--accent)' }}
            >
              Go to Dashboard
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
