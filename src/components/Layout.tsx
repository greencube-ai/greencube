import { useEffect, useState } from 'react';
import { useLocation, useNavigate } from 'react-router-dom';
import { useApp } from '../context/AppContext';
import { getServerInfo } from '../lib/invoke';
import { onToast } from '../lib/events';

interface LayoutProps {
  children: React.ReactNode;
}

const navItems = [
  { path: '/', label: 'Dashboard' },
  { path: '/connect', label: 'Connect' },
  { path: '/settings', label: 'Settings' },
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
    <div className="flex h-screen overflow-hidden">
      {/* Sidebar — fixed, never scrolls */}
      <aside
        className="w-56 flex-shrink-0 flex flex-col border-r h-screen"
        style={{
          backgroundColor: 'var(--bg-secondary)',
          borderColor: 'var(--border)',
        }}
      >
        {/* Logo */}
        <div className="p-4 flex items-center gap-2.5">
          <svg width="20" height="20" viewBox="0 0 512 512" className="logo-glow">
            <polygon points="256,80 56,196 56,432 256,316" fill="#22C55E"/>
            <polygon points="256,80 456,196 456,432 256,316" fill="#1BA34E"/>
            <polygon points="256,80 56,196 256,316 456,196" fill="#34D36E"/>
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

      {/* Main content — scrolls independently, sidebar stays fixed */}
      <main className="flex-1 overflow-y-auto h-screen">
        <div className="px-8 py-6">
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
