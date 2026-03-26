import { useState, useEffect, useRef, useCallback } from 'react';
import { useNavigate } from 'react-router-dom';
import { useApp } from '../context/AppContext';
import { getActivityFeed, getServerInfo } from '../lib/invoke';
import { onActivityRefresh, onAgentStatusChange } from '../lib/events';
import { AgentCard } from '../components/AgentCard';
import { ActivityFeed } from '../components/ActivityFeed';
import { CreateAgentModal } from '../components/CreateAgentModal';
import { ChatPanel } from '../components/ChatPanel';
import { EmptyState } from '../components/EmptyState';
import type { AuditEntry } from '../lib/types';

export function Dashboard() {
  const { state, dispatch, refreshAgents } = useApp();
  const navigate = useNavigate();
  const [showCreate, setShowCreate] = useState(false);
  const [activity, setActivity] = useState<AuditEntry[]>([]);
  const [apiPort, setApiPort] = useState(9000);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Only update activity state if data actually changed (prevents scroll reset)
  const activityRef = useRef<string>('');
  const updateActivity = useCallback((entries: AuditEntry[]) => {
    const key = entries.length > 0 ? entries[0].id + entries.length : '';
    if (key !== activityRef.current) {
      activityRef.current = key;
      setActivity(entries);
    }
  }, []);

  const debouncedRefresh = useCallback(() => {
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => {
      getActivityFeed(50).then(updateActivity).catch(console.error);
    }, 500);
  }, [updateActivity]);

  useEffect(() => {
    getActivityFeed(50).then(updateActivity).catch(console.error);
    getServerInfo().then((info) => setApiPort(info.port)).catch(() => {});
    const interval = setInterval(() => {
      getActivityFeed(50).then(updateActivity).catch(console.error);
    }, 5000);
    return () => clearInterval(interval);
  }, [updateActivity]);

  useEffect(() => {
    const unlistenRefresh = onActivityRefresh(() => {
      debouncedRefresh();
    });
    const unlistenStatus = onAgentStatusChange((data) => {
      dispatch({ type: 'UPDATE_AGENT_STATUS', id: data.id, status: data.status });
    });
    return () => {
      unlistenRefresh.then((fn) => fn());
      unlistenStatus.then((fn) => fn());
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, [dispatch, debouncedRefresh]);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === 'n') {
        e.preventDefault();
        setShowCreate(true);
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, []);

  const agentNames: Record<string, string> = {};
  state.agents.forEach((a) => { agentNames[a.id] = a.name; });
  const hasApiKey = state.config?.llm.api_key && state.config.llm.api_key.length > 0;

  return (
    <div>
      {!state.dockerAvailable && (
        <div
          className="mb-5 px-4 py-3 rounded-xl text-sm border"
          style={{ backgroundColor: 'rgba(234, 179, 8, 0.06)', borderColor: 'rgba(234, 179, 8, 0.2)', color: '#eab308' }}
        >
          Docker not detected. Tool execution is disabled.
        </div>
      )}

      <div className="flex items-center justify-between mb-8">
        <div>
          <h1 className="text-2xl font-bold mb-1">Dashboard</h1>
          <p className="text-xs text-[var(--text-muted)]">
            {state.agents.length} agent{state.agents.length !== 1 ? 's' : ''} registered
          </p>
        </div>
        <button
          onClick={() => setShowCreate(true)}
          className="px-5 py-2.5 rounded-lg text-black text-sm font-medium hover:brightness-110 transition"
          style={{ backgroundColor: 'var(--accent)' }}
          title="Ctrl+N"
        >
          New Agent
        </button>
      </div>

      {state.agents.length === 0 ? (
        <div className="mt-12">
          <EmptyState
            message="No agents yet"
            subtitle="Create your first agent to get started. They'll remember everything."
          />
          <div className="text-center mt-8">
            <button
              onClick={() => setShowCreate(true)}
              className="px-8 py-3 rounded-lg text-black text-sm font-semibold hover:brightness-110 transition"
              style={{ backgroundColor: 'var(--accent)' }}
            >
              Create Your First Agent
            </button>
          </div>
        </div>
      ) : (
        <>
          {/* Chat panel first — this is the main action */}
          <ChatPanel agents={state.agents} apiPort={apiPort} hasApiKey={!!hasApiKey} />

          {/* Agents + Activity below */}
          <div className="grid grid-cols-1 lg:grid-cols-3 gap-5 mt-6">
            <div className="lg:col-span-2">
              <h2 className="text-sm font-medium text-[var(--text-secondary)] mb-3">Agents</h2>
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
                {state.agents.map((agent) => (
                  <AgentCard key={agent.id} agent={agent} onClick={() => navigate(`/agent/${agent.id}`)} />
                ))}
              </div>
            </div>
            <div>
              <h2 className="text-sm font-medium text-[var(--text-secondary)] mb-3">Activity</h2>
              <div
                className="rounded-xl border p-4"
                style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}
              >
                <ActivityFeed
                  entries={activity}
                  agentNames={agentNames}
                  emptyMessage="Send a message to see activity here"
                />
              </div>
            </div>
          </div>
        </>
      )}

      <CreateAgentModal
        isOpen={showCreate}
        onClose={() => setShowCreate(false)}
        onCreated={(agent) => {
          dispatch({ type: 'ADD_AGENT', agent });
          refreshAgents();
        }}
      />
    </div>
  );
}
