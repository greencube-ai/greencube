import { useState, useEffect, useRef, useCallback } from 'react';
import { useNavigate } from 'react-router-dom';
import { useApp } from '../context/AppContext';
import { getActivityFeed, getServerInfo, getAgentLineage } from '../lib/invoke';
import type { AgentLineage } from '../lib/invoke';
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
  const [apiPort, setApiPort] = useState(9000);
  const [lineageMap, setLineageMap] = useState<Record<string, AgentLineage>>({});
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

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

  // Fetch lineage for all agents
  useEffect(() => {
    if (state.agents.length === 0) return;
    Promise.all(
      state.agents.map(a => getAgentLineage(a.id).then(l => [a.id, l] as const).catch(() => null))
    ).then(results => {
      const map: Record<string, AgentLineage> = {};
      for (const r of results) {
        if (r) map[r[0]] = r[1];
      }
      setLineageMap(map);
    });
  }, [state.agents]);

  useEffect(() => {
    const unlistenRefresh = onActivityRefresh(() => { debouncedRefresh(); });
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
  const activeCount = state.agents.filter(a => a.status === 'active').length;
  const totalTasks = state.agents.reduce((sum, a) => sum + a.total_tasks, 0);

  return (
    <div>
      {/* Header */}
      <div className="flex items-center justify-between mb-8">
        <div>
          <h1 className="text-2xl font-bold mb-1">Habitat</h1>
          <p className="text-xs text-[var(--text-muted)]">
            {state.agents.length} agent{state.agents.length !== 1 ? 's' : ''}
            {activeCount > 0 && <span style={{ color: 'var(--accent)' }}> / {activeCount} active</span>}
            {totalTasks > 0 && <span> / {totalTasks} total tasks</span>}
            <span> / localhost:{apiPort}</span>
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
            message="Your habitat is empty"
            subtitle="Create an agent. Connect it via the Connect page. Watch it grow."
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
        <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
          {/* Agent cards */}
          <div className="lg:col-span-2">
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
              {state.agents.map((agent) => {
                const lineage = lineageMap[agent.id];
                return (
                  <AgentCard
                    key={agent.id}
                    agent={agent}
                    onClick={() => navigate(`/agent/${agent.id}`)}
                    parentName={lineage?.parent?.name}
                    childCount={lineage?.children?.length}
                  />
                );
              })}
            </div>
          </div>

          {/* Live activity stream */}
          <div>
            <div
              className="rounded-xl border overflow-hidden"
              style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}
            >
              <div className="px-4 py-3 border-b flex items-center justify-between" style={{ borderColor: 'var(--border)' }}>
                <span className="text-sm font-medium text-[var(--text-secondary)]">Live</span>
                {activity.length > 0 && (
                  <div className="flex items-center gap-1.5">
                    <div className="w-1.5 h-1.5 rounded-full status-pulse" style={{ backgroundColor: 'var(--accent)' }} />
                    <span className="text-[10px] text-[var(--text-muted)]">{activity.length} events</span>
                  </div>
                )}
              </div>
              <div className="p-3">
                <ActivityFeed
                  entries={activity}
                  agentNames={agentNames}
                  emptyMessage="Waiting for activity"
                />
              </div>
            </div>
          </div>
        </div>
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
