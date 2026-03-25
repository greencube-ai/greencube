import { useState, useRef, useEffect } from 'react';
import type { Agent } from '../lib/types';

interface ChatMessage {
  role: 'user' | 'assistant';
  content: string;
}

interface ChatPanelProps {
  agents: Agent[];
  apiPort: number;
  hasApiKey: boolean;
}

export function ChatPanel({ agents, apiPort, hasApiKey }: ChatPanelProps) {
  const [selectedAgentId, setSelectedAgentId] = useState(agents[0]?.id || '');
  const [input, setInput] = useState('');
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [loading, setLoading] = useState(false);
  const messagesEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

  // Update selected agent if agents list changes
  useEffect(() => {
    if (!selectedAgentId && agents.length > 0) {
      setSelectedAgentId(agents[0].id);
    }
  }, [agents, selectedAgentId]);

  if (!hasApiKey) {
    return (
      <div
        className="rounded-lg border p-6 text-center"
        style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}
      >
        <p className="text-sm text-[var(--text-muted)]">
          Set your API key in Settings to start chatting with your agents.
        </p>
      </div>
    );
  }

  const sendMessage = async () => {
    if (!input.trim() || !selectedAgentId || loading) return;
    const userMsg: ChatMessage = { role: 'user', content: input };
    setMessages((prev) => [...prev, userMsg]);
    setInput('');
    setLoading(true);
    try {
      const allMessages = [...messages, userMsg].map((m) => ({
        role: m.role,
        content: m.content,
      }));
      const resp = await fetch(
        `http://localhost:${apiPort}/v1/chat/completions`,
        {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            'x-agent-id': selectedAgentId,
          },
          body: JSON.stringify({
            model: 'gpt-4o',
            messages: allMessages,
          }),
        }
      );
      const data = await resp.json();
      if (data.error) {
        setMessages((prev) => [
          ...prev,
          { role: 'assistant', content: `Error: ${data.error}` },
        ]);
      } else {
        const content =
          data.choices?.[0]?.message?.content || 'No response received.';
        setMessages((prev) => [...prev, { role: 'assistant', content }]);
      }
    } catch (e) {
      setMessages((prev) => [
        ...prev,
        { role: 'assistant', content: `Connection error: ${e}` },
      ]);
    } finally {
      setLoading(false);
    }
  };

  const selectedAgent = agents.find((a) => a.id === selectedAgentId);

  return (
    <div
      className="rounded-lg border overflow-hidden"
      style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}
    >
      {/* Header */}
      <div
        className="flex items-center justify-between px-4 py-3 border-b"
        style={{ borderColor: 'var(--border)' }}
      >
        <div className="flex items-center gap-3">
          <h3 className="text-sm font-medium">Try it now</h3>
          <select
            value={selectedAgentId}
            onChange={(e) => setSelectedAgentId(e.target.value)}
            className="text-xs py-1 px-2"
          >
            {agents.map((a) => (
              <option key={a.id} value={a.id}>
                {a.name}
              </option>
            ))}
          </select>
        </div>
        {selectedAgent && (
          <span className="text-[10px] text-[var(--text-muted)]">
            via localhost:{apiPort}
          </span>
        )}
      </div>

      {/* Messages */}
      <div className="px-4 py-3 max-h-64 overflow-y-auto space-y-3 min-h-[80px]">
        {messages.length === 0 && (
          <p className="text-xs text-[var(--text-muted)] text-center py-4">
            Send a message to test your agent.
          </p>
        )}
        {messages.map((msg, i) => (
          <div
            key={i}
            className={`text-sm ${msg.role === 'user' ? 'text-right' : ''}`}
          >
            <div
              className={`inline-block px-3 py-2 rounded-lg max-w-[80%] text-left ${
                msg.role === 'user' ? 'ml-auto' : ''
              }`}
              style={{
                backgroundColor:
                  msg.role === 'user'
                    ? 'var(--accent-subtle)'
                    : 'var(--bg-tertiary)',
                color:
                  msg.role === 'user'
                    ? 'var(--accent)'
                    : 'var(--text-primary)',
              }}
            >
              {msg.content}
            </div>
          </div>
        ))}
        {loading && (
          <div className="text-xs text-[var(--text-muted)]">
            <span className="status-pulse inline-block w-1.5 h-1.5 rounded-full mr-1.5" style={{ backgroundColor: 'var(--accent)' }} />
            Thinking...
          </div>
        )}
        <div ref={messagesEndRef} />
      </div>

      {/* Input */}
      <div className="flex gap-2 px-4 py-3 border-t" style={{ borderColor: 'var(--border)' }}>
        <input
          type="text"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && !e.shiftKey && sendMessage()}
          placeholder="Send a message..."
          className="flex-1 text-sm"
          disabled={loading}
        />
        <button
          onClick={sendMessage}
          disabled={!input.trim() || loading}
          className="px-4 py-2 rounded-lg text-black text-sm font-medium disabled:opacity-40 hover:brightness-110 transition"
          style={{ backgroundColor: 'var(--accent)' }}
        >
          Send
        </button>
      </div>
    </div>
  );
}
