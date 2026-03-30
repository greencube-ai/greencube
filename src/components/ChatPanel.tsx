import { useState, useRef, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { Agent } from '../lib/types';

interface ChatMessage {
  role: 'user' | 'assistant';
  content: string;
  streaming?: boolean;
  error?: boolean;
  taskId?: string;
  rated?: number; // 1 or -1
}

interface ChatPanelProps {
  agents: Agent[];
  apiPort: number;
  hasApiKey: boolean;
}

export function ChatPanel({ agents, apiPort, hasApiKey }: ChatPanelProps) {
  const [selectedAgentId, setSelectedAgentId] = useState(agents[0]?.id || '');
  const [input, setInput] = useState('');
  // Per-agent chat history: Record<agentId, ChatMessage[]>
  const [chatHistories, setChatHistories] = useState<Record<string, ChatMessage[]>>({});
  const [loading, setLoading] = useState(false);
  const abortRef = useRef<AbortController | null>(null);

  const messages = chatHistories[selectedAgentId] || [];

  const setMessages = useCallback(
    (updater: ChatMessage[] | ((prev: ChatMessage[]) => ChatMessage[])) => {
      setChatHistories((prev) => {
        const current = prev[selectedAgentId] || [];
        const next = typeof updater === 'function' ? updater(current) : updater;
        return { ...prev, [selectedAgentId]: next };
      });
    },
    [selectedAgentId]
  );

  useEffect(() => {
    if (!selectedAgentId && agents.length > 0) {
      setSelectedAgentId(agents[0].id);
    }
  }, [agents, selectedAgentId]);

  if (!hasApiKey) {
    return (
      <div className="rounded-lg border p-6 text-center" style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}>
        <p className="text-sm text-[var(--text-muted)]">Set your API key in Settings to start chatting with your agents.</p>
      </div>
    );
  }

  const sendMessage = async () => {
    if (!input.trim() || !selectedAgentId || loading) return;
    const userMsg: ChatMessage = { role: 'user', content: input };
    const allMsgs = [...messages, userMsg];
    setMessages(allMsgs);
    setInput('');
    setLoading(true);

    // Add placeholder assistant message for streaming
    const assistantIdx = allMsgs.length;
    setMessages([...allMsgs, { role: 'assistant', content: '', streaming: true }]);

    const controller = new AbortController();
    abortRef.current = controller;

    // Set a 60-second timeout
    const timeout = setTimeout(() => controller.abort(), 60000);

    try {
      const resp = await fetch(`http://127.0.0.1:${apiPort}/v1/chat/completions`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', 'x-agent-id': selectedAgentId },
        body: JSON.stringify({
          model: 'gpt-4o',
          stream: true,
          messages: allMsgs.map((m) => ({ role: m.role, content: m.content })),
        }),
        signal: controller.signal,
      });

      clearTimeout(timeout);
      const contentType = resp.headers.get('content-type') || '';

      if (contentType.includes('text/event-stream') && resp.body) {
        // SSE streaming path
        const reader = resp.body.getReader();
        const decoder = new TextDecoder();
        let accumulated = '';
        let buffer = '';

        while (true) {
          const { done, value } = await reader.read();
          if (done) break;

          buffer += decoder.decode(value, { stream: true });
          const lines = buffer.split('\n');
          buffer = lines.pop() || ''; // Keep incomplete line in buffer

          for (const line of lines) {
            const trimmed = line.trim();
            if (!trimmed || !trimmed.startsWith('data: ')) continue;
            const data = trimmed.slice(6);
            if (data === '[DONE]') break;

            try {
              const parsed = JSON.parse(data);
              const delta = parsed.choices?.[0]?.delta?.content;
              if (delta) {
                accumulated += delta;
                setMessages((prev) => {
                  const copy = [...prev];
                  copy[assistantIdx] = { role: 'assistant', content: accumulated, streaming: true };
                  return copy;
                });
              }
            } catch {
              // Skip unparseable chunks
            }
          }
        }

        // Mark streaming complete
        setMessages((prev) => {
          const copy = [...prev];
          copy[assistantIdx] = {
            role: 'assistant',
            content: accumulated || 'No response received.',
            streaming: false,
          };
          return copy;
        });
      } else {
        // JSON response (non-streaming or error)
        const data = await resp.json();
        const content = data.error
          ? `Error: ${data.error}`
          : data.choices?.[0]?.message?.content || 'No response received.';
        const taskId = data.greencube_task_id;
        setMessages((prev) => {
          const copy = [...prev];
          copy[assistantIdx] = { role: 'assistant', content, error: !!data.error, taskId };
          return copy;
        });
      }
    } catch (e) {
      clearTimeout(timeout);
      const isAbort = (e as Error).name === 'AbortError';
      setMessages((prev) => {
        const copy = [...prev];
        const existing = copy[assistantIdx]?.content || '';
        copy[assistantIdx] = {
          role: 'assistant',
          content: existing
            ? `${existing}\n\n[Response interrupted]`
            : isAbort
              ? 'Response timed out after 60 seconds.'
              : `Connection error: ${e}`,
          error: true,
          streaming: false,
        };
        return copy;
      });
    } finally {
      setLoading(false);
      abortRef.current = null;
    }
  };

  const selectedAgent = agents.find((a) => a.id === selectedAgentId);

  return (
    <div className="rounded-lg border overflow-hidden" style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}>
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 border-b" style={{ borderColor: 'var(--border)' }}>
        <div className="flex items-center gap-3">
          <h3 className="text-sm font-medium">Try it now</h3>
          <select
            value={selectedAgentId}
            onChange={(e) => setSelectedAgentId(e.target.value)}
            className="text-xs py-1 px-2"
          >
            {agents.map((a) => (
              <option key={a.id} value={a.id}>{a.name}</option>
            ))}
          </select>
        </div>
        {selectedAgent && (
          <span className="text-[10px] text-[var(--text-muted)]">streaming via localhost:{apiPort}</span>
        )}
      </div>

      {/* Messages */}
      <div className="px-4 py-3 max-h-80 overflow-y-auto space-y-3 min-h-[80px]">
        {messages.length === 0 && (
          <p className="text-xs text-[var(--text-muted)] text-center py-4">Send a message to test your agent.</p>
        )}
        {messages.map((msg, i) => (
          <div key={i} className={`text-sm ${msg.role === 'user' ? 'text-right' : ''}`}>
            <div
              className={`inline-block px-3 py-2 rounded-lg max-w-[80%] text-left whitespace-pre-wrap ${msg.role === 'user' ? 'ml-auto' : ''}`}
              style={{
                backgroundColor: msg.role === 'user' ? 'var(--accent-subtle)' : 'var(--bg-tertiary)',
                color: msg.error ? 'var(--status-error)' : msg.role === 'user' ? 'var(--accent)' : 'var(--text-primary)',
              }}
            >
              {msg.content || (msg.streaming ? '' : 'No response')}
              {msg.streaming && (
                <span className="inline-block w-1 h-3 ml-0.5 rounded-full animate-pulse" style={{ backgroundColor: 'var(--accent)' }} />
              )}
            </div>
            {msg.role === 'assistant' && !msg.streaming && msg.taskId && !msg.error && (
              <div className="flex gap-1 mt-1">
                <button
                  onClick={() => {
                    invoke('rate_response', { agentId: selectedAgentId, taskId: msg.taskId, rating: 1 }).catch(console.error);
                    setMessages(prev => {
                      const copy = [...prev];
                      copy[i] = { ...copy[i], rated: 1 };
                      return copy;
                    });
                  }}
                  disabled={msg.rated !== undefined}
                  className="text-[10px] px-1.5 py-0.5 rounded transition-colors"
                  style={{
                    color: msg.rated === 1 ? 'var(--accent)' : 'var(--text-muted)',
                    opacity: msg.rated !== undefined && msg.rated !== 1 ? 0.3 : 1,
                  }}
                >
                  good
                </button>
                <button
                  onClick={() => {
                    invoke('rate_response', { agentId: selectedAgentId, taskId: msg.taskId, rating: -1 }).catch(console.error);
                    setMessages(prev => {
                      const copy = [...prev];
                      copy[i] = { ...copy[i], rated: -1 };
                      return copy;
                    });
                  }}
                  disabled={msg.rated !== undefined}
                  className="text-[10px] px-1.5 py-0.5 rounded transition-colors"
                  style={{
                    color: msg.rated === -1 ? 'var(--status-error)' : 'var(--text-muted)',
                    opacity: msg.rated !== undefined && msg.rated !== -1 ? 0.3 : 1,
                  }}
                >
                  bad
                </button>
              </div>
            )}
          </div>
        ))}
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
