interface StatusBadgeProps {
  status: 'idle' | 'active' | 'error';
}

const statusColors: Record<string, string> = {
  active: 'var(--status-active)',
  idle: 'var(--status-idle)',
  error: 'var(--status-error)',
};

export function StatusBadge({ status }: StatusBadgeProps) {
  const color = statusColors[status] || statusColors.idle;
  return (
    <div className="flex items-center gap-2 text-xs">
      <div
        className={`w-2 h-2 rounded-full ${status === 'active' ? 'status-pulse' : ''}`}
        style={{ backgroundColor: color }}
      />
      <span style={{ color }} className="capitalize">
        {status}
      </span>
    </div>
  );
}
