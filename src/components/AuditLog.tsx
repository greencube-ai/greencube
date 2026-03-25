import type { AuditEntry } from '../lib/types';
import { EmptyState } from './EmptyState';

interface AuditLogProps {
  entries: AuditEntry[];
}

export function AuditLog({ entries }: AuditLogProps) {
  if (entries.length === 0) {
    return <EmptyState message="No audit entries yet" />;
  }

  return (
    <div className="overflow-x-auto">
      <table className="w-full text-xs">
        <thead>
          <tr className="text-left text-[var(--text-muted)] border-b" style={{ borderColor: 'var(--border)' }}>
            <th className="py-2 pr-4">Time</th>
            <th className="py-2 pr-4">Action</th>
            <th className="py-2 pr-4">Permission</th>
            <th className="py-2 pr-4">Duration</th>
            <th className="py-2 pr-4">Cost</th>
            <th className="py-2">Error</th>
          </tr>
        </thead>
        <tbody>
          {entries.map((entry, i) => {
            const time = new Date(entry.created_at).toLocaleTimeString();
            return (
              <tr
                key={entry.id}
                className="border-b hover:bg-[var(--bg-hover)]"
                style={{
                  borderColor: 'var(--border)',
                  backgroundColor: i % 2 === 0 ? 'var(--bg-secondary)' : 'var(--bg-primary)',
                }}
              >
                <td className="py-2 pr-4 text-[var(--text-muted)]">{time}</td>
                <td className="py-2 pr-4 text-[var(--text-primary)]">{entry.action_type}</td>
                <td className="py-2 pr-4">
                  <span
                    className="px-1.5 py-0.5 rounded text-[10px] font-medium"
                    style={{
                      color: entry.permission_result === 'allowed' ? 'var(--status-active)' : 'var(--status-error)',
                      backgroundColor:
                        entry.permission_result === 'allowed'
                          ? 'rgba(34, 197, 94, 0.1)'
                          : 'rgba(239, 68, 68, 0.1)',
                    }}
                  >
                    {entry.permission_result}
                  </span>
                </td>
                <td className="py-2 pr-4 text-[var(--text-muted)]">
                  {entry.duration_ms != null ? `${entry.duration_ms}ms` : '—'}
                </td>
                <td className="py-2 pr-4 text-[var(--text-muted)]">
                  {entry.cost_cents > 0 ? `$${(entry.cost_cents / 100).toFixed(2)}` : '—'}
                </td>
                <td className="py-2 text-[var(--status-error)]">
                  {entry.error || '—'}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
