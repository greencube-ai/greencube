import { useEffect, useState, useCallback } from 'react';
import { useLocation, useNavigate } from 'react-router-dom';
import { getServerInfo, getUnreadCount, getNotifications, dismissAllNotifications, markNotificationRead } from '../lib/invoke';
import { listen } from '@tauri-apps/api/event';
import { onToast } from '../lib/events';
import type { Notification } from '../lib/types';

interface Toast { id: number; type: string; message: string; }
interface LayoutProps { children: React.ReactNode; }

const navItems = [
  { path: '/', label: 'Habitat' },
  { path: '/connect', label: 'Connect' },
  { path: '/settings', label: 'Settings' },
];

export function Layout({ children }: LayoutProps) {
  const location = useLocation();
  const navigate = useNavigate();
  const [port, setPort] = useState(9000);
  const [unreadCount, setUnreadCount] = useState(0);
  const [showNotifs, setShowNotifs] = useState(false);
  const [notifs, setNotifs] = useState<Notification[]>([]);
  const [toasts, setToasts] = useState<Toast[]>([]);

  const addToast = useCallback((type: string, message: string) => {
    const id = Date.now() + Math.random();
    setToasts(prev => [...prev, { id, type, message }]);
    setTimeout(() => setToasts(prev => prev.filter(t => t.id !== id)), 4000);
  }, []);

  useEffect(() => {
    getServerInfo().then((info) => setPort(info.port)).catch(() => {});
    getUnreadCount().then(setUnreadCount).catch(() => {});
    const interval = setInterval(() => { getUnreadCount().then(setUnreadCount).catch(() => {}); }, 10000);
    const unlisten = listen('notification-new', () => { getUnreadCount().then(setUnreadCount).catch(() => {}); });
    const unlistenToast = onToast((data) => addToast(data.type, data.message));
    return () => { clearInterval(interval); unlisten.then(fn => fn()); unlistenToast.then(fn => fn()); };
  }, [addToast]);

  return (
    <div className="flex h-screen overflow-hidden">
      <aside className="w-52 flex-shrink-0 flex flex-col border-r h-screen" style={{ backgroundColor: 'var(--bg-secondary)', borderColor: 'var(--border)' }}>
        {/* Logo + bell */}
        <div className="p-4 flex items-center justify-between">
          <div className="flex items-center gap-2">
            <svg width="16" height="16" viewBox="0 0 512 512">
              <rect x="64" y="64" width="384" height="384" rx="48" ry="48" fill="none" stroke="#22C55E" strokeWidth="40"/>
            </svg>
            <span className="text-base font-bold">GreenCube</span>
          </div>
          <button onClick={() => { setShowNotifs(!showNotifs); if (!showNotifs) getNotifications(true, 20).then(setNotifs).catch(() => {}); }}
            className="relative p-1 rounded-md hover:bg-[var(--bg-hover)]">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke={unreadCount > 0 ? '#22c55e' : 'currentColor'} strokeWidth="2" className="text-[var(--text-muted)]">
              <path d="M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9"/><path d="M13.73 21a2 2 0 0 1-3.46 0"/>
            </svg>
            {unreadCount > 0 && <span className="absolute -top-0.5 -right-0.5 w-3.5 h-3.5 rounded-full text-[8px] font-bold flex items-center justify-center" style={{ backgroundColor: 'var(--accent)', color: '#000' }}>{unreadCount > 9 ? '9+' : unreadCount}</span>}
          </button>
        </div>

        {/* Notifications */}
        {showNotifs && (
          <div className="mx-2 mb-2 rounded-lg border overflow-hidden" style={{ backgroundColor: 'var(--bg-tertiary)', borderColor: 'var(--border)' }}>
            <div className="px-3 py-2 border-b flex items-center justify-between" style={{ borderColor: 'var(--border)' }}>
              <span className="text-xs font-medium text-[var(--text-secondary)]">Notifications</span>
              {notifs.length > 0 && <button onClick={() => { dismissAllNotifications().then(() => { setNotifs([]); setUnreadCount(0); setShowNotifs(false); }); }} className="text-[10px] text-[var(--text-muted)]">clear</button>}
            </div>
            <div className="max-h-40 overflow-y-auto">
              {notifs.length === 0 ? <div className="px-3 py-3 text-xs text-[var(--text-muted)] text-center">none</div> : notifs.map((n) => (
                <button key={n.id} onClick={() => { markNotificationRead(n.id); setShowNotifs(false); navigate(`/agent/${n.agent_id}`); setUnreadCount(prev => Math.max(0, prev - 1)); }}
                  className="w-full text-left px-3 py-2 border-b hover:bg-[var(--bg-hover)] text-xs" style={{ borderColor: 'var(--border)' }}>
                  {n.content}
                </button>
              ))}
            </div>
          </div>
        )}

        {/* Nav */}
        <nav className="flex-1 px-2 py-1">
          {navItems.map((item) => {
            const isActive = location.pathname === item.path;
            return (
              <button key={item.path} onClick={() => navigate(item.path)}
                className={`w-full text-left px-3 py-2 rounded-md text-sm mb-0.5 transition-colors ${
                  isActive ? 'text-[var(--accent)]' : 'text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)]'
                }`} style={isActive ? { backgroundColor: 'var(--accent-subtle)' } : undefined}>
                {item.label}
              </button>
            );
          })}
        </nav>

        <div className="px-4 py-3 border-t text-[10px] text-[var(--text-muted)] font-mono" style={{ borderColor: 'var(--border)' }}>
          localhost:{port}
        </div>
      </aside>

      <main className="flex-1 overflow-y-auto h-screen">
        <div className="px-8 py-6">{children}</div>
      </main>

      {/* Toast notifications */}
      {toasts.length > 0 && (
        <div className="fixed bottom-4 right-4 flex flex-col gap-2 z-50">
          {toasts.map(toast => {
            const color = toast.type === 'verify_good' ? '#22c55e'
              : toast.type === 'verify_bad' ? '#eab308'
              : toast.type === 'learning' ? '#3b82f6'
              : toast.type === 'error' ? '#ef4444'
              : toast.type === 'warning' ? '#f97316'
              : '#71717a';
            const icon = toast.type === 'verify_good' ? '\u2713'
              : toast.type === 'verify_bad' ? '!'
              : toast.type === 'learning' ? '\u2726'
              : toast.type === 'error' ? '\u2717'
              : '\u2022';
            return (
              <div key={toast.id} className="toast-enter flex items-center gap-2 px-4 py-2.5 rounded-lg border text-sm"
                style={{
                  backgroundColor: 'var(--bg-secondary)',
                  borderColor: color + '30',
                  boxShadow: `0 4px 12px ${color}15`,
                  maxWidth: 320,
                }}>
                <span style={{ color, fontSize: 14, fontWeight: 700 }}>{icon}</span>
                <span className="text-[var(--text-secondary)]">{toast.message}</span>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
