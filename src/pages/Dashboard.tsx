import { useState, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import { useApp } from '../context/AppContext';
import { getActivityFeed, getServerInfo } from '../lib/invoke';
import { onActivityUpdate, onAgentStatusChange } from '../lib/events';
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

  // Load activity feed + server port
  useEffect(() => {
    getActivityFeed(50).then(setActivity).catch(console.error);
    getServerInfo().then((info) => setApiPort(info.port)).catch(() => {});
  }, []);

  // Listen for real-time events
  useEffect(() => {
    const unlistenActivity = onActivityUpdate((entry) => {
      setActivity((prev) => [entry, ...prev].slice(0, 100));
    });
    const unlistenStatus = onAgentStatusChange((data) => {
      dispatch({ type: 'UPDATE_AGENT_STATUS', id: data.id, status: data.status });
    });
    return () => {
      unlistenActivity.then((fn) => fn());
      unlistenStatus.then((fn) => fn());
    };
  }, [dispatch]);

  // Cmd/Ctrl+N to create new agent
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
      {/* Docker warning banner */}
      {!state.dockerAvailable && (
        <div
          className="mb-4 px-4 py-2.5 rounded-lg text-sm border"
          style={{
            backgroundColor: 'rgba(234, 179, 8, 0.08)',
            borderColor: 'rgba(234, 179, 8, 0.25)',
            color: '#eab308',
          }}
        >
          Docker not detected. Tool execution is disabled.
        </div>
      )}

      {/* Header */}
      <div className="flex items-center justify-between mb-6">
        <h1 className="text-xl font-bold">Dashboard</h1>
        <button
          onClick={() => setShowCreate(true)}
          className="px-4 py-2 rounded-lg text-black text-sm font-medium hover:brightness-110 transition"
          style={{ backgroundColor: 'var(--accent)' }}
          title="Ctrl+N"
        >
          + New Agent
        </button>
      </div>

      {state.agents.length === 0 ? (
        <div className="mt-16">
          <EmptyState
            message="No agents yet"
            subtitle="They won't build themselves... or will they?"
          />
          <div className="text-center mt-6">
            <button
              onClick={() => setShowCreate(true)}
              className="px-6 py-2.5 rounded-lg text-black text-sm font-medium hover:brightness-110 transition"
              style={{ backgroundColor: 'var(--accent)' }}
            >
              Create Your First Agent
            </button>
          </div>
        </div>
      ) : (
        <>
          <div className="grid grid-cols-1 lg:grid-cols-3 gap-4 mb-6">
            {/* Agent cards */}
            <div className="lg:col-span-2 grid grid-cols-1 sm:grid-cols-2 gap-4">
              {state.agents.map((agent) => (
                <AgentCard
                  key={agent.id}
                  agent={agent}
                  onClick={() => navigate(`/agent/${agent.id}`)}
                />
              ))}
            </div>

            {/* Activity feed */}
            <div
              className="rounded-lg border p-4"
              style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}
            >
              <h2 className="text-sm font-medium text-[var(--text-secondary)] mb-3">Activity</h2>
              <ActivityFeed entries={activity} agentNames={agentNames} />
            </div>
          </div>

          {/* Chat panel */}
          <ChatPanel agents={state.agents} apiPort={apiPort} hasApiKey={!!hasApiKey} />
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
