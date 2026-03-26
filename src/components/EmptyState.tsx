interface EmptyStateProps {
  message: string;
  subtitle?: string;
}

export function EmptyState({ message, subtitle }: EmptyStateProps) {
  return (
    <div className="flex flex-col items-center justify-center py-12 text-[var(--text-muted)]">
      <div
        className="w-12 h-12 rounded-xl flex items-center justify-center mb-4"
        style={{ backgroundColor: 'var(--bg-tertiary)' }}
      >
        <svg
          className="w-6 h-6 opacity-40"
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
        >
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth={1.5}
            d="M20 13V6a2 2 0 00-2-2H6a2 2 0 00-2 2v7m16 0v5a2 2 0 01-2 2H6a2 2 0 01-2-2v-5m16 0h-2.586a1 1 0 00-.707.293l-2.414 2.414a1 1 0 01-.707.293h-3.172a1 1 0 01-.707-.293l-2.414-2.414A1 1 0 006.586 13H4"
          />
        </svg>
      </div>
      <p className="text-sm font-medium mb-1">{message}</p>
      {subtitle && (
        <p className="text-xs text-[var(--text-muted)] opacity-60 max-w-xs text-center">{subtitle}</p>
      )}
    </div>
  );
}
