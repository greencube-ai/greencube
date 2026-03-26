import { useEffect, useState } from 'react';
import { useLocation, useNavigate } from 'react-router-dom';
import { useApp } from '../context/AppContext';
import { getServerInfo } from '../lib/invoke';
import { onToast } from '../lib/events';

interface LayoutProps {
  children: React.ReactNode;
}

const navItems = [
  { path: '/', label: 'Dashboard', icon: '◻' },
  { path: '/connect', label: 'Connect', icon: '⚡' },
  { path: '/settings', label: 'Settings', icon: '⚙' },
];

export function Layout({ children }: LayoutProps) {
  const location = useLocation();
  const navigate = useNavigate();
  const { state } = useApp();
  const [port, setPort] = useState(9000);
  const [toast, setToast] = useState<{ type: string; message: string } | null>(null);

  useEffect(() => {
    getServerInfo().then((info) => setPort(info.port)).catch(() => {});
  }, []);

  // Toast listener
  useEffect(() => {
    const unlisten = onToast((data) => {
      setToast(data);
      setTimeout(() => setToast(null), 5000);
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  return (
    <div className="flex min-h-screen">
      {/* Sidebar */}
      <aside
        className="w-56 flex-shrink-0 flex flex-col border-r"
        style={{
          backgroundColor: 'var(--bg-secondary)',
          borderColor: 'var(--border)',
        }}
      >
        {/* Logo */}
        <div className="p-4 flex items-center gap-2.5">
          <svg width="20" height="20" viewBox="0 0 512 512" className="logo-glow" style={{ borderRadius: '3px' }}>
            {/* Green face (front-left) */}
            <polygon points="56.34,198.5 256,83 256,313 56.34,428.5" fill="#22C55E"/>
            {/* Red face (top) */}
            <polygon points="56.34,198.5 256,313 455.66,198.5 256,83" fill="#EF4444"/>
            {/* Blue face (right) */}
            <polygon points="256,83 455.66,198.5 455.66,428.5 256,313" fill="#3B82F6"/>
            {/* Grid lines - subtle */}
            <line x1="123" y1="237" x2="256" y2="160" stroke="#111" strokeWidth="3" opacity="0.35"/>
            <line x1="189" y1="275" x2="256" y2="236" stroke="#111" strokeWidth="3" opacity="0.35"/>
            <line x1="323" y1="160" x2="256" y2="236" stroke="#111" strokeWidth="3" opacity="0.35"/>
            <line x1="389" y1="198" x2="256" y2="275" stroke="#111" strokeWidth="3" opacity="0.35"/>
          </svg>
          <span className="text-lg font-bold text-[var(--text-primary)]">GreenCube</span>
        </div>

        {/* Navigation */}
        <nav className="flex-1 px-2 py-2">
          {navItems.map((item) => {
            const isActive = location.pathname === item.path;
            return (
              <button
                key={item.path}
                onClick={() => navigate(item.path)}
                className={`w-full text-left px-3 py-2 rounded-md text-sm mb-1 flex items-center gap-2 transition-colors ${
                  isActive
                    ? 'text-[var(--accent)]'
                    : 'text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)]'
                }`}
                style={isActive ? { backgroundColor: 'var(--accent-subtle)' } : undefined}
              >
                <span>{item.icon}</span>
                {item.label}
              </button>
            );
          })}
        </nav>

        {/* Footer: status */}
        <div className="px-4 py-3 border-t space-y-1.5" style={{ borderColor: 'var(--border)' }}>
          <div className="flex items-center gap-2 text-xs text-[var(--text-muted)]">
            <div
              className={`w-2 h-2 rounded-full ${state.dockerAvailable ? 'status-pulse' : ''}`}
              style={{
                backgroundColor: state.dockerAvailable ? 'var(--status-active)' : 'var(--status-error)',
              }}
            />
            Docker {state.dockerAvailable ? 'Connected' : 'Unavailable'}
          </div>
          <div className="text-[10px] text-[var(--text-muted)] font-mono">
            API: localhost:{port}
          </div>
        </div>
      </aside>

      {/* Main content with page transition */}
      <main className="flex-1 overflow-y-auto">
        <div key={location.pathname} className="page-enter p-6">
          {children}
        </div>
      </main>

      {/* Toast notification */}
      {toast && (
        <div
          className="fixed top-4 right-4 z-50 px-4 py-3 rounded-lg border shadow-lg toast-enter text-sm"
          style={{
            backgroundColor: 'var(--bg-secondary)',
            borderColor: toast.type === 'error' ? 'var(--status-error)' : 'var(--accent)',
            color: toast.type === 'error' ? 'var(--status-error)' : 'var(--text-primary)',
          }}
        >
          {toast.message}
        </div>
      )}
    </div>
  );
}
