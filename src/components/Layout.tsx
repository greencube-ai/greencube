import { useEffect, useState } from 'react';
import { useLocation, useNavigate } from 'react-router-dom';
import { useApp } from '../context/AppContext';
import { getServerInfo, getUnreadCount, getNotifications, dismissAllNotifications, markNotificationRead } from '../lib/invoke';
import { onToast } from '../lib/events';
import { listen } from '@tauri-apps/api/event';
import type { Notification } from '../lib/types';

interface LayoutProps {
  children: React.ReactNode;
}

const navItems = [
  { path: '/', label: 'Habitat' },
  { path: '/connect', label: 'Connect' },
  { path: '/settings', label: 'Settings' },
];

export function Layout({ children }: LayoutProps) {
  const location = useLocation();
  const navigate = useNavigate();
  const { state } = useApp();
  const [port, setPort] = useState(9000);
  const [toast, setToast] = useState<{ type: string; message: string } | null>(null);
  const [unreadCount, setUnreadCount] = useState(0);
  const [showNotifs, setShowNotifs] = useState(false);
  const [notifs, setNotifs] = useState<Notification[]>([]);

  useEffect(() => {
    getServerInfo().then((info) => setPort(info.port)).catch(() => {});
    getUnreadCount().then(setUnreadCount).catch(() => {});
    const interval = setInterval(() => {
      getUnreadCount().then(setUnreadCount).catch(() => {});
    }, 10000);
    const unlisten = listen('notification-new', () => {
      getUnreadCount().then(setUnreadCount).catch(() => {});
    });
    return () => { clearInterval(interval); unlisten.then(fn => fn()); };
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
        {/* Logo + notification bell */}
        <div className="p-4 flex items-center justify-between">
          <div className="flex items-center gap-2.5">
            <svg width="18" height="18" viewBox="0 0 512 512" className="logo-glow">
              <rect x="64" y="64" width="384" height="384" rx="48" ry="48" fill="none" stroke="#22C55E" strokeWidth="40"/>
            </svg>
            <span className="text-lg font-bold text-[var(--text-primary)]">GreenCube</span>
          </div>
          <button
            onClick={() => {
              setShowNotifs(!showNotifs);
              if (!showNotifs) {
                getNotifications(true, 20).then(setNotifs).catch(() => {});
              }
            }}
            className="relative p-1.5 rounded-md transition-colors hover:bg-[var(--bg-hover)]"
          >
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke={unreadCount > 0 ? '#22c55e' : 'currentColor'} strokeWidth="2" className="text-[var(--text-muted)]">
              <path d="M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9"/>
              <path d="M13.73 21a2 2 0 0 1-3.46 0"/>
            </svg>
            {unreadCount > 0 && (
              <span className="absolute -top-0.5 -right-0.5 w-4 h-4 rounded-full text-[9px] font-bold flex items-center justify-center" style={{ backgroundColor: 'var(--accent)', color: '#000' }}>
                {unreadCount > 9 ? '9+' : unreadCount}
              </span>
            )}
          </button>
        </div>

        {/* Notification panel */}
        {showNotifs && (
          <div className="mx-2 mb-2 rounded-lg border overflow-hidden" style={{ backgroundColor: 'var(--bg-tertiary)', borderColor: 'var(--border)' }}>
            <div className="px-3 py-2 border-b flex items-center justify-between" style={{ borderColor: 'var(--border)' }}>
              <span className="text-xs font-medium text-[var(--text-secondary)]">Notifications</span>
              {notifs.length > 0 && (
                <button
                  onClick={() => {
                    dismissAllNotifications().then(() => {
                      setNotifs([]);
                      setUnreadCount(0);
                      setShowNotifs(false);
                    }).catch(console.error);
                  }}
                  className="text-[10px] text-[var(--text-muted)] hover:text-[var(--text-primary)]"
                >
                  clear all
                </button>
              )}
            </div>
            <div className="max-h-48 overflow-y-auto">
              {notifs.length === 0 ? (
                <div className="px-3 py-4 text-xs text-[var(--text-muted)] text-center">no notifications</div>
              ) : (
                notifs.map((n) => (
                  <button
                    key={n.id}
                    onClick={() => {
                      markNotificationRead(n.id).catch(console.error);
                      setShowNotifs(false);
                      navigate(`/agent/${n.agent_id}`);
                      setUnreadCount(prev => Math.max(0, prev - 1));
                    }}
                    className="w-full text-left px-3 py-2.5 border-b hover:bg-[var(--bg-hover)] transition-colors"
                    style={{ borderColor: 'var(--border)' }}
                  >
                    <div className="text-xs text-[var(--text-primary)] leading-relaxed">{n.content}</div>
                    <div className="text-[10px] text-[var(--text-muted)] mt-1">
                      {new Date(n.created_at).toLocaleString()}
                    </div>
                  </button>
                ))
              )}
            </div>
          </div>
        )}

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
