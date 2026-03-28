import { useState, useEffect, useRef, useCallback } from 'react';
import { useNavigate } from 'react-router-dom';
import { useApp } from '../context/AppContext';
import { getActivityFeed } from '../lib/invoke';
import { onActivityRefresh, onAgentStatusChange } from '../lib/events';
import { AgentCard } from '../components/AgentCard';
import { ActivityFeed } from '../components/ActivityFeed';
import { CreateAgentModal } from '../components/CreateAgentModal';
import { EmptyState } from '../components/EmptyState';
import type { AuditEntry } from '../lib/types';

export function Dashboard() {
  const { state, dispatch, refreshAgents } = useApp();
  const navigate = useNavigate();
  const [showCreate, setShowCreate] = useState(false);
  const [activity, setActivity] = useState<AuditEntry[]>([]);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const activityRef = useRef<string>('');
  const updateActivity = useCallback((entries: AuditEntry[]) => {
    const key = entries.length > 0 ? entries[0].id + entries.length : '';
    if (key !== activityRef.current) {
      activityRef.current = key;
      setActivity(entries);
    }
  }, []);

  useEffect(() => {
    getActivityFeed(50).then(updateActivity).catch(console.error);
    const interval = setInterval(() => {
      getActivityFeed(50).then(updateActivity).catch(console.error);
    }, 5000);
    return () => clearInterval(interval);
  }, [updateActivity]);

  useEffect(() => {
    const unlistenRefresh = onActivityRefresh(() => {
      getActivityFeed(50).then(updateActivity).catch(console.error);
    });
    const unlistenStatus = onAgentStatusChange((data) => {
      dispatch({ type: 'UPDATE_AGENT_STATUS', id: data.id, status: data.status });
    });
    return () => {
      unlistenRefresh.then((fn) => fn());
      unlistenStatus.then((fn) => fn());
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, [dispatch, updateActivity]);

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

  return (
    <div>
      <div className="flex items-center justify-between mb-8">
        <h1 className="text-2xl font-bold">Habitat</h1>
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
        <EmptyState
          message="Your habitat is empty"
          subtitle="Create an agent and connect it via the Connect page."
        />
      ) : (
        <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
          <div className="lg:col-span-2">
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
              {state.agents.map((agent) => (
                <AgentCard key={agent.id} agent={agent} onClick={() => navigate(`/agent/${agent.id}`)} />
              ))}
            </div>
          </div>
          <div>
            <div className="rounded-xl border overflow-hidden" style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}>
              <div className="px-4 py-3 border-b flex items-center justify-between" style={{ borderColor: 'var(--border)' }}>
                <span className="text-sm font-medium text-[var(--text-secondary)]">Live</span>
                {activity.length > 0 && (
                  <div className="flex items-center gap-1.5">
                    <div className="w-1.5 h-1.5 rounded-full status-pulse" style={{ backgroundColor: 'var(--accent)' }} />
                    <span className="text-[10px] text-[var(--text-muted)]">{activity.length}</span>
                  </div>
                )}
              </div>
              <div className="p-3">
                <ActivityFeed entries={activity} agentNames={agentNames} emptyMessage="Waiting for activity" />
              </div>
            </div>
          </div>
        </div>
      )}

      <CreateAgentModal
        isOpen={showCreate}
        onClose={() => setShowCreate(false)}
        onCreated={(agent) => { dispatch({ type: 'ADD_AGENT', agent }); refreshAgents(); }}
      />
    </div>
  );
}
