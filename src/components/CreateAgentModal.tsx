import { useState } from 'react';
import { createAgent } from '../lib/invoke';
import type { Agent } from '../lib/types';

const TOOLS = ['shell', 'read_file', 'write_file', 'http_get'];

interface CreateAgentModalProps {
  isOpen: boolean;
  onClose: () => void;
  onCreated: (agent: Agent) => void;
}

export function CreateAgentModal({ isOpen, onClose, onCreated }: CreateAgentModalProps) {
  const [name, setName] = useState('');
  const [systemPrompt, setSystemPrompt] = useState('');
  const [selectedTools, setSelectedTools] = useState<string[]>(['shell', 'read_file', 'write_file']);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');

  if (!isOpen) return null;

  const toggleTool = (tool: string) => {
    setSelectedTools((prev) =>
      prev.includes(tool) ? prev.filter((t) => t !== tool) : [...prev, tool]
    );
  };

  const handleCreate = async () => {
    if (!name.trim() || selectedTools.length === 0) return;
    setLoading(true);
    setError('');
    try {
      const agent = await createAgent(name, systemPrompt, selectedTools);
      onCreated(agent);
      setName('');
      setSystemPrompt('');
      setSelectedTools(['shell', 'read_file', 'write_file']);
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50" onClick={onClose}>
      <div
        className="w-full max-w-md p-6 rounded-lg border"
        style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}
        onClick={(e) => e.stopPropagation()}
      >
        <h2 className="text-lg font-bold mb-4">New Agent</h2>

        <label className="block text-xs text-[var(--text-muted)] mb-1">Name</label>
        <input
          type="text"
          value={name}
          onChange={(e) => setName(e.target.value)}
          className="w-full mb-4"
          placeholder="e.g. DataBot"
          autoFocus
        />

        <label className="block text-xs text-[var(--text-muted)] mb-1">System Prompt (optional)</label>
        <textarea
          value={systemPrompt}
          onChange={(e) => setSystemPrompt(e.target.value)}
          className="w-full mb-4 h-20 resize-none"
          placeholder="You are a helpful assistant."
        />

        <label className="block text-xs text-[var(--text-muted)] mb-2">Tools</label>
        <div className="flex flex-wrap gap-2 mb-4">
          {TOOLS.map((tool) => (
            <button
              key={tool}
              onClick={() => toggleTool(tool)}
              className={`px-3 py-1 rounded-md text-xs border transition-colors ${
                selectedTools.includes(tool)
                  ? 'text-[var(--accent)] border-[var(--accent)]'
                  : 'text-[var(--text-muted)] border-[var(--border)]'
              }`}
              style={selectedTools.includes(tool) ? { backgroundColor: 'var(--accent-subtle)' } : undefined}
            >
              {tool}
            </button>
          ))}
        </div>

        {error && <p className="text-xs text-[var(--status-error)] mb-4">{error}</p>}

        <div className="flex gap-2">
          <button
            onClick={onClose}
            className="flex-1 py-2 rounded-lg border text-sm"
            style={{ borderColor: 'var(--border)', color: 'var(--text-secondary)' }}
          >
            Cancel
          </button>
          <button
            onClick={handleCreate}
            disabled={!name.trim() || selectedTools.length === 0 || loading}
            className="flex-1 py-2 rounded-lg text-black font-medium text-sm disabled:opacity-50"
            style={{ backgroundColor: 'var(--accent)' }}
          >
            {loading ? 'Creating...' : 'Create'}
          </button>
        </div>
      </div>
    </div>
  );
}
