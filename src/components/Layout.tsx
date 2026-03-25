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
            <polygon points="65.7,160 256,50 256,270 65.7,380" fill="#22C55E"/>
            <polygon points="65.7,160 256,270 446.3,160 256,50" fill="#F0F0F0"/>
            <polygon points="256,50 446.3,160 446.3,380 256,270" fill="#EF4444"/>
            <line x1="129" y1="197" x2="256" y2="123" stroke="#1a1a1a" strokeWidth="3" opacity="0.3"/>
            <line x1="192" y1="233" x2="256" y2="197" stroke="#1a1a1a" strokeWidth="3" opacity="0.3"/>
            <line x1="320" y1="123" x2="256" y2="197" stroke="#1a1a1a" strokeWidth="3" opacity="0.3"/>
            <line x1="384" y1="160" x2="256" y2="233" stroke="#1a1a1a" strokeWidth="3" opacity="0.3"/>
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
